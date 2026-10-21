//! Plain terminal text. JSON stays on the machine path and is never colored.
use crate::error::Error;
use crate::runtime::RuntimeInventory;
use std::io::{self, Write};
use std::time::{Duration, Instant};

const GREEN: &str = "32";
const YELLOW: &str = "33";
const RED: &str = "31";
const CYAN: &str = "36";

pub fn columns() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|width| (40..=500).contains(width))
        .unwrap_or(80)
}

pub fn human_pair(bytes: u64) -> String {
    format!(
        "{} ({})",
        scaled(bytes, 1000, &["B", "KB", "MB", "GB", "TB"]),
        scaled(bytes, 1024, &["B", "KiB", "MiB", "GiB", "TiB"])
    )
}

fn scaled(bytes: u64, base: u64, units: &[&str]) -> String {
    if bytes < base {
        return format!("{bytes} {}", units[0]);
    }
    let mut value = bytes as f64;
    let mut index = 0;
    let step = base as f64;
    while value >= step && index + 1 < units.len() {
        value /= step;
        index += 1;
    }
    format!("{value:.1} {}", units[index])
}

pub fn short_id(id: &str) -> String {
    let short: String = id.chars().take(8).collect();
    if short.is_empty() {
        "unknown".into()
    } else {
        short
    }
}

fn paint(on: bool, code: &str, text: &str) -> String {
    if on {
        format!("\u{1b}[{code}m{text}\u{1b}[0m")
    } else {
        text.to_owned()
    }
}

fn clip(text: &str, columns: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= columns {
        return text.to_owned();
    }
    if columns < 5 {
        return chars.into_iter().take(columns).collect();
    }
    let keep = columns - 3;
    let left = keep / 2;
    let right = keep - left;
    let mut out: String = chars[..left].iter().collect();
    out.push_str("...");
    out.extend(chars[chars.len() - right..].iter());
    out
}

fn shorten_middle(text: &str, max: usize) -> String {
    clip(text, max)
}

fn verdict(color: bool, columns: usize, code: &str, word: &str, rest: &str) -> String {
    let plain = clip(&format!("{word}{rest}"), columns);
    match plain.strip_prefix(word) {
        Some(tail) => format!("{}{tail}", paint(color, code, word)),
        None => plain,
    }
}

fn next_line(color: bool, columns: usize, command: &str) -> String {
    let plain = clip(&format!("next: {command}"), columns);
    match plain.strip_prefix("next: ") {
        Some(command) => format!("next: {}", paint(color, CYAN, command)),
        None => plain,
    }
}

fn field(columns: usize, label: &str, value: &str) -> String {
    let prefix = format!("  {label}  ");
    let room = columns.saturating_sub(prefix.chars().count()).max(1);
    clip(&format!("{prefix}{}", shorten_middle(value, room)), columns)
}

pub fn error_text(error: &Error, color: bool, columns: usize) -> String {
    let headline = clip(&format!("error: {}", friendly(error)), columns);
    let first = match headline.strip_prefix("error") {
        Some(rest) => format!("{}{rest}", paint(color, RED, "error")),
        None => headline,
    };
    format!("{first}\n{}", next_line(color, columns, &next_for(error)))
}

pub fn clap_text(message: &str, color: bool, columns: usize) -> String {
    let line = message
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("invalid arguments");
    let line = line.trim().trim_start_matches("error:").trim();
    let headline = clip(&format!("error: {line}"), columns);
    let first = match headline.strip_prefix("error") {
        Some(rest) => format!("{}{rest}", paint(color, RED, "error")),
        None => headline,
    };
    format!(
        "{first}\n{}",
        next_line(color, columns, "modelprepper --help")
    )
}

fn friendly(error: &Error) -> String {
    if error.code == "input_read_failed" && is_missing(&error.message) {
        let path = error
            .message
            .split(':')
            .next()
            .unwrap_or(error.message.as_str())
            .trim();
        return format!("cannot read {path} (file not found)");
    }
    error
        .message
        .split(" (os error")
        .next()
        .unwrap_or(error.message.as_str())
        .trim()
        .to_owned()
}

fn is_missing(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("os error 2")
        || lower.contains("not found")
        || lower.contains("no such file")
        || lower.contains("cannot find the file")
}

