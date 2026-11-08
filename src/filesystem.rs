//! Name the filesystem under a path and decide which writes it may receive.
//!
//! ext4, APFS, and NTFS are the baseline. btrfs, xfs, tmpfs, and zfs stay
//! usable so a development tree and the temporary-directory tests keep
//! working. NFS and SMB may hold payload only. FAT, exFAT, an unknown type,
//! and a read-only mount are refused before a write. This module does not
//! mount a share and does not time a hard disk.

use crate::{Error, Result};
use serde::Serialize;
use std::{fs, io, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FilesystemRole {
    Baseline,
    Local,
    Payload,
    Refused,
    Unmeasured,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Placement {
    pub kind: String,
    pub role: FilesystemRole,
    pub read_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Purpose {
    Catalog,
    Staging,
    Payload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Observed {
    kind: String,
    read_only: bool,
    network: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Baseline,
    Local,
    Payload,
    Refused,
}

pub(crate) fn classify(path: &Path) -> Result<Placement> {
    let observed = observe(path)?;
    Ok(Placement {
        kind: observed.kind.clone(),
        role: class_role(classify_kind(&observed.kind, observed.network)),
        read_only: observed.read_only,
    })
}

pub(crate) fn admit(path: &Path, purpose: Purpose) -> Result<()> {
    let observed = observe(path)?;
    admit_observed(
        &observed.kind,
        observed.read_only,
        observed.network,
        purpose,
    )
}

pub(crate) fn admit_observed(
    kind: &str,
    read_only: bool,
    network: bool,
    purpose: Purpose,
) -> Result<()> {
    let kind = kind.trim().to_ascii_lowercase();
    if read_only {
        return Err(Error::new(
            "filesystem_read_only",
            format!("{kind} is mounted read-only"),
        ));
    }
    match (purpose, classify_kind(&kind, network)) {
        (Purpose::Payload, Class::Baseline | Class::Local | Class::Payload) => Ok(()),
        (Purpose::Catalog | Purpose::Staging, Class::Baseline | Class::Local) => Ok(()),
        (Purpose::Catalog, Class::Payload) => Err(Error::new(
            "unsupported_catalog_filesystem",
            format!("{kind} cannot hold the SQLite catalog"),
        )),
        (Purpose::Staging, Class::Payload) => Err(Error::new(
            "unsupported_staging_filesystem",
            format!("{kind} cannot hold the checkpoint journal"),
        )),
        (_, Class::Refused) => Err(Error::new(
            "unsupported_filesystem",
            format!("{kind} is not a supported filesystem"),
        )),
    }
}

fn classify_kind(kind: &str, network: bool) -> Class {
    if network {
        return Class::Payload;
    }
    match kind {
        "ext4" | "apfs" | "ntfs" => Class::Baseline,
        "btrfs" | "xfs" | "tmpfs" | "zfs" => Class::Local,
        "nfs" | "nfs4" | "cifs" | "smb" | "smb2" | "smb3" | "smbfs" | "afpfs" | "webdav" => {
            Class::Payload
        }
        _ => Class::Refused,
    }
}

fn class_role(class: Class) -> FilesystemRole {
    match class {
        Class::Baseline => FilesystemRole::Baseline,
        Class::Local => FilesystemRole::Local,
        Class::Payload => FilesystemRole::Payload,
        Class::Refused => FilesystemRole::Refused,
    }
}

fn observe(path: &Path) -> Result<Observed> {
    let path = existing_ancestor(path).map_err(unreadable)?;
    #[cfg(target_os = "linux")]
    {
        let text = fs::read_to_string("/proc/self/mountinfo").map_err(unreadable)?;
        mount_from_table(&text, &path).map_err(unreadable)
    }
    #[cfg(target_os = "macos")]
    {
        observe_macos(&path)
    }
    #[cfg(windows)]
    {
        observe_windows(&path)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = path;
        Err(Error::new(
            "filesystem_unreadable",
            "this operating system has no filesystem probe",
        ))
    }
}

fn unreadable(error: io::Error) -> Error {
    Error::new("filesystem_unreadable", error.to_string())
}

fn existing_ancestor(path: &Path) -> io::Result<std::path::PathBuf> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "filesystem checks need an absolute path",
        ));
    }
    let mut current = path.to_path_buf();
    while !current.exists() {
        if !current.pop() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no existing directory contains this path",
            ));
        }
    }
    fs::canonicalize(&current)
}

