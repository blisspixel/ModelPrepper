//! Weekly OS timer that runs one `watch` pass. It does not approve a transfer.
use crate::{Error, Result};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const SERVICE: &str = "modelprepper-watch.service";
const TIMER: &str = "modelprepper-watch.timer";
const PLIST: &str = "com.modelprepper.watch.plist";
const TASK_FILE: &str = "ModelPrepper-Watch.xml";
const TASK_NAME: &str = r"\ModelPrepper\Watch";
const LABEL: &str = "com.modelprepper.watch";
const CALENDAR: &str = "Monday 00:05";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Host {
    Systemd,
    Launchd,
    TaskScheduler,
}

#[derive(Debug, Clone)]
pub struct SchedulePlan {
    pub host: Host,
    pub binary: PathBuf,
    pub config: PathBuf,
    pub stage: PathBuf,
    pub working_directory: PathBuf,
    pub unit_dir: PathBuf,
    pub uid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ScheduleFile {
    pub name: String,
    pub path: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ScheduleReport {
    pub schema_version: u32,
    pub action: String,
    pub host: Host,
    pub command: Vec<String>,
    pub config: String,
    pub stage: String,
    pub working_directory: String,
    pub timezone: String,
    pub calendar: String,
    pub process_limit: String,
    pub preview: String,
    pub files: Vec<ScheduleFile>,
    pub enable: Vec<Vec<String>>,
    pub remove: Vec<Vec<String>>,
    pub transfer_authorized: bool,
    pub workspace_bytes: Option<u64>,
}

pub trait HostRunner {
    fn run(&mut self, program: &str, args: &[String]) -> io::Result<()>;
}

pub struct SystemRunner;

impl HostRunner for SystemRunner {
    fn run(&mut self, program: &str, args: &[String]) -> io::Result<()> {
        let status = std::process::Command::new(program).args(args).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("{program} exited {status}")))
        }
    }
}

pub fn current_host() -> Host {
    if cfg!(windows) {
        Host::TaskScheduler
    } else if cfg!(target_os = "macos") {
        Host::Launchd
    } else {
        Host::Systemd
    }
}

pub fn unit_directory(
    host: Host,
    home: Option<&Path>,
    xdg_config_home: Option<&Path>,
    local_app_data: Option<&Path>,
) -> Result<PathBuf> {
    match host {
        Host::Systemd => {
            if let Some(xdg) = xdg_config_home.filter(|path| !path.as_os_str().is_empty()) {
                return absolute_dir(xdg).map(|path| path.join("systemd/user"));
            }
            absolute_dir(home.ok_or_else(missing_home)?)
                .map(|path| path.join(".config/systemd/user"))
        }
        Host::Launchd => absolute_dir(home.ok_or_else(missing_home)?)
            .map(|path| path.join("Library/LaunchAgents")),
        Host::TaskScheduler => {
            if let Some(local) = local_app_data.filter(|path| !path.as_os_str().is_empty()) {
                return absolute_dir(local).map(|path| path.join("ModelPrepper/schedule"));
            }
            absolute_dir(home.ok_or_else(missing_home)?)
                .map(|path| path.join("AppData/Local/ModelPrepper/schedule"))
        }
    }
}

pub fn preview(plan: &SchedulePlan) -> Result<ScheduleReport> {
    let mut report = render(plan)?;
    report.action = "preview".to_owned();
    report.preview = digest(&report);
    Ok(report)
}

pub fn install(
    plan: &SchedulePlan,
    preview_hex: &str,
    runner: &mut dyn HostRunner,
) -> Result<ScheduleReport> {
    let mut report = preview(plan)?;
    if preview_hex != report.preview {
        return Err(Error::new(
            "schedule_preview_mismatch",
            "the preview digest does not match this timer",
        ));
    }
    let mut written = Vec::new();
    for file in &report.files {
        if let Err(error) = write_unit(Path::new(&file.path), &file.body) {
            let _ = delete_paths(&written);
            return Err(failed("schedule_install_failed", &error));
        }
        written.push(PathBuf::from(&file.path));
    }
    // launchd bootstrap fails when the label is already loaded. A missing
    // agent makes bootout fail, and that is not an install failure.
    if plan.host == Host::Launchd
        && let Some(boot) = report.remove.first()
    {
        let _ = run_one(runner, boot);
    }
    if let Err(error) = run_all(runner, &report.enable) {
        let _ = delete_paths(&written);
        return Err(failed("schedule_install_failed", &error));
    }
    report.action = "installed".to_owned();
    Ok(report)
}