fn next_for(error: &Error) -> String {
    match error.code {
        "input_read_failed" if is_missing(&error.message) => {
            let path = error
                .message
                .split(':')
                .next()
                .unwrap_or("config.toml")
                .trim();
            format!("modelprepper config init --output {path} --catalog <dir> --volume <dir>")
        }
        "config_write_failed" => {
            "modelprepper config init --output <new-file> --catalog <dir> --volume <dir>".into()
        }
        "overlapping_vault_paths" => {
            "choose a catalog directory and a volume directory that do not overlap".into()
        }
        "invalid_config" => "modelprepper config check --config <file>".into(),
        "invalid_usage" => "modelprepper --help".into(),
        "license_needs_review" => "review the license text, then propose the commit again".into(),
        "proposal_not_pending" | "invalid_proposal" => {
            "modelprepper proposals list --config <file>".into()
        }
        "volume_unavailable" | "volume_identity_mismatch" => {
            "modelprepper status --config <file>".into()
        }
        "vault_not_ready" => "modelprepper init --config <file>".into(),
        "repository_not_found_or_private" => {
            "check the repository id; private and missing repos look the same".into()
        }
        "revision_not_found" => {
            "modelprepper resolve --repo <org/model> --revision <commit>".into()
        }
        "path_not_found" | "unsafe_path" => {
            "pass a real directory, not a file, symlink, or missing path".into()
        }
        "evidence_mismatch" => "restore the reviewed license file, then run adopt again".into(),
        "destination_conflict" => "modelprepper status --config <file>".into(),
        "source_remove_failed" => {
            "the preserved copy was kept; inspect the source file named above".into()
        }
        _ => "modelprepper --help".into(),
    }
}

pub fn render_status(status: &crate::vault::Status, color: bool, columns: usize) -> String {
    let (code, word) = if status.state == "ready" {
        (GREEN, "ready")
    } else {
        (YELLOW, "blocked")
    };
    let mut lines = vec![verdict(
        color,
        columns,
        code,
        word,
        &format!("  vault {}", short_id(&status.vault_id)),
    )];
    for volume in &status.volumes {
        let space = match volume.available_bytes {
            Some(bytes) => format!("{} free", human_pair(bytes)),
            None => "space not measured".into(),
        };
        // The path is clipped on its own line. Putting it beside the state
        // let a long Windows path swallow "mounted" inside the ellipsis.
        let head = format!("  {}  {}", volume.label, volume.state);
        if head.chars().count() + 2 + space.chars().count() <= columns {
            lines.push(format!("{head}  {space}"));
        } else {
            lines.push(clip(&head, columns));
            lines.push(clip(&format!("  {space}"), columns));
        }
        lines.push(field(columns, "path", &volume.path.display().to_string()));
    }
    if status.preserved.is_empty() {
        lines.push("  preserved  none yet".into());
    } else {
        for copy in &status.preserved {
            lines.push(field(
                columns,
                "preserved",
                &format!("{} @ {}", copy.repo_id, short_id(&copy.resolved_revision)),
            ));
        }
    }
    let command = if status
        .volumes
        .iter()
        .any(|volume| volume.state != "mounted")
    {
        "reconnect the disk, then modelprepper status --config <file>"
    } else if status.preserved.is_empty() {
        "modelprepper resolve --repo <org/model>"
    } else {
        "modelprepper local list --config <file>"
    };
    lines.push(next_line(color, columns, command));
    lines.join("\n")
}

pub fn render_config_created(path: &std::path::Path, color: bool, columns: usize) -> String {
    [
        verdict(color, columns, GREEN, "created", "  config"),
        field(columns, "path", &path.display().to_string()),
        "  vault not initialized".into(),
        next_line(color, columns, "modelprepper init --config <file>"),
    ]
    .join("\n")
}

pub fn render_config_check(watches: usize, volumes: usize, color: bool, columns: usize) -> String {
    [
        verdict(color, columns, GREEN, "valid", "  config"),
        format!("  {watches} watches  {volumes} volumes"),
        next_line(color, columns, "modelprepper init --config <file>"),
    ]
    .join("\n")
}