#[cfg(target_os = "macos")]
fn observe_macos(path: &Path) -> Result<Observed> {
    let stat = rustix::fs::statfs(path).map_err(|error| unreadable(error.into()))?;
    let vfs = rustix::fs::statvfs(path).map_err(|error| unreadable(error.into()))?;
    let kind = c_name(&stat.f_fstypename);
    if kind.is_empty() {
        return Err(Error::new(
            "filesystem_unreadable",
            "statfs returned an empty filesystem name",
        ));
    }
    Ok(Observed {
        kind,
        read_only: vfs.f_flag.contains(rustix::fs::StatVfsMountFlags::RDONLY),
        network: false,
    })
}

#[cfg(target_os = "macos")]
fn c_name(name: &[std::ffi::c_char]) -> String {
    let bytes = name
        .iter()
        .copied()
        .take_while(|unit| *unit != 0)
        .map(|unit| unit as u8)
        .collect::<Vec<_>>();
    String::from_utf8_lossy(&bytes).trim().to_ascii_lowercase()
}

#[cfg(windows)]
fn observe_windows(path: &Path) -> Result<Observed> {
    let probed = volume_kind::probe(path).map_err(unreadable)?;
    Ok(Observed {
        kind: probed.kind,
        read_only: probed.read_only,
        network: probed.remote,
    })
}

#[cfg(any(test, target_os = "linux"))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct Mount {
    point: String,
    kind: String,
    read_only: bool,
}

/// Pick the longest mount in a `/proc/self/mountinfo` table. A later row of
/// the same length wins. `ro` counts only as a mount option, not a super option.
#[cfg(any(test, target_os = "linux"))]
fn mount_from_table(table: &str, path: &Path) -> io::Result<Observed> {
    let path = path
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "path is not utf-8"))?;
    let mut best: Option<Mount> = None;
    for line in table.lines() {
        let Some(mount) = parse_mount(line) else {
            continue;
        };
        if !covers(&mount.point, path) {
            continue;
        }
        let longer = best
            .as_ref()
            .is_none_or(|current| mount.point.len() >= current.point.len());
        if longer {
            best = Some(mount);
        }
    }
    let Some(mount) = best else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no mount contains this path",
        ));
    };
    Ok(Observed {
        kind: mount.kind,
        read_only: mount.read_only,
        network: false,
    })
}

#[cfg(any(test, target_os = "linux"))]
fn parse_mount(line: &str) -> Option<Mount> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let separator = fields.iter().position(|field| *field == "-")?;
    if separator < 6 || separator + 1 >= fields.len() {
        return None;
    }
    let kind = fields[separator + 1].trim().to_ascii_lowercase();
    Some(Mount {
        point: unescape(fields[4]),
        kind,
        read_only: fields[5].split(',').any(|option| option == "ro"),
    })
}

#[cfg(any(test, target_os = "linux"))]
fn covers(mount: &str, path: &str) -> bool {
    mount == "/" || path == mount || path.starts_with(&format!("{mount}/"))
}