pub fn remove(
    host: Host,
    unit_dir: &Path,
    uid: u32,
    runner: &mut dyn HostRunner,
) -> Result<ScheduleReport> {
    let unit_dir = absolute_dir(unit_dir)?;
    let names = file_names(host);
    let paths: Vec<PathBuf> = names.iter().map(|name| unit_dir.join(name)).collect();
    let existed = paths.iter().any(|path| path.exists());
    let commands = remove_commands(host, uid, &paths);
    // Disable before deleting so a failed host command leaves the files in place.
    if let Some(disable) = commands.first()
        && let Err(error) = run_one(runner, disable)
    {
        return Err(failed("schedule_remove_failed", &error));
    }
    let mut deleted = false;
    for path in &paths {
        match fs::remove_file(path) {
            Ok(()) => deleted = true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(failed("schedule_remove_failed", &error)),
        }
    }
    if let Some(reload) = commands.get(1)
        && let Err(error) = run_one(runner, reload)
    {
        return Err(Error::new(
            "schedule_remove_failed",
            format!("unit files were removed; {error}"),
        ));
    }
    Ok(blank_report(
        host,
        if deleted || existed {
            "removed"
        } else {
            "absent"
        },
        commands,
    ))
}

fn render(plan: &SchedulePlan) -> Result<ScheduleReport> {
    let binary = absolute_text(&plan.binary)?;
    let config = absolute_text(&plan.config)?;
    let stage = absolute_text(&plan.stage)?;
    let working_directory = absolute_text(&plan.working_directory)?;
    let unit_dir = absolute_dir(&plan.unit_dir)?;
    let command = vec![
        binary.clone(),
        "watch".to_owned(),
        "--config".to_owned(),
        config.clone(),
        "--stage".to_owned(),
        stage.clone(),
    ];
    let (timezone, process_limit, files, enable, remove) = match plan.host {
        Host::Systemd => systemd(&command, &working_directory, &unit_dir)?,
        Host::Launchd => launchd(&command, &working_directory, &unit_dir, plan.uid)?,
        Host::TaskScheduler => task(&command, &working_directory, &unit_dir)?,
    };
    Ok(ScheduleReport {
        schema_version: 1,
        action: String::new(),
        host: plan.host,
        command,
        config,
        stage,
        working_directory,
        timezone,
        calendar: CALENDAR.to_owned(),
        process_limit,
        preview: String::new(),
        files,
        enable,
        remove,
        transfer_authorized: false,
        workspace_bytes: None,
    })
}