pub fn render_resolution(resolved: &crate::hub::Resolution, color: bool, columns: usize) -> String {
    let matched = resolved.inventory.license.status == crate::inventory::EvidenceStatus::Matched;
    let (code, word) = if matched {
        (GREEN, "pinned")
    } else {
        (YELLOW, "unreviewed")
    };
    let source = &resolved.inventory.source;
    [
        verdict(
            color,
            columns,
            code,
            word,
            &format!(
                "  {} @ {}",
                source.repo_id,
                short_id(&source.resolved_revision)
            ),
        ),
        field(
            columns,
            "license",
            &format!(
                "{}  {}",
                resolved.inventory.license.license_id,
                evidence_name(resolved.inventory.license.status)
            ),
        ),
        "  transfer not authorized".into(),
        next_line(
            color,
            columns,
            &format!(
                "modelprepper propose --config <file> --repo {}",
                source.repo_id
            ),
        ),
    ]
    .join("\n")
}

pub fn render_verification(
    report: &crate::verify::Verification,
    color: bool,
    columns: usize,
) -> String {
    let (code, word) = if report.ok {
        (GREEN, "verified")
    } else {
        (RED, "mismatch")
    };
    let mut lines = vec![verdict(
        color,
        columns,
        code,
        word,
        &format!(
            "  {} @ {}",
            report.repo_id,
            short_id(&report.resolved_revision)
        ),
    )];
    lines.push(format!(
        "  matched {}  excluded {}",
        report.selected.len(),
        report.excluded.len()
    ));
    if !report.excluded.is_empty() {
        lines.push(field(columns, "excluded", &report.excluded.join(", ")));
    }
    if !report.ok && !report.message.is_empty() {
        lines.push(field(columns, "files", &report.message));
    }
    let command = if report.ok {
        "modelprepper adopt --config <file> --plan <id> --path <dir> --copy"
    } else {
        "restore the named files, then run verify-local again"
    };
    lines.push(next_line(color, columns, command));
    lines.join("\n")
}

pub fn render_adoption(adoption: &crate::vault::Adoption, color: bool, columns: usize) -> String {
    let mut lines = vec![
        verdict(
            color,
            columns,
            GREEN,
            "preserved",
            &format!(
                "  {} @ {}",
                adoption.repo_id,
                short_id(&adoption.resolved_revision)
            ),
        ),
        field(
            columns,
            "files",
            &format!(
                "{} files  {}  SHA-256 matched upstream",
                adoption.file_count,
                human_pair(adoption.size_bytes)
            ),
        ),
        field(columns, "primary", &adoption.path.display().to_string()),
        field(
            columns,
            "license",
            &format!("{}, text kept with the copy", adoption.license_id),
        ),
        field(columns, "provenance", adoption.provenance),
    ];
    if !adoption.excluded.is_empty() {
        lines.push(field(
            columns,
            "excluded",
            &format!("{} (optional, absent)", adoption.excluded.join(", ")),
        ));
    }
    lines.push(next_line(
        color,
        columns,
        "modelprepper status --config <file>",
    ));
    lines.join("\n")
}

pub fn render_proposal(proposal: &crate::vault::Proposal, color: bool, columns: usize) -> String {
    let (code, word) = match proposal.decision {
        crate::vault::Decision::Approved => (GREEN, "approved"),
        crate::vault::Decision::Rejected => (YELLOW, "rejected"),
        crate::vault::Decision::Pending => (YELLOW, "pending"),
    };
    let repo = &proposal.review.inventory.source;
    [
        verdict(
            color,
            columns,
            code,
            word,
            &format!("  plan {}", short_id(&proposal.plan_id)),
        ),
        field(
            columns,
            "repo",
            &format!("{} @ {}", repo.repo_id, short_id(&repo.resolved_revision)),
        ),
        field(columns, "plan", &proposal.plan_id),
        "  transfer not authorized".into(),
        next_line(
            color,
            columns,
            if proposal.decision == crate::vault::Decision::Pending {
                "modelprepper decide --config <file> --plan <id> --decision reject"
            } else {
                "modelprepper proposals list --config <file>"
            },
        ),
    ]
    .join("\n")
}