#[cfg(any(test, target_os = "linux"))]
fn unescape(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = String::with_capacity(field.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 4 <= bytes.len() {
            let octal = &bytes[index + 1..index + 4];
            if octal.iter().all(|unit| (b'0'..=b'7').contains(unit)) {
                let value = u16::from(octal[0] - b'0') * 64
                    + u16::from(octal[1] - b'0') * 8
                    + u16::from(octal[2] - b'0');
                if value <= 0xff {
                    out.push(char::from(value as u8));
                    index += 4;
                    continue;
                }
            }
        }
        out.push(char::from(bytes[index]));
        index += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "\
33 2 0:30 /@ / rw,relatime shared:1 - btrfs /dev/mapper/root rw,compress=zstd:3,ssd,space_cache=v2,subvolid=256,subvol=/@
58 33 0:30 /@home /home rw,relatime shared:152 - btrfs /dev/mapper/root rw,compress=zstd:3
90 33 0:31 / /mnt/old ro,noatime - ext4 /dev/sdb ro
91 33 0:32 / /mnt/disk\\040one rw,nosuid - ext4 /dev/sdc rw,errors=remount-ro
92 33 0:33 / /mnt/share rw - nfs 10.0.0.8:/export rw,vers=4.2
broken
1 2 3 - ext4
1 2 3 4 /mnt/cut rw -
";

    #[test]
    fn the_longest_mount_wins_and_escapes_stay_inside_the_field() {
        let home = mount_from_table(TABLE, Path::new("/home/pueo/Work")).unwrap();
        assert_eq!(home.kind, "btrfs");
        assert!(!home.read_only);
        let root = mount_from_table(TABLE, Path::new("/")).unwrap();
        assert_eq!(root.kind, "btrfs");
        let old = mount_from_table(TABLE, Path::new("/mnt/old/models")).unwrap();
        assert_eq!(old.kind, "ext4");
        assert!(old.read_only);
        let spaced = mount_from_table(TABLE, Path::new("/mnt/disk one/models")).unwrap();
        assert_eq!(spaced.kind, "ext4");
        assert!(!spaced.read_only);
        let share = mount_from_table(TABLE, Path::new("/mnt/share")).unwrap();
        assert_eq!(share.kind, "nfs");
        assert_eq!(
            mount_from_table(TABLE, Path::new("/mnt/disk"))
                .unwrap()
                .kind,
            "btrfs",
            "a space-escaped mount is not a prefix of the shorter name"
        );
        let nested = "\
1 1 0:1 / /home rw - tmpfs tmpfs rw
2 1 0:2 / / rw - ext4 /dev/root rw
";
        assert_eq!(
            mount_from_table(nested, Path::new("/home/vault"))
                .unwrap()
                .kind,
            "tmpfs"
        );
        assert_eq!(
            mount_from_table(nested, Path::new("/home2")).unwrap().kind,
            "ext4"
        );
    }

    #[test]
    fn a_later_equal_mount_replaces_the_earlier_one() {
        let table = "\
1 1 0:1 / /data rw - tmpfs tmpfs rw
2 1 0:2 / /data rw - xfs /dev/nvme0n1 rw
";
        let found = mount_from_table(table, Path::new("/data/vault")).unwrap();
        assert_eq!(found.kind, "xfs");
    }

    #[test]
    fn an_empty_name_and_a_missing_mount_fail_closed() {
        let table = "1 1 0:1 / /mnt/empty rw - ext4 /dev/empty rw\n";
        assert!(mount_from_table(table, Path::new("/nowhere")).is_err());
        assert!(mount_from_table("", Path::new("/tmp")).is_err());
        assert!(mount_from_table(TABLE, Path::new("/")).is_ok());
        let weird = "1 1 0:1 / /only rw,errors=remount-ro - ext4 /dev/sda rw\n";
        let found = mount_from_table(weird, Path::new("/only")).unwrap();
        assert!(!found.read_only);
        assert_eq!(unescape("a\\134b"), "a\\b");
        assert_eq!(unescape("tail\\04"), "tail\\04");
        assert_eq!(unescape("over\\777"), "over\\777");
    }

    #[test]
    fn policy_separates_baseline_local_payload_and_refused_filesystems() {
        for kind in ["ext4", "apfs", "ntfs", "EXT4", "APFS", "Ntfs"] {
            for purpose in [Purpose::Catalog, Purpose::Staging, Purpose::Payload] {
                admit_observed(kind, false, false, purpose).unwrap();
            }
            assert_eq!(
                class_role(classify_kind(&kind.to_ascii_lowercase(), false)),
                FilesystemRole::Baseline
            );
        }
        for kind in ["btrfs", "xfs", "tmpfs", "zfs"] {
            for purpose in [Purpose::Catalog, Purpose::Staging, Purpose::Payload] {
                admit_observed(kind, false, false, purpose).unwrap();
            }
            assert_eq!(
                class_role(classify_kind(kind, false)),
                FilesystemRole::Local
            );
        }
        for kind in [
            "nfs", "nfs4", "cifs", "smb", "smb2", "smb3", "smbfs", "afpfs", "webdav",
        ] {
            assert_eq!(
                admit_observed(kind, false, false, Purpose::Catalog)
                    .unwrap_err()
                    .code,
                "unsupported_catalog_filesystem"
            );
            assert_eq!(
                admit_observed(kind, false, false, Purpose::Staging)
                    .unwrap_err()
                    .code,
                "unsupported_staging_filesystem"
            );
            admit_observed(kind, false, false, Purpose::Payload).unwrap();
            assert_eq!(
                class_role(classify_kind(kind, false)),
                FilesystemRole::Payload
            );
        }
        assert_eq!(
            admit_observed("ntfs", false, true, Purpose::Catalog)
                .unwrap_err()
                .code,
            "unsupported_catalog_filesystem",
            "a remote drive that reports NTFS is still a network catalog"
        );
        admit_observed("ntfs", false, true, Purpose::Payload).unwrap();
        for kind in [
            "vfat", "msdos", "fat", "fat32", "exfat", "overlay", "fuse", "refs", "",
        ] {
            for purpose in [Purpose::Catalog, Purpose::Staging, Purpose::Payload] {
                let error = admit_observed(kind, false, false, purpose).unwrap_err();
                assert_eq!(error.code, "unsupported_filesystem", "{kind} {purpose:?}");
            }
            assert_eq!(
                class_role(classify_kind(kind, false)),
                FilesystemRole::Refused
            );
        }
        for purpose in [Purpose::Catalog, Purpose::Staging, Purpose::Payload] {
            assert_eq!(
                admit_observed("ext4", true, false, purpose)
                    .unwrap_err()
                    .code,
                "filesystem_read_only"
            );
            assert_eq!(
                admit_observed("unmeasured", false, false, purpose)
                    .unwrap_err()
                    .code,
                "unsupported_filesystem"
            );
        }
    }

    #[test]
    fn a_missing_child_uses_the_ancestor_and_does_not_create_it() {
        let missing = std::env::temp_dir().join("modelprepper-wave8-missing");
        assert!(!missing.exists());
        let placed = classify(&missing).unwrap();
        assert!(!missing.exists());
        assert!(!placed.read_only);
        assert!(matches!(
            placed.role,
            FilesystemRole::Baseline | FilesystemRole::Local
        ));
        let error = classify(Path::new("relative")).unwrap_err();
        assert_eq!(error.code, "filesystem_unreadable");
    }

    #[test]
    fn the_live_directory_matches_the_runner_or_a_known_local_kind() {
        let path = fs::canonicalize(std::env::temp_dir()).unwrap();
        let placed = classify(&path).unwrap();
        assert!(!placed.read_only, "{placed:?}");
        if std::env::var_os("GITHUB_ACTIONS").is_some() {
            let expected = if cfg!(windows) {
                "ntfs"
            } else if cfg!(target_os = "macos") {
                "apfs"
            } else {
                "ext4"
            };
            assert_eq!(placed.kind, expected, "{placed:?}");
            assert_eq!(placed.role, FilesystemRole::Baseline);
            let workspace = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
            let source = classify(&workspace).unwrap();
            assert_eq!(source.kind, expected, "{source:?}");
        } else {
            assert!(
                matches!(
                    placed.kind.as_str(),
                    "ext4" | "apfs" | "ntfs" | "btrfs" | "xfs" | "tmpfs" | "zfs"
                ),
                "{placed:?}"
            );
            assert!(matches!(
                placed.role,
                FilesystemRole::Baseline | FilesystemRole::Local
            ));
        }
        #[cfg(target_os = "linux")]
        {
            let text = fs::read_to_string("/proc/self/mountinfo").unwrap();
            let parsed = mount_from_table(&text, &path).unwrap();
            assert_eq!(placed.kind, parsed.kind);
            assert_eq!(placed.read_only, parsed.read_only);
        }
    }

    #[test]
    fn status_reports_the_same_filesystem_init_just_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let config = crate::Config::default_for(root.join("catalog"), root.join("models"));
        let status = crate::vault::initialize(&config).unwrap();
        let volume = &status.volumes[0];
        let placed = classify(&volume.path).unwrap();
        assert_eq!(volume.state, "mounted");
        assert_eq!(volume.filesystem, placed.kind);
        assert_eq!(volume.filesystem_role, placed.role);
        assert_eq!(volume.read_only, placed.read_only);
        assert!(matches!(
            volume.filesystem_role,
            FilesystemRole::Baseline | FilesystemRole::Local
        ));
        let renamed = root.join("models-away");
        fs::rename(&volume.path, &renamed).unwrap();
        let offline = crate::vault::status(&config).unwrap();
        assert_eq!(offline.volumes[0].state, "offline");
        assert_eq!(offline.volumes[0].filesystem, "unmeasured");
        assert_eq!(
            offline.volumes[0].filesystem_role,
            FilesystemRole::Unmeasured
        );
        assert!(!offline.volumes[0].read_only);
        fs::rename(&renamed, &volume.path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_path_is_not_matched_to_a_mount() {
        use std::os::unix::ffi::OsStrExt;
        let odd = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(&[0xff, 0xfe]));
        assert!(mount_from_table(TABLE, &odd).is_err());
    }
}