fn systemd(command: &[String], working_directory: &str, unit_dir: &Path) -> Result<Unit> {
    let exec = command
        .iter()
        .map(|arg| systemd_arg(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let service = format!(
        "[Unit]\nDescription=ModelPrepper weekly watch\n\n[Service]\nType=oneshot\nTimeoutStartSec=12h\nWorkingDirectory={workdir}\nExecStart={exec}\n# This timer runs watch once. It does not approve a transfer.\n",
        workdir = systemd_arg(working_directory),
    );
    let timer = "\
[Unit]\nDescription=ModelPrepper weekly watch\n\n[Timer]\nOnCalendar=Mon *-*-* 00:05:00 UTC\nPersistent=true\nUnit=modelprepper-watch.service\n\n[Install]\nWantedBy=timers.target\n";
    let files = vec![
        file(unit_dir, SERVICE, service),
        file(unit_dir, TIMER, timer.to_owned()),
    ];
    let enable = vec![
        words(&["systemctl", "--user", "daemon-reload"]),
        words(&["systemctl", "--user", "enable", "--now", TIMER]),
    ];
    let remove = vec![
        words(&["systemctl", "--user", "disable", "--now", TIMER]),
        words(&["systemctl", "--user", "daemon-reload"]),
    ];
    Ok(("UTC".to_owned(), "12h".to_owned(), files, enable, remove))
}

fn launchd(command: &[String], working_directory: &str, unit_dir: &Path, uid: u32) -> Result<Unit> {
    let args = command
        .iter()
        .map(|arg| format!("    <string>{}</string>\n", xml_text(arg)))
        .collect::<String>();
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key>\n  <string>{LABEL}</string>\n  <key>ProgramArguments</key>\n  <array>\n{args}  </array>\n  <key>WorkingDirectory</key>\n  <string>{}</string>\n  <key>RunAtLoad</key>\n  <false/>\n  <key>StartCalendarInterval</key>\n  <dict>\n    <key>Weekday</key>\n    <integer>1</integer>\n    <key>Hour</key>\n    <integer>0</integer>\n    <key>Minute</key>\n    <integer>5</integer>\n  </dict>\n</dict>\n</plist>\n<!-- This agent runs watch once. It does not approve a transfer. -->\n",
        xml_text(working_directory),
    );
    let plist_text = absolute_text(&unit_dir.join(PLIST))?;
    let domain = format!("gui/{uid}");
    let target = format!("{domain}/{LABEL}");
    let files = vec![file(unit_dir, PLIST, body)];
    let enable = vec![words(&["launchctl", "bootstrap", &domain, &plist_text])];
    let remove = vec![words(&["launchctl", "bootout", &target])];
    Ok(("local".to_owned(), "none".to_owned(), files, enable, remove))
}

fn task(command: &[String], working_directory: &str, unit_dir: &Path) -> Result<Unit> {
    let arguments = command[1..]
        .iter()
        .map(|arg| windows_arg(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n  <RegistrationInfo><URI>{TASK_NAME}</URI></RegistrationInfo>\n  <Triggers>\n    <CalendarTrigger>\n      <StartBoundary>2026-01-05T00:05:00Z</StartBoundary>\n      <Enabled>true</Enabled>\n      <ScheduleByWeek>\n        <DaysOfWeek><Monday/></DaysOfWeek>\n        <WeeksInterval>1</WeeksInterval>\n      </ScheduleByWeek>\n    </CalendarTrigger>\n  </Triggers>\n  <Principals>\n    <Principal>\n      <LogonType>InteractiveToken</LogonType>\n      <RunLevel>LeastPrivilege</RunLevel>\n    </Principal>\n  </Principals>\n  <Settings>\n    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\n    <StartWhenAvailable>true</StartWhenAvailable>\n    <ExecutionTimeLimit>PT12H</ExecutionTimeLimit>\n    <Enabled>true</Enabled>\n  </Settings>\n  <Actions Context=\"Author\">\n    <Exec>\n      <Command>{}</Command>\n      <Arguments>{}</Arguments>\n      <WorkingDirectory>{}</WorkingDirectory>\n    </Exec>\n  </Actions>\n</Task>\n<!-- This task runs watch once. It does not approve a transfer. -->\n",
        xml_text(&command[0]),
        xml_text(&arguments),
        xml_text(working_directory),
    );
    let xml_path = absolute_text(&unit_dir.join(TASK_FILE))?;
    let files = vec![file(unit_dir, TASK_FILE, body)];
    let enable = vec![words(&[
        "schtasks", "/Create", "/TN", TASK_NAME, "/XML", &xml_path, "/F",
    ])];
    let remove = vec![words(&["schtasks", "/Delete", "/TN", TASK_NAME, "/F"])];
    Ok(("UTC".to_owned(), "12h".to_owned(), files, enable, remove))
}

type Unit = (
    String,
    String,
    Vec<ScheduleFile>,
    Vec<Vec<String>>,
    Vec<Vec<String>>,
);

fn file(dir: &Path, name: &str, body: String) -> ScheduleFile {
    ScheduleFile {
        name: name.to_owned(),
        path: dir.join(name).to_string_lossy().into_owned(),
        body,
    }
}

fn blank_report(host: Host, action: &str, remove: Vec<Vec<String>>) -> ScheduleReport {
    ScheduleReport {
        schema_version: 1,
        action: action.to_owned(),
        host,
        command: Vec::new(),
        config: String::new(),
        stage: String::new(),
        working_directory: String::new(),
        timezone: String::new(),
        calendar: CALENDAR.to_owned(),
        process_limit: String::new(),
        preview: String::new(),
        files: Vec::new(),
        enable: Vec::new(),
        remove,
        transfer_authorized: false,
        workspace_bytes: None,
    }
}

fn digest(report: &ScheduleReport) -> String {
    let mut hasher = Sha256::new();
    hasher.update(host_name(report.host).as_bytes());
    hasher.update(b"\n");
    hasher.update(report.timezone.as_bytes());
    hasher.update(b"\n");
    hasher.update(report.calendar.as_bytes());
    hasher.update(b"\n");
    hasher.update(report.process_limit.as_bytes());
    hasher.update(b"\n");
    for arg in &report.command {
        hasher.update(arg.as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(b"\n");
    for file in &report.files {
        hasher.update(file.name.as_bytes());
        hasher.update(b"\n");
        hasher.update(file.path.as_bytes());
        hasher.update(b"\n");
        hasher.update(file.body.as_bytes());
        hasher.update(b"\n");
    }
    for command in report.enable.iter().chain(&report.remove) {
        for arg in command {
            hasher.update(arg.as_bytes());
            hasher.update(b"\0");
        }
        hasher.update(b"\n");
    }
    hex(&hasher.finalize())
}

fn systemd_arg(arg: &str) -> String {
    let simple = !arg.is_empty()
        && arg.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | ':' | '+' | '@')
        });
    if simple {
        return arg.to_owned();
    }
    let mut out = String::from("\"");
    for c in arg.chars() {
        match c {
            '%' => out.push_str("%%"),
            '\\' | '"' | '$' | '`' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn windows_arg(arg: &str) -> String {
    let simple = !arg.is_empty()
        && arg.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '/' | '\\' | '.' | '_' | '-' | ':' | '+')
        });
    if simple {
        return arg.to_owned();
    }
    let mut out = String::from("\"");
    for c in arg.chars() {
        if c == '"' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn xml_text(value: &str) -> String {
    let mut out = String::new();
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn absolute_text(path: &Path) -> Result<String> {
    let text = path
        .to_str()
        .ok_or_else(|| Error::new("schedule_unreadable", "the path is not valid Unicode"))?;
    if text.is_empty() || text.chars().any(|c| matches!(c, '\n' | '\r' | '\0')) {
        return Err(Error::new(
            "schedule_unreadable",
            "the path is empty or contains a newline",
        ));
    }
    if !path.is_absolute() {
        return Err(Error::new(
            "schedule_unreadable",
            "the binary, config, stage, and working directory must be absolute",
        ));
    }
    Ok(text.to_owned())
}

fn absolute_dir(path: &Path) -> Result<PathBuf> {
    absolute_text(path)?;
    Ok(path.to_path_buf())
}

fn missing_home() -> Error {
    Error::new(
        "schedule_unreadable",
        "HOME or the platform config directory is not set",
    )
}

fn write_unit(path: &Path, body: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    file.write_all(body.as_bytes())?;
    file.sync_all()
}

fn delete_paths(paths: &[PathBuf]) -> io::Result<()> {
    for path in paths {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn run_all(runner: &mut dyn HostRunner, commands: &[Vec<String>]) -> io::Result<()> {
    for command in commands {
        run_one(runner, command)?;
    }
    Ok(())
}

fn run_one(runner: &mut dyn HostRunner, command: &[String]) -> io::Result<()> {
    let Some((program, args)) = command.split_first() else {
        return Err(io::Error::other("the host command is empty"));
    };
    runner.run(program, args)
}

fn remove_commands(host: Host, uid: u32, paths: &[PathBuf]) -> Vec<Vec<String>> {
    match host {
        Host::Systemd => vec![
            words(&["systemctl", "--user", "disable", "--now", TIMER]),
            words(&["systemctl", "--user", "daemon-reload"]),
        ],
        Host::Launchd => {
            let _ = paths;
            vec![words(&[
                "launchctl",
                "bootout",
                &format!("gui/{uid}/{LABEL}"),
            ])]
        }
        Host::TaskScheduler => vec![words(&["schtasks", "/Delete", "/TN", TASK_NAME, "/F"])],
    }
}

fn file_names(host: Host) -> &'static [&'static str] {
    match host {
        Host::Systemd => &[SERVICE, TIMER],
        Host::Launchd => &[PLIST],
        Host::TaskScheduler => &[TASK_FILE],
    }
}

fn words(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

fn host_name(host: Host) -> &'static str {
    match host {
        Host::Systemd => "systemd",
        Host::Launchd => "launchd",
        Host::TaskScheduler => "task_scheduler",
    }
}

fn failed(code: &'static str, error: &io::Error) -> Error {
    Error::new(code, error.to_string())
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Record {
        calls: Vec<Vec<String>>,
        fail_at: Option<usize>,
    }

    impl HostRunner for Record {
        fn run(&mut self, program: &str, args: &[String]) -> io::Result<()> {
            let mut line = vec![program.to_owned()];
            line.extend(args.iter().cloned());
            let index = self.calls.len();
            self.calls.push(line);
            if self.fail_at == Some(index) {
                return Err(io::Error::other("host refused"));
            }
            Ok(())
        }
    }

    /// A leading slash is absolute on Unix and relative on Windows.
    fn host_path(path: &str) -> PathBuf {
        #[cfg(windows)]
        {
            PathBuf::from(format!("C:{path}"))
        }
        #[cfg(not(windows))]
        {
            PathBuf::from(path)
        }
    }

    /// Join with `/` so the rendered spelling does not gain a backslash on Windows.
    fn slash(root: &Path, suffix: &str) -> PathBuf {
        let root = root.to_str().expect("test path is utf-8");
        let root = root.trim_end_matches(['/', '\\']);
        PathBuf::from(format!("{root}/{suffix}"))
    }

    fn plan(host: Host, root: &Path) -> SchedulePlan {
        SchedulePlan {
            host,
            binary: slash(root, "bin/model prepper"),
            config: slash(root, "my dir/config.toml"),
            stage: slash(root, "stage dir/stage"),
            working_directory: slash(root, "my dir"),
            unit_dir: slash(root, "units"),
            uid: 501,
        }
    }

    fn assert_watch(report: &ScheduleReport) {
        assert_eq!(report.schema_version, 1);
        assert!(!report.transfer_authorized);
        assert!(report.workspace_bytes.is_none());
        assert_eq!(report.command[1], "watch");
        assert!(!report.command.iter().any(|arg| arg == "decide"));
        assert_eq!(report.calendar, "Monday 00:05");
        let joined = report
            .files
            .iter()
            .map(|file| file.body.as_str())
            .collect::<String>();
        assert!(!joined.contains("/bin/sh"));
        assert!(!joined.contains("cmd.exe"));
        assert!(joined.contains("does not approve a transfer"));
    }

    #[test]
    fn three_hosts_quote_paths_and_keep_the_weekly_clock() {
        let root = host_path("/tmp/modelprepper-schedule");
        let planned = plan(Host::Systemd, &root);
        let systemd = preview(&planned).unwrap();
        assert_watch(&systemd);
        assert_eq!(systemd.timezone, "UTC");
        assert_eq!(systemd.process_limit, "12h");
        let service = &systemd.files[0].body;
        assert!(service.contains("TimeoutStartSec=12h"), "{service}");
        let binary = planned.binary.to_str().unwrap();
        let config = planned.config.to_str().unwrap();
        assert!(service.contains(&format!("\"{binary}\"")), "{service}");
        assert!(service.contains(&format!("\"{config}\"")), "{service}");
        assert!(
            service.contains("OnCalendar=Mon *-*-* 00:05:00 UTC")
                || systemd.files[1].body.contains("00:05:00 UTC")
        );
        assert!(systemd.enable[0][0] == "systemctl");
        assert!(
            !systemd
                .files
                .iter()
                .any(|file| file.body.contains("gui/501"))
        );

        let odd = SchedulePlan {
            binary: host_path("/tmp/100%done\"app"),
            ..plan(Host::Systemd, &root)
        };
        let quoted = preview(&odd).unwrap();
        assert!(
            quoted.files[0].body.contains("100%%done\\\"app"),
            "{}",
            quoted.files[0].body
        );

        let launchd = preview(&plan(Host::Launchd, &root)).unwrap();
        assert_watch(&launchd);
        assert_eq!(launchd.timezone, "local");
        assert_eq!(launchd.process_limit, "none");
        let plist = &launchd.files[0].body;
        assert!(plist.contains("<integer>1</integer>"), "{plist}");
        assert!(plist.contains("<integer>5</integer>"), "{plist}");
        assert!(plist.contains("<false/>"), "{plist}");
        assert!(plist.contains("model prepper"), "{plist}");
        assert_eq!(launchd.enable[0][0], "launchctl");
        assert_eq!(launchd.enable[0][1], "bootstrap");
        assert_eq!(launchd.enable[0][2], "gui/501");

        let marked = SchedulePlan {
            binary: host_path("/tmp/a&b<c"),
            ..plan(Host::Launchd, &root)
        };
        let escaped = preview(&marked).unwrap();
        assert!(
            escaped.files[0].body.contains("/tmp/a&amp;b&lt;c"),
            "{}",
            escaped.files[0].body
        );
        assert!(!escaped.files[0].body.contains("/tmp/a&b<c"));

        let task_plan = plan(Host::TaskScheduler, &root);
        let task = preview(&task_plan).unwrap();
        assert_watch(&task);
        assert_eq!(task.timezone, "UTC");
        assert!(
            task.files[0].body.contains("2026-01-05T00:05:00Z"),
            "{}",
            task.files[0].body
        );
        assert!(
            task.files[0].body.contains("<Monday/>"),
            "{}",
            task.files[0].body
        );
        assert!(
            task.files[0].body.contains("LeastPrivilege"),
            "{}",
            task.files[0].body
        );
        assert!(
            task.files[0].body.contains("PT12H"),
            "{}",
            task.files[0].body
        );
        let task_binary = task_plan.binary.to_str().unwrap();
        assert!(
            task.files[0].body.contains("&quot;") && task.files[0].body.contains(task_binary),
            "{}",
            task.files[0].body
        );
        assert_eq!(task.enable[0][0], "schtasks");
        assert!(task.enable[0].contains(&TASK_NAME.to_owned()));
        assert_ne!(systemd.preview, launchd.preview);
        assert_ne!(systemd.preview, task.preview);
        assert_eq!(systemd.preview.len(), 64);
    }

    #[test]
    fn digest_changes_when_the_config_changes() {
        let root = host_path("/var/prep");
        let first = preview(&plan(Host::Systemd, &root)).unwrap();
        let mut other = plan(Host::Systemd, &root);
        other.config = host_path("/var/prep/other.toml");
        let second = preview(&other).unwrap();
        assert_ne!(first.preview, second.preview);
        assert!(
            preview(&SchedulePlan {
                binary: PathBuf::from("relative"),
                ..plan(Host::Systemd, &root)
            })
            .is_err()
        );
    }

    #[test]
    fn install_requires_the_digest_and_rolls_back_a_failed_host() {
        let dir = tempfile::tempdir().unwrap();
        let planned = plan(Host::Systemd, dir.path());
        let shown = preview(&planned).unwrap();
        let mut idle = Record {
            calls: Vec::new(),
            fail_at: None,
        };
        let mismatch = install(&planned, "nope", &mut idle).unwrap_err();
        assert_eq!(mismatch.code, "schedule_preview_mismatch");
        assert!(idle.calls.is_empty());
        assert!(!planned.unit_dir.exists());
        assert!(!planned.stage.exists());

        let mut runner = Record {
            calls: Vec::new(),
            fail_at: Some(1),
        };
        let failed = install(&planned, &shown.preview, &mut runner).unwrap_err();
        assert_eq!(failed.code, "schedule_install_failed");
        assert_eq!(runner.calls.len(), 2);
        assert!(!planned.unit_dir.join(SERVICE).exists());
        assert!(!planned.unit_dir.join(TIMER).exists());

        let mut runner = Record {
            calls: Vec::new(),
            fail_at: None,
        };
        let installed = install(&planned, &shown.preview, &mut runner).unwrap();
        assert_eq!(installed.action, "installed");
        assert!(!installed.transfer_authorized);
        assert!(installed.workspace_bytes.is_none());
        assert_eq!(runner.calls[0][0], "systemctl");
        assert_eq!(runner.calls[1][2], "enable");
        assert!(planned.unit_dir.join(SERVICE).is_file());
        let sibling = planned.unit_dir.join("other.service");
        fs::write(&sibling, "keep\n").unwrap();
        let again = install(&planned, &shown.preview, &mut runner).unwrap();
        assert_eq!(again.action, "installed");
        assert_eq!(fs::read_to_string(&sibling).unwrap(), "keep\n");

        let mut remover = Record {
            calls: Vec::new(),
            fail_at: Some(0),
        };
        let refused = remove(Host::Systemd, &planned.unit_dir, 501, &mut remover).unwrap_err();
        assert_eq!(refused.code, "schedule_remove_failed");
        assert!(planned.unit_dir.join(SERVICE).is_file());

        let mut remover = Record {
            calls: Vec::new(),
            fail_at: None,
        };
        let removed = remove(Host::Systemd, &planned.unit_dir, 501, &mut remover).unwrap();
        assert_eq!(removed.action, "removed");
        assert!(!removed.transfer_authorized);
        assert!(removed.workspace_bytes.is_none());
        assert!(!planned.unit_dir.join(SERVICE).exists());
        assert!(!planned.unit_dir.join(TIMER).exists());
        assert_eq!(fs::read_to_string(&sibling).unwrap(), "keep\n");
        let absent = remove(Host::Systemd, &planned.unit_dir, 501, &mut remover).unwrap();
        assert_eq!(absent.action, "absent");
    }

    #[test]
    fn launchd_ignores_a_missing_agent_and_task_remove_is_named() {
        let dir = tempfile::tempdir().unwrap();
        let planned = plan(Host::Launchd, dir.path());
        let shown = preview(&planned).unwrap();
        let mut runner = Record {
            calls: Vec::new(),
            fail_at: Some(1),
        };
        assert!(install(&planned, &shown.preview, &mut runner).is_err());
        assert!(!planned.unit_dir.join(PLIST).exists());
        assert_eq!(runner.calls[0][1], "bootout");
        let mut runner = Record {
            calls: Vec::new(),
            fail_at: None,
        };
        assert_eq!(
            install(&planned, &shown.preview, &mut runner)
                .unwrap()
                .action,
            "installed"
        );
        let removed = remove(Host::Launchd, &planned.unit_dir, 501, &mut runner).unwrap();
        assert_eq!(removed.action, "removed");
        assert_eq!(removed.remove[0][2], "gui/501/com.modelprepper.watch");
        assert!(!planned.unit_dir.join(PLIST).exists());

        let task = plan(Host::TaskScheduler, dir.path());
        let shown = preview(&task).unwrap();
        let installed = install(&task, &shown.preview, &mut runner).unwrap();
        assert!(installed.enable[0].contains(&r"\ModelPrepper\Watch".to_owned()));
        let removed = remove(Host::TaskScheduler, &task.unit_dir, 0, &mut runner).unwrap();
        assert_eq!(removed.remove[0][0], "schtasks");
        assert!(!task.unit_dir.join(TASK_FILE).exists());
    }

    #[test]
    fn unit_directory_uses_the_platform_home_and_rejects_a_relative_one() {
        let home = host_path("/home/keeper");
        let xdg = host_path("/xdg");
        let local = host_path("/local");
        assert_eq!(
            unit_directory(Host::Systemd, Some(&home), Some(&xdg), None).unwrap(),
            xdg.join("systemd/user")
        );
        assert_eq!(
            unit_directory(Host::Systemd, Some(&home), None, None).unwrap(),
            home.join(".config/systemd/user")
        );
        assert_eq!(
            unit_directory(Host::Launchd, Some(&home), None, None).unwrap(),
            home.join("Library/LaunchAgents")
        );
        assert_eq!(
            unit_directory(Host::TaskScheduler, Some(&home), None, Some(&local)).unwrap(),
            local.join("ModelPrepper/schedule")
        );
        assert_eq!(
            unit_directory(Host::TaskScheduler, Some(&home), None, None).unwrap(),
            home.join("AppData/Local/ModelPrepper/schedule")
        );
        assert!(unit_directory(Host::Systemd, None, None, None).is_err());
        assert!(unit_directory(Host::Systemd, Some(Path::new("relative")), None, None).is_err());
        assert_eq!(
            unit_directory(Host::Systemd, Some(&home), Some(Path::new("")), None).unwrap(),
            PathBuf::from("/home/keeper/.config/systemd/user")
        );
        assert!(
            preview(&SchedulePlan {
                config: host_path("/tmp/has\nnewline"),
                ..plan(Host::Systemd, &host_path("/tmp/prep"))
            })
            .is_err()
        );
        assert_eq!(
            current_host(),
            if cfg!(windows) {
                Host::TaskScheduler
            } else if cfg!(target_os = "macos") {
                Host::Launchd
            } else {
                Host::Systemd
            }
        );
        assert!(unit_directory(Host::Launchd, None, None, None).is_err());
        assert!(unit_directory(Host::TaskScheduler, None, None, None).is_err());
    }

    #[test]
    fn system_runner_reports_the_process_status() {
        let mut runner = SystemRunner;
        #[cfg(unix)]
        {
            runner.run("true", &[]).unwrap();
            assert!(runner.run("false", &[]).is_err());
        }
        #[cfg(windows)]
        {
            runner
                .run("cmd", &["/c".to_owned(), "exit".to_owned(), "0".to_owned()])
                .unwrap();
            assert!(
                runner
                    .run("cmd", &["/c".to_owned(), "exit".to_owned(), "1".to_owned()])
                    .is_err()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_path_is_refused() {
        use std::os::unix::ffi::OsStrExt;
        let path = PathBuf::from(std::ffi::OsStr::from_bytes(b"/tmp/bad\xff"));
        let error = absolute_text(&path).unwrap_err();
        assert_eq!(error.code, "schedule_unreadable");
    }

    #[test]
    fn reload_failure_after_delete_is_still_a_remove_error() {
        let dir = tempfile::tempdir().unwrap();
        let planned = plan(Host::Systemd, dir.path());
        let shown = preview(&planned).unwrap();
        let mut runner = Record {
            calls: Vec::new(),
            fail_at: None,
        };
        install(&planned, &shown.preview, &mut runner).unwrap();
        let mut runner = Record {
            calls: Vec::new(),
            fail_at: Some(1),
        };
        let error = remove(Host::Systemd, &planned.unit_dir, 0, &mut runner).unwrap_err();
        assert_eq!(error.code, "schedule_remove_failed");
        assert!(!planned.unit_dir.join(SERVICE).exists());
        assert!(error.message.contains("unit files were removed"));
    }

    #[test]
    fn text_preview_stays_within_the_terminal_width() {
        let report = preview(&plan(
            Host::Systemd,
            &host_path("/tmp/modelprepper-schedule"),
        ))
        .unwrap();
        let text = crate::present::render_schedule(&report, false, 80);
        assert!(
            text.lines().all(|line| line.chars().count() <= 80),
            "{text}"
        );
        assert!(text.contains("preview"), "{text}");
        assert!(text.contains(&report.preview), "{text}");
        assert!(text.contains("still blocked"), "{text}");
    }

    fn write_config(dir: &Path) -> (PathBuf, PathBuf) {
        // macOS temp directories sit under the /var symlink. Vault paths refuse that.
        let root = fs::canonicalize(dir).unwrap();
        let config = crate::Config::default_for(root.join("catalog"), root.join("models"));
        let text = toml::to_string_pretty(&config).unwrap();
        let path = root.join("config.toml");
        fs::write(&path, text).unwrap();
        fs::create_dir(root.join("stage-parent")).unwrap();
        (path, root.join("stage-parent").join("stage"))
    }

    #[test]
    fn the_command_previews_without_writing_and_installs_only_through_the_runner() {
        use clap::Parser;
        let dir = tempfile::tempdir().unwrap();
        let (config_path, stage) = write_config(dir.path());
        let units = dir.path().join("units");
        let config = config_path.to_str().unwrap();
        let stage_arg = stage.to_str().unwrap();
        let preview = crate::cli::Cli::try_parse_from([
            "modelprepper",
            "schedule",
            "preview",
            "--config",
            config,
            "--stage",
            stage_arg,
        ])
        .unwrap();
        let mut runner = Record {
            calls: Vec::new(),
            fail_at: None,
        };
        let output = crate::cli::execute_with(
            preview,
            crate::cli::View::piped(),
            &mut runner,
            Some(&units),
        )
        .unwrap();
        let crate::cli::CommandOutput::Json(value) = output else {
            panic!("preview is json when stdout is a pipe");
        };
        assert_eq!(value["action"], "preview");
        assert_eq!(value["transfer_authorized"], false);
        assert!(value["workspace_bytes"].is_null());
        assert!(
            value["command"]
                .as_array()
                .unwrap()
                .iter()
                .any(|arg| arg == "watch")
        );
        assert!(
            !value["command"]
                .as_array()
                .unwrap()
                .iter()
                .any(|arg| arg == "decide")
        );
        assert!(runner.calls.is_empty());
        assert!(!stage.exists());
        assert!(!units.exists());
        let digest = value["preview"].as_str().unwrap().to_owned();

        let refused = crate::cli::Cli::try_parse_from([
            "modelprepper",
            "schedule",
            "install",
            "--config",
            config,
            "--stage",
            stage_arg,
            "--preview",
            "nope",
        ])
        .unwrap();
        let error = crate::cli::execute_with(
            refused,
            crate::cli::View::piped(),
            &mut runner,
            Some(&units),
        )
        .unwrap_err();
        assert_eq!(error.code, "schedule_preview_mismatch");
        assert!(runner.calls.is_empty());

        let install = crate::cli::Cli::try_parse_from([
            "modelprepper",
            "schedule",
            "install",
            "--config",
            config,
            "--stage",
            stage_arg,
            "--preview",
            digest.as_str(),
        ])
        .unwrap();
        let text_view = crate::cli::View {
            text: true,
            ..crate::cli::View::piped()
        };
        let output =
            crate::cli::execute_with(install, text_view, &mut runner, Some(&units)).unwrap();
        let crate::cli::CommandOutput::Text { body, ok } = output else {
            panic!("text install");
        };
        assert!(ok);
        assert!(body.contains("installed"), "{body}");
        assert!(
            body.lines().all(|line| line.chars().count() <= 80),
            "{body}"
        );
        assert!(!stage.exists());
        assert!(!runner.calls.is_empty());
        assert_ne!(runner.calls[0][0], "modelprepper");
        let sibling = units.join("other.service");
        fs::write(&sibling, "keep\n").unwrap();

        let remove =
            crate::cli::Cli::try_parse_from(["modelprepper", "schedule", "remove"]).unwrap();
        let output =
            crate::cli::execute_with(remove, crate::cli::View::piped(), &mut runner, Some(&units))
                .unwrap();
        let crate::cli::CommandOutput::Json(value) = output else {
            panic!("remove json");
        };
        assert_eq!(value["action"], "removed");
        assert_eq!(value["transfer_authorized"], false);
        assert!(value["workspace_bytes"].is_null());
        assert_eq!(fs::read_to_string(&sibling).unwrap(), "keep\n");

        fs::create_dir_all(dir.path().join("models")).unwrap();
        let inside = dir.path().join("models").join("stage");
        let overlap = crate::cli::Cli::try_parse_from([
            "modelprepper",
            "schedule",
            "preview",
            "--config",
            config,
            "--stage",
            inside.to_str().unwrap(),
        ])
        .unwrap();
        let error = crate::cli::execute_with(
            overlap,
            crate::cli::View::piped(),
            &mut runner,
            Some(&units),
        )
        .unwrap_err();
        assert_eq!(error.code, "overlapping_vault_paths");
    }
}