pub fn render_proposals(page: &crate::vault::ProposalPage, color: bool, columns: usize) -> String {
    let rest_owned = if page.proposals.is_empty() {
        "  reviews".to_owned()
    } else {
        format!("  {} reviews", page.proposals.len())
    };
    let code = if page.proposals.is_empty() {
        YELLOW
    } else {
        GREEN
    };
    let word = if page.proposals.is_empty() {
        "none"
    } else {
        "listed"
    };
    let mut lines = vec![verdict(color, columns, code, word, &rest_owned)];
    for proposal in &page.proposals {
        lines.push(field(
            columns,
            &short_id(&proposal.plan_id),
            &format!(
                "{}  {} @ {}",
                decision_name(proposal.decision),
                proposal.repo_id,
                short_id(&proposal.resolved_revision)
            ),
        ));
    }
    lines.push(next_line(
        color,
        columns,
        if page.proposals.is_empty() {
            "modelprepper propose --config <file> --repo <org/model>"
        } else {
            "modelprepper proposals show --config <file> --plan <id>"
        },
    ));
    lines.join("\n")
}

pub fn render_plan(plan: &crate::plan::Plan, color: bool, columns: usize) -> String {
    let blockers = plan
        .blockers
        .iter()
        .map(|blocker| {
            serde_json::to_value(blocker)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "blocker".into())
        })
        .collect::<Vec<_>>()
        .join(", ");
    [
        verdict(color, columns, YELLOW, "preview", "  untrusted"),
        field(
            columns,
            "repo",
            &format!(
                "{} @ {}",
                plan.source.repo_id,
                short_id(&plan.source.resolved_revision)
            ),
        ),
        field(
            columns,
            "files",
            &format!(
                "{} files  {}",
                plan.selected_files.len(),
                human_pair(plan.payload_bytes)
            ),
        ),
        field(columns, "blockers", &blockers),
        next_line(color, columns, "this preview cannot authorize a transfer"),
    ]
    .join("\n")
}

pub fn render_runtime(report: &RuntimeInventory, color: bool, columns: usize) -> String {
    let plain = report.text();
    let mut lines: Vec<String> = plain.lines().map(|line| clip(line, columns)).collect();
    if color && !lines.is_empty() {
        let code = if lines[0].starts_with("recorded") {
            GREEN
        } else if lines[0].starts_with("partial")
            || lines[0].starts_with("unchecked")
            || lines[0].starts_with("none")
            || lines[0].starts_with("empty")
        {
            YELLOW
        } else {
            GREEN
        };
        let word = lines[0].split_whitespace().next().unwrap_or("");
        lines[0] = verdict(
            color,
            columns,
            code,
            word,
            lines[0].strip_prefix(word).unwrap_or(""),
        );
    }
    lines.join("\n")
}

pub struct ProgressLine<W> {
    out: W,
    terminal: bool,
    quiet: bool,
    columns: usize,
    expected: usize,
    phase: String,
    order: Vec<String>,
    bucket: i32,
    last_paint: Option<Instant>,
    file_started: Instant,
    active: String,
}

impl ProgressLine<io::Stderr> {
    pub fn stderr(terminal: bool, quiet: bool, columns: usize, expected: usize) -> Self {
        Self::new(io::stderr(), terminal, quiet, columns, expected)
    }
}

impl<W: Write> ProgressLine<W> {
    pub fn new(out: W, terminal: bool, quiet: bool, columns: usize, expected: usize) -> Self {
        Self {
            out,
            terminal,
            quiet,
            columns,
            expected,
            phase: String::new(),
            order: Vec::new(),
            bucket: -1,
            last_paint: None,
            file_started: Instant::now(),
            active: String::new(),
        }
    }

    pub fn update(&mut self, phase: &str, path: &str, done: u64, total: u64) {
        self.render(Instant::now(), phase, path, done, total);
    }

    pub fn render(&mut self, now: Instant, phase: &str, path: &str, done: u64, total: u64) {
        if self.quiet {
            return;
        }
        if phase != self.phase {
            self.phase = phase.to_owned();
            self.order.clear();
            self.active.clear();
            self.bucket = -1;
            self.last_paint = None;
        }
        if phase == "recording" {
            let line = clip("recording  catalog", self.columns);
            self.emit_plain(&line, self.terminal);
            return;
        }
        if path != self.active {
            self.active = path.to_owned();
            self.file_started = now;
            self.bucket = -1;
            self.last_paint = None;
            if !path.is_empty() && !self.order.iter().any(|seen| seen == path) {
                self.order.push(path.to_owned());
            }
        }
        let finished = total == 0 || done >= total;
        if self.terminal {
            let due = self
                .last_paint
                .map(|then| now.saturating_duration_since(then) >= Duration::from_millis(250))
                .unwrap_or(true);
            if !finished && !due {
                return;
            }
            self.last_paint = Some(now);
            let line = self.live_line(path, done, total, now);
            let _ = write!(self.out, "\r{line}\x1b[K");
            if finished {
                let name = shorten_middle(file_name(path), 30);
                let result = clip(&format!("ok  {name}  {}", human_pair(total)), self.columns);
                let _ = write!(self.out, "\r{result}\x1b[K\n");
            }
            let _ = self.out.flush();
            return;
        }
        let bucket = done
            .saturating_mul(10)
            .checked_div(total)
            .map(|value| value.min(10) as i32)
            .unwrap_or(10);
        if bucket <= self.bucket {
            return;
        }
        self.bucket = bucket;
        let line = self.live_line(path, done, total, now);
        self.emit_plain(&line, false);
    }

    fn live_line(&self, path: &str, done: u64, total: u64, now: Instant) -> String {
        let index = self.order.iter().position(|seen| seen == path).unwrap_or(0) + 1;
        let fraction = if self.expected > 0 {
            format!("{index}/{}  ", self.expected)
        } else {
            String::new()
        };
        let percent = done
            .saturating_mul(100)
            .checked_div(total)
            .unwrap_or(100)
            .min(100);
        let elapsed = now
            .saturating_duration_since(self.file_started)
            .as_secs_f64();
        let rate = if elapsed < 0.05 {
            0
        } else {
            (done as f64 / elapsed) as u64
        };
        let left = total.saturating_sub(done);
        let eta = if rate == 0 {
            "~? s".into()
        } else {
            let secs = left as f64 / rate as f64;
            if secs < 60.0 {
                format!("~{:.0} s", secs.max(1.0))
            } else {
                format!("~{:.0} min", (secs / 60.0).max(1.0))
            }
        };
        clip(
            &format!(
                "{}  {fraction}{}  {percent}%  {} of {}  {}/s  {eta}",
                self.phase,
                shorten_middle(file_name(path), 28),
                scaled(done, 1000, &["B", "KB", "MB", "GB", "TB"]),
                scaled(total, 1000, &["B", "KB", "MB", "GB", "TB"]),
                scaled(rate, 1000, &["B", "KB", "MB", "GB", "TB"]),
            ),
            self.columns,
        )
    }

    fn emit_plain(&mut self, line: &str, cr: bool) {
        if cr {
            let _ = write!(self.out, "\r{line}\x1b[K\n");
        } else {
            let _ = writeln!(self.out, "{line}");
        }
        let _ = self.out.flush();
    }
}

fn evidence_name(status: crate::inventory::EvidenceStatus) -> &'static str {
    match status {
        crate::inventory::EvidenceStatus::Matched => "matched",
        crate::inventory::EvidenceStatus::Unreviewed => "unreviewed",
        crate::inventory::EvidenceStatus::Conflicting => "conflicting",
    }
}

fn decision_name(decision: crate::vault::Decision) -> &'static str {
    match decision {
        crate::vault::Decision::Pending => "pending",
        crate::vault::Decision::Approved => "approved",
        crate::vault::Decision::Rejected => "rejected",
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\'])
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_show_decimal_and_binary_units() {
        assert_eq!(human_pair(63_809_069_056), "63.8 GB (59.4 GiB)");
        assert_eq!(human_pair(5504), "5.5 KB (5.4 KiB)");
        assert_eq!(human_pair(32), "32 B (32 B)");
    }

    #[test]
    fn plain_errors_name_a_next_step_without_os_numbers() {
        let error = Error::new(
            "input_read_failed",
            "nope.toml: No such file or directory (os error 2)",
        );
        let text = error_text(&error, false, 80);
        assert!(
            text.starts_with("error: cannot read nope.toml (file not found)"),
            "{text}"
        );
        assert!(text.contains("next: modelprepper config init --output nope.toml"));
        assert!(!text.contains("os error"));
        assert!(!text.contains('\u{1b}'));
        let colored = error_text(&error, true, 80);
        assert!(colored.contains("\u{1b}[31merror\u{1b}[0m"));
    }

    #[test]
    fn progress_rewrites_one_terminal_line_and_stays_bounded_when_piped() {
        let mut terminal = ProgressLine::new(Vec::<u8>::new(), true, false, 40, 4);
        let start = Instant::now();
        for file in 1..=4 {
            let path = format!("model-0000{file}.safetensors");
            terminal.render(start, "verifying", &path, 0, 100);
            terminal.render(
                start + Duration::from_millis(10),
                "verifying",
                &path,
                40,
                100,
            );
            terminal.render(
                start + Duration::from_millis(300),
                "verifying",
                &path,
                100,
                100,
            );
        }
        let text = String::from_utf8(terminal.out).unwrap();
        assert_eq!(text.matches("ok  ").count(), 4, "{text}");
        assert!(text.contains('\r'), "{text}");
        for line in text.split(['\n', '\r']) {
            let visible = line.replace("\u{1b}[K", "");
            assert!(visible.chars().count() <= 40, "{visible}");
        }

        let mut piped = ProgressLine::new(Vec::<u8>::new(), false, true, 80, 1);
        piped.render(start, "verifying", "weights.bin", 50, 100);
        assert!(piped.out.is_empty());

        let mut log = ProgressLine::new(Vec::<u8>::new(), false, false, 80, 1);
        for step in 0..=100 {
            log.render(start, "copying", "weights.bin", step, 100);
        }
        log.render(start, "recording", "catalog", 1, 1);
        let log_text = String::from_utf8(log.out).unwrap();
        assert!(!log_text.contains('\r'), "{log_text}");
        assert!(log_text.contains("recording  catalog"));
        let hashing = log_text
            .lines()
            .filter(|line| !line.starts_with("recording"))
            .count();
        assert!(hashing <= 11, "{hashing}");
    }

    #[test]
    fn status_keeps_volume_state_when_the_path_is_long() {
        let status = crate::vault::Status {
            schema_version: 1,
            state: "ready".into(),
            vault_id: "8397c2c3aaaaaaaaaaaaaaaaaaaaaaaa".into(),
            volumes: vec![crate::vault::VolumeStatus {
                label: "primary".into(),
                volume_id: "volume".into(),
                path: std::path::PathBuf::from(
                    r"\\?\C:\GitHub\ModelPrepper\target\docs-capture\volumes\primary\nested",
                ),
                state: "mounted",
                available_bytes: Some(463_856_467_968),
            }],
            preserved: Vec::new(),
        };
        let text = render_status(&status, false, 80);
        assert!(
            text.lines().all(|line| line.chars().count() <= 80),
            "{text}"
        );
        assert!(text.contains("  primary  mounted  "), "{text}");
        assert!(text.contains("path"), "{text}");
        assert!(!text.contains("...ounted"), "{text}");
    }

    #[test]
    fn receipt_uses_preserved_and_both_size_units_inside_eighty_columns() {
        let adoption = crate::vault::Adoption {
            schema_version: 1,
            status: "adopted",
            repo_id: "example/model".into(),
            resolved_revision: "0123456789abcdef0123456789abcdef01234567".into(),
            provenance: crate::vault::PROVENANCE_LOCAL_UPSTREAM,
            path: std::path::PathBuf::from("models/preserved/example/model/01234567"),
            moved: false,
            file_count: 3,
            size_bytes: 63_809_069_056,
            license_id: "apache-2.0".into(),
            excluded: vec!["README.md".into()],
        };
        let text = render_adoption(&adoption, false, 80);
        assert!(
            text.lines().all(|line| line.chars().count() <= 80),
            "{text}"
        );
        assert!(text.starts_with("preserved"));
        assert!(text.contains("63.8 GB"));
        assert!(text.contains("59.4 GiB"));
        assert!(text.contains("SHA-256 matched upstream"));
        assert!(text.contains("apache-2.0, text kept with the copy"));
        assert!(text.contains("upstream-verified (local source)"));
        assert!(text.contains("README.md"));
        assert!(!text.contains('\u{1b}'));
        let colored = render_adoption(&adoption, true, 80);
        assert!(colored.contains("\u{1b}[32mpreserved\u{1b}[0m"));
        assert!(!colored.contains("\u{1b}[32mexample"));
    }
}
