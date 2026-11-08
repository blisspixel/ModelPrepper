//! Shared collection file. Preview lists explicit entries and does not download them.
use crate::{Error, Result};
use serde::Serialize;
use std::collections::BTreeSet;

const MAX_ENTRIES: usize = 100;
const MAX_NAME: usize = 80;
const MAX_NOTE: usize = 200;
const MAX_LABEL: usize = 64;
const REPRESENTATIONS: &[&str] = &[
    "original-required",
    "original-preferred",
    "selected-derivative",
];
/// Kit slot names. A ceiling does not fill a slot or assign an entry to one.
const CATEGORIES: &[&str] = &[
    "text",
    "coding",
    "vision",
    "image_generation",
    "video_generation",
    "speech_to_text",
    "text_to_speech",
    "embeddings",
    "reference",
];

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct CollectionReport {
    pub schema_version: u32,
    pub action: &'static str,
    pub supported: bool,
    pub name: String,
    pub replica: Option<String>,
    pub ceilings: Vec<CategoryCeiling>,
    pub entries: Vec<CollectionEntry>,
    pub entry_count: usize,
    pub shared_bases: Vec<SharedBase>,
    pub transfer_authorized: bool,
    pub network_contact: bool,
    pub vault_mutated: bool,
    pub downloads_started: bool,
    pub workspace_bytes: Option<u64>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct CollectionEntry {
    pub repo_id: String,
    pub revision: String,
    pub note: String,
    /// The operator's written choice. Absent means the file did not choose.
    pub representation: Option<String>,
    /// Declared base repository. Absent means the file did not name one.
    pub base_repo: Option<String>,
    /// Declared base revision. Absent means the file did not establish one.
    pub base_revision: Option<String>,
    /// Preview does not fetch or verify the declared base.
    pub base_preserved: bool,
    /// Preview does not contact a publisher, so this stays false.
    pub resolved: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct CategoryCeiling {
    pub category: String,
    pub bytes: u64,
    /// The operator's written cap. Preview does not charge entries against it.
    pub applied: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct SharedBase {
    pub base_repo: String,
    pub base_revision: String,
    /// How many entries name this exact base. Preview does not merge them.
    pub entry_count: usize,
    /// Preview does not drop an entry or claim a byte saving.
    pub deduplicated: bool,
}

pub fn preview(text: &str) -> Result<CollectionReport> {
    let table: toml::Table = toml::from_str(text)
        .map_err(|error| Error::new("invalid_collection", error.to_string()))?;
    if table
        .get("schema_version")
        .and_then(toml::Value::as_integer)
        != Some(1)
    {
        return Ok(empty_report(false));
    }
    reject_unknown(
        table.keys(),
        |key| {
            matches!(
                key,
                "schema_version" | "name" | "replica" | "entries" | "ceilings"
            )
        },
        "unknown field",
    )?;
    let name = required_string(&table, "name")?;
    check_text(&name, 1, MAX_NAME, "name")?;
    let replica = match table.get("replica") {
        None => None,
        Some(toml::Value::String(label)) => {
            check_label(label)?;
            Some(label.clone())
        }
        Some(_) => {
            return Err(Error::new(
                "invalid_collection",
                "replica must be a volume label",
            ));
        }
    };
    let ceilings = parse_ceilings(&table)?;
    let Some(items) = table.get("entries").and_then(toml::Value::as_array) else {
        return Err(Error::new("invalid_collection", "entries must be an array"));
    };
    if items.len() > MAX_ENTRIES {
        return Err(Error::new(
            "invalid_collection",
            format!("a collection lists at most {MAX_ENTRIES} entries"),
        ));
    }
    let mut entries = Vec::with_capacity(items.len());
    let mut seen = BTreeSet::new();
    for item in items {
        let entry = parse_entry(item)?;
        if !seen.insert((entry.repo_id.clone(), entry.revision.clone())) {
            return Err(Error::new(
                "invalid_collection",
                format!("duplicate entry {}@{}", entry.repo_id, entry.revision),
            ));
        }
        entries.push(entry);
    }
    let entry_count = entries.len();
    let shared_bases = shared_bases(&entries);
    Ok(CollectionReport {
        schema_version: 1,
        action: "preview",
        supported: true,
        name,
        replica,
        ceilings,
        entries,
        entry_count,
        shared_bases,
        transfer_authorized: false,
        network_contact: false,
        vault_mutated: false,
        downloads_started: false,
        workspace_bytes: None,
    })
}

fn empty_report(supported: bool) -> CollectionReport {
    CollectionReport {
        schema_version: 1,
        action: "preview",
        supported,
        name: String::new(),
        replica: None,
        ceilings: Vec::new(),
        entries: Vec::new(),
        entry_count: 0,
        shared_bases: Vec::new(),
        transfer_authorized: false,
        network_contact: false,
        vault_mutated: false,
        downloads_started: false,
        workspace_bytes: None,
    }
}

fn reject_unknown<'a>(
    keys: impl Iterator<Item = &'a String>,
    allowed: impl Fn(&str) -> bool,
    label: &str,
) -> Result<()> {
    let mut unknown: Vec<_> = keys.filter(|key| !allowed(key)).cloned().collect();
    unknown.sort();
    if let Some(key) = unknown.first() {
        return Err(Error::new("invalid_collection", format!("{label} {key}")));
    }
    Ok(())
}

fn required_string(table: &toml::Table, key: &str) -> Result<String> {
    match table.get(key) {
        Some(toml::Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(Error::new(
            "invalid_collection",
            format!("{key} must be a string"),
        )),
        None => Err(Error::new(
            "invalid_collection",
            format!("{key} is required"),
        )),
    }
}

fn check_text(value: &str, min: usize, max: usize, field: &str) -> Result<()> {
    let count = value.chars().count();
    if count < min || count > max || value.chars().any(|character| character.is_control()) {
        return Err(Error::new(
            "invalid_collection",
            format!("{field} must be {min} to {max} characters without control characters"),
        ));
    }
    Ok(())
}

fn check_label(label: &str) -> Result<()> {
    if label.is_empty()
        || label.len() > MAX_LABEL
        || !label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
    {
        return Err(Error::new(
            "invalid_collection",
            "replica must be a volume label of letters, digits, '-' or '_'",
        ));
    }
    Ok(())
}

fn parse_entry(value: &toml::Value) -> Result<CollectionEntry> {
    let Some(table) = value.as_table() else {
        return Err(Error::new("invalid_collection", "an entry must be a table"));
    };
    reject_unknown(
        table.keys(),
        |key| {
            matches!(
                key,
                "repo_id" | "revision" | "note" | "representation" | "base_repo" | "base_revision"
            )
        },
        "unknown entry field",
    )?;
    let repo_id = match table.get("repo_id") {
        Some(toml::Value::String(value)) => crate::validation::normalize_repo(value)?,
        Some(_) => {
            return Err(Error::new("invalid_collection", "repo_id must be a string"));
        }
        None => {
            return Err(Error::new("invalid_collection", "repo_id is required"));
        }
    };
    let revision = match table.get("revision") {
        Some(toml::Value::String(value)) => {
            crate::validation::validate_revision(value)?;
            value.clone()
        }
        Some(_) => {
            return Err(Error::new(
                "invalid_collection",
                "revision must be a string",
            ));
        }
        None => {
            return Err(Error::new("invalid_collection", "revision is required"));
        }
    };
    let note = match table.get("note") {
        None => String::new(),
        Some(toml::Value::String(value)) => value.clone(),
        Some(_) => {
            return Err(Error::new("invalid_collection", "note must be a string"));
        }
    };
    if !note.is_empty() {
        check_text(&note, 1, MAX_NOTE, "note")?;
    }
    let representation = match table.get("representation") {
        None => None,
        Some(toml::Value::String(value)) if REPRESENTATIONS.contains(&value.as_str()) => {
            Some(value.clone())
        }
        Some(toml::Value::String(_)) => {
            return Err(Error::new(
                "invalid_collection",
                "representation must be original-required, original-preferred, or selected-derivative",
            ));
        }
        Some(_) => {
            return Err(Error::new(
                "invalid_collection",
                "representation must be a string",
            ));
        }
    };
    let base_repo = match table.get("base_repo") {
        None => None,
        Some(toml::Value::String(value)) => Some(crate::validation::normalize_repo(value)?),
        Some(_) => {
            return Err(Error::new(
                "invalid_collection",
                "base_repo must be a string",
            ));
        }
    };
    let base_revision = match table.get("base_revision") {
        None => None,
        Some(toml::Value::String(value)) => {
            crate::validation::validate_revision(value)?;
            Some(value.clone())
        }
        Some(_) => {
            return Err(Error::new(
                "invalid_collection",
                "base_revision must be a string",
            ));
        }
    };
    if base_revision.is_some() && base_repo.is_none() {
        return Err(Error::new(
            "invalid_collection",
            "base_revision requires base_repo",
        ));
    }
    if base_repo.as_deref() == Some(repo_id.as_str())
        && base_revision.as_deref() == Some(revision.as_str())
    {
        return Err(Error::new(
            "invalid_collection",
            "an entry cannot name itself as its base",
        ));
    }
    Ok(CollectionEntry {
        repo_id,
        revision,
        note,
        representation,
        base_repo,
        base_revision,
        base_preserved: false,
        resolved: false,
    })
}

fn shared_bases(entries: &[CollectionEntry]) -> Vec<SharedBase> {
    let mut groups: Vec<SharedBase> = Vec::new();
    for entry in entries {
        let Some(repo) = entry.base_repo.as_deref() else {
            continue;
        };
        let Some(revision) = entry.base_revision.as_deref() else {
            continue;
        };
        if let Some(group) = groups
            .iter_mut()
            .find(|group| group.base_repo == repo && group.base_revision == revision)
        {
            group.entry_count += 1;
        } else {
            groups.push(SharedBase {
                base_repo: repo.to_owned(),
                base_revision: revision.to_owned(),
                entry_count: 1,
                deduplicated: false,
            });
        }
    }
    groups.retain(|group| group.entry_count >= 2);
    groups
}

fn parse_ceilings(table: &toml::Table) -> Result<Vec<CategoryCeiling>> {
    let Some(value) = table.get("ceilings") else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_array() else {
        return Err(Error::new(
            "invalid_collection",
            "ceilings must be an array",
        ));
    };
    if items.len() > CATEGORIES.len() {
        return Err(Error::new(
            "invalid_collection",
            format!("a collection lists at most {} ceilings", CATEGORIES.len()),
        ));
    }
    let mut ceilings = Vec::with_capacity(items.len());
    let mut seen = BTreeSet::new();
    for item in items {
        let ceiling = parse_ceiling(item)?;
        if !seen.insert(ceiling.category.clone()) {
            return Err(Error::new(
                "invalid_collection",
                format!("duplicate ceiling {}", ceiling.category),
            ));
        }
        ceilings.push(ceiling);
    }
    Ok(ceilings)
}

fn parse_ceiling(value: &toml::Value) -> Result<CategoryCeiling> {
    let Some(table) = value.as_table() else {
        return Err(Error::new(
            "invalid_collection",
            "a ceiling must be a table",
        ));
    };
    reject_unknown(
        table.keys(),
        |key| matches!(key, "category" | "bytes"),
        "unknown ceiling field",
    )?;
    let category = match table.get("category") {
        Some(toml::Value::String(value)) if CATEGORIES.contains(&value.as_str()) => value.clone(),
        Some(toml::Value::String(_)) => {
            return Err(Error::new(
                "invalid_collection",
                "category must be text, coding, vision, image_generation, video_generation, speech_to_text, text_to_speech, embeddings, or reference",
            ));
        }
        Some(_) => {
            return Err(Error::new(
                "invalid_collection",
                "category must be a string",
            ));
        }
        None => {
            return Err(Error::new("invalid_collection", "category is required"));
        }
    };
    let bytes = match table.get("bytes") {
        Some(toml::Value::Integer(value)) if *value >= 0 => *value as u64,
        Some(toml::Value::Integer(_)) => {
            return Err(Error::new(
                "invalid_collection",
                "bytes must be zero or greater",
            ));
        }
        Some(_) => {
            return Err(Error::new("invalid_collection", "bytes must be an integer"));
        }
        None => return Err(Error::new("invalid_collection", "bytes is required")),
    };
    Ok(CategoryCeiling {
        category,
        bytes,
        applied: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn listed(body: &str) -> String {
        format!("schema_version = 1\nname = \"offline coding\"\nentries = []\n{body}")
    }

    fn one(repo: &str, revision: &str, note: &str) -> String {
        format!(
            "schema_version = 1\nname = \"offline coding\"\nreplica = \"backup\"\n[[entries]]\nrepo_id = \"{repo}\"\nrevision = \"{revision}\"\nnote = \"{note}\"\n"
        )
    }

    fn code(text: &str) -> String {
        preview(text).unwrap_err().code.to_string()
    }

    #[test]
    fn preview_lists_a_pinned_entry_and_does_not_resolve_it() {
        let report = preview(&one(
            "https://huggingface.co/example/model/",
            "0123456789abcdef0123456789abcdef01234567",
            "pinned",
        ))
        .unwrap();
        assert!(report.supported);
        assert_eq!(report.action, "preview");
        assert_eq!(report.name, "offline coding");
        assert_eq!(report.replica.as_deref(), Some("backup"));
        assert_eq!(report.entry_count, 1);
        assert_eq!(report.entries[0].repo_id, "example/model");
        assert!(report.entries[0].representation.is_none());
        assert!(report.entries[0].base_repo.is_none());
        assert!(report.entries[0].base_revision.is_none());
        assert!(!report.entries[0].base_preserved);
        assert!(report.ceilings.is_empty());
        assert!(!report.entries[0].resolved);
        assert!(!report.transfer_authorized);
        assert!(!report.network_contact);
        assert!(!report.vault_mutated);
        assert!(!report.downloads_started);
        assert!(report.workspace_bytes.is_none());
        let same_repo = one(
            "example/other",
            "0123456789abcdef0123456789abcdef01234567",
            "",
        );
        let report = preview(&same_repo).unwrap();
        assert!(report.entries[0].note.is_empty());
        assert_eq!(report.entries.len(), report.entry_count);
        let text = crate::present::render_collection(&report, false, 120);
        assert!(text.contains("unspecified"), "{text}");
        assert!(text.contains("unresolved"), "{text}");
        assert!(text.contains("base not declared"), "{text}");
        assert!(!text.contains("pinned"), "{text}");
        let chosen = format!(
            "{}\nrepresentation = \"original-required\"\n",
            one(
                "example/model",
                "0123456789abcdef0123456789abcdef01234567",
                "pinned",
            )
        );
        for value in [
            "original-required",
            "original-preferred",
            "selected-derivative",
        ] {
            let report = preview(&chosen.replace("original-required", value)).unwrap();
            assert_eq!(report.entries[0].representation.as_deref(), Some(value));
            assert!(!report.downloads_started);
            assert!(!report.entries[0].resolved);
        }
        assert_eq!(
            code(&chosen.replace("original-required", "original")),
            "invalid_collection"
        );
        assert_eq!(
            code(&chosen.replacen(
                "representation = \"original-required\"",
                "representation = 1",
                1
            )),
            "invalid_collection"
        );
    }

    #[test]
    fn the_example_file_previews_without_a_download() {
        let report = preview(include_str!("../examples/collection.toml")).unwrap();
        assert!(report.supported);
        assert_eq!(report.entries.len(), 2);
        assert_eq!(
            report.entries[0].representation.as_deref(),
            Some("original-required")
        );
        assert!(report.entries[0].base_repo.is_none());
        assert!(!report.entries[0].base_preserved);
        assert_eq!(
            report.entries[1].base_repo.as_deref(),
            Some("example/model")
        );
        assert_eq!(
            report.entries[1].base_revision.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        assert!(!report.entries[1].base_preserved);
        assert!(!report.entries[1].resolved);
        assert!(report.shared_bases.is_empty());
        assert_eq!(report.ceilings.len(), 1);
        assert_eq!(report.ceilings[0].category, "coding");
        assert_eq!(report.ceilings[0].bytes, 500_000_000_000);
        assert!(!report.ceilings[0].applied);
        assert!(!report.downloads_started);
        assert!(report.workspace_bytes.is_none());
    }

    #[test]
    fn an_unknown_schema_is_reported_and_not_parsed() {
        for text in [
            "",
            "schema_version = 2\nname = 1\n",
            "schema_version = \"1\"\n",
        ] {
            let report = preview(text).unwrap();
            assert!(!report.supported, "{text}");
            assert!(report.entries.is_empty());
            assert!(report.ceilings.is_empty());
            assert!(report.shared_bases.is_empty());
            assert!(!report.transfer_authorized);
            assert!(report.workspace_bytes.is_none());
        }
    }

    #[test]
    fn a_malformed_collection_names_the_field() {
        assert_eq!(code("["), "invalid_collection");
        assert!(!preview("[1]\n").unwrap().supported);
        assert_eq!(
            code("schema_version = 1\nalpha = 1\nzebra = 1\n"),
            "invalid_collection"
        );
        assert!(
            preview("schema_version = 1\nalpha = 1\nzebra = 1\n")
                .unwrap_err()
                .message
                .contains("alpha")
        );
        assert_eq!(code("schema_version = 1\n"), "invalid_collection");
        assert_eq!(
            code("schema_version = 1\nname = 1\nentries = []\n"),
            "invalid_collection"
        );
        assert_eq!(
            code("schema_version = 1\nname = \"\"\nentries = []\n"),
            "invalid_collection"
        );
        assert_eq!(
            code(&format!(
                "schema_version = 1\nname = \"{}\"\nentries = []\n",
                "n".repeat(81)
            )),
            "invalid_collection"
        );
        assert_eq!(
            code("schema_version = 1\nname = \"bad\\nname\"\nentries = []\n"),
            "invalid_collection"
        );
        assert_eq!(code(&listed("replica = 1")), "invalid_collection");
        assert_eq!(code(&listed("replica = \"\"")), "invalid_collection");
        assert_eq!(
            code(&listed(&format!("replica = \"{}\"", "r".repeat(65)))),
            "invalid_collection"
        );
        assert_eq!(
            code(&listed("replica = \"has space\"")),
            "invalid_collection"
        );
        assert_eq!(
            code("schema_version = 1\nname = \"ok\"\n"),
            "invalid_collection"
        );
        assert_eq!(
            code("schema_version = 1\nname = \"ok\"\nentries = \"no\"\n"),
            "invalid_collection"
        );
        assert_eq!(code(&entries(101)), "invalid_collection");
        assert_eq!(
            preview("schema_version = 1\nname = \"ok\"\nentries = []\n")
                .unwrap()
                .entry_count,
            0
        );
        assert_eq!(preview(&entries(100)).unwrap().entry_count, 100);
        assert_eq!(
            code("schema_version = 1\nname = \"ok\"\nentries = [1]\n"),
            "invalid_collection"
        );
        assert_eq!(
            code(&entry("repo_id = 1\nrevision = \"main\"\n")),
            "invalid_collection"
        );
        assert_eq!(code(&entry("revision = \"main\"\n")), "invalid_collection");
        assert_eq!(
            code(&entry("repo_id = \"not a repo\"\nrevision = \"main\"\n")),
            "invalid_repo"
        );
        assert_eq!(
            code(&entry("repo_id = \"example/model\"\nrevision = 1\n")),
            "invalid_collection"
        );
        assert_eq!(
            code(&entry("repo_id = \"example/model\"\n")),
            "invalid_collection"
        );
        assert_eq!(
            code(&entry("repo_id = \"example/model\"\nrevision = \"../x\"\n")),
            "invalid_revision"
        );
        assert_eq!(
            code(&entry(
                "repo_id = \"example/model\"\nrevision = \"main\"\nnote = 1\n"
            )),
            "invalid_collection"
        );
        assert_eq!(
            code(&entry(&format!(
                "repo_id = \"example/model\"\nrevision = \"main\"\nnote = \"{}\"\n",
                "n".repeat(201)
            ))),
            "invalid_collection"
        );
        assert_eq!(
            code(&entry(
                "repo_id = \"example/model\"\nrevision = \"main\"\nnote = \"bad\\nnote\"\n"
            )),
            "invalid_collection"
        );
        assert_eq!(
            code(&entry(
                "repo_id = \"example/model\"\nrevision = \"main\"\nextra = 1\n"
            )),
            "invalid_collection"
        );
        let duplicate = "\
schema_version = 1
name = \"ok\"
[[entries]]
repo_id = \"example/model\"
revision = \"main\"
[[entries]]
repo_id = \"example/model\"
revision = \"main\"
";
        let error = preview(duplicate).unwrap_err();
        assert_eq!(error.code, "invalid_collection");
        assert!(error.message.contains("duplicate entry example/model@main"));
        assert_eq!(code(&listed("ceilings = 1\n")), "invalid_collection");
        assert_eq!(code(&listed("ceilings = [1]\n")), "invalid_collection");
        assert_eq!(
            code(&listed("[[ceilings]]\nbytes = 1\n")),
            "invalid_collection"
        );
        assert_eq!(
            code(&listed("[[ceilings]]\ncategory = 1\nbytes = 1\n")),
            "invalid_collection"
        );
        assert_eq!(
            code(&listed(
                "[[ceilings]]\ncategory = \"reasoning\"\nbytes = 1\n"
            )),
            "invalid_collection"
        );
        assert_eq!(
            code(&listed("[[ceilings]]\ncategory = \"coding\"\n")),
            "invalid_collection"
        );
        assert_eq!(
            code(&listed("[[ceilings]]\ncategory = \"coding\"\nbytes = -1\n")),
            "invalid_collection"
        );
        assert_eq!(
            code(&listed(
                "[[ceilings]]\ncategory = \"coding\"\nbytes = 1.0\n"
            )),
            "invalid_collection"
        );
        assert_eq!(
            code(&listed(
                "[[ceilings]]\ncategory = \"coding\"\nbytes = 1\nextra = 1\n"
            )),
            "invalid_collection"
        );
        let duplicate_ceiling = listed(
            "[[ceilings]]\ncategory = \"coding\"\nbytes = 1\n[[ceilings]]\ncategory = \"coding\"\nbytes = 2\n",
        );
        let error = preview(&duplicate_ceiling).unwrap_err();
        assert_eq!(error.code, "invalid_collection");
        assert!(error.message.contains("duplicate ceiling coding"));
        let mut too_many = String::from("schema_version = 1\nname = \"ok\"\nentries = []\n");
        for _ in 0..10 {
            too_many.push_str("[[ceilings]]\ncategory = \"coding\"\nbytes = 1\n");
        }
        assert!(
            preview(&too_many)
                .unwrap_err()
                .message
                .contains("at most 9")
        );
        assert_eq!(
            code(&entry(
                "repo_id = \"example/model\"\nrevision = \"main\"\nbase_repo = 1\n"
            )),
            "invalid_collection"
        );
        assert_eq!(
            code(&entry(
                "repo_id = \"example/model\"\nrevision = \"main\"\nbase_repo = \"not a repo\"\n"
            )),
            "invalid_repo"
        );
        assert_eq!(
            code(&entry(
                "repo_id = \"example/model\"\nrevision = \"main\"\nbase_revision = 1\n"
            )),
            "invalid_collection"
        );
        assert_eq!(
            code(&entry(
                "repo_id = \"example/model\"\nrevision = \"main\"\nbase_revision = \"../x\"\n"
            )),
            "invalid_revision"
        );
        let missing_repo = preview(&entry(
            "repo_id = \"example/model\"\nrevision = \"main\"\nbase_revision = \"v1\"\n",
        ))
        .unwrap_err();
        assert!(
            missing_repo
                .message
                .contains("base_revision requires base_repo")
        );
        let itself = preview(&entry(
            "repo_id = \"example/model\"\nrevision = \"main\"\nbase_repo = \"https://huggingface.co/example/model/\"\nbase_revision = \"main\"\n",
        ))
        .unwrap_err();
        assert!(itself.message.contains("cannot name itself"));
    }

    #[test]
    fn a_declared_base_is_recorded_and_not_preserved() {
        let named = preview(&entry(
            "repo_id = \"example/model\"\nrevision = \"main\"\nbase_repo = \"https://huggingface.co/example/base/\"\nbase_revision = \"v1\"\n",
        ))
        .unwrap();
        assert_eq!(named.entries[0].base_repo.as_deref(), Some("example/base"));
        assert_eq!(named.entries[0].base_revision.as_deref(), Some("v1"));
        assert!(!named.entries[0].base_preserved);
        assert!(!named.entries[0].resolved);
        assert!(!named.downloads_started);
        assert!(named.workspace_bytes.is_none());
        let text = crate::present::render_collection(&named, false, 120);
        assert!(
            text.contains("base example/base@v1 not preserved"),
            "{text}"
        );
        let open = preview(&entry(
            "repo_id = \"example/model\"\nrevision = \"main\"\nbase_repo = \"example/model\"\n",
        ))
        .unwrap();
        assert_eq!(open.entries[0].base_repo.as_deref(), Some("example/model"));
        assert!(open.entries[0].base_revision.is_none());
        assert!(!open.entries[0].base_preserved);
        let open_text = crate::present::render_collection(&open, false, 120);
        assert!(
            open_text.contains("base example/model revision not established"),
            "{open_text}"
        );
        let earlier = preview(&entry(
            "repo_id = \"example/model\"\nrevision = \"main\"\nbase_repo = \"example/model\"\nbase_revision = \"v1\"\n",
        ))
        .unwrap();
        assert_eq!(earlier.entries[0].base_revision.as_deref(), Some("v1"));
        assert!(!earlier.entries[0].base_preserved);
        assert!(named.shared_bases.is_empty());
    }

    #[test]
    fn a_repeated_base_is_listed_and_not_deduplicated() {
        let one = preview(&format!(
            "{}\n{}",
            entry("repo_id = \"example/model\"\nrevision = \"main\"\nbase_repo = \"example/base\"\nbase_revision = \"v1\"\n"),
            "[[entries]]\nrepo_id = \"example/other\"\nrevision = \"main\"\nbase_repo = \"example/base\"\nbase_revision = \"v1\"\n"
        ))
        .unwrap();
        assert_eq!(one.entries.len(), 2);
        assert_eq!(one.shared_bases.len(), 1);
        assert_eq!(one.shared_bases[0].base_repo, "example/base");
        assert_eq!(one.shared_bases[0].base_revision, "v1");
        assert_eq!(one.shared_bases[0].entry_count, 2);
        assert!(!one.shared_bases[0].deduplicated);
        assert!(!one.downloads_started);
        assert!(one.workspace_bytes.is_none());
        let text = crate::present::render_collection(&one, false, 120);
        assert!(
            text.contains("example/base@v1  2 entries  not deduplicated"),
            "{text}"
        );
        let split = preview(&format!(
            "{}\n[[entries]]\nrepo_id = \"example/other\"\nrevision = \"main\"\nbase_repo = \"example/base\"\nbase_revision = \"v2\"\n",
            entry("repo_id = \"example/model\"\nrevision = \"main\"\nbase_repo = \"example/base\"\nbase_revision = \"v1\"\n")
        ))
        .unwrap();
        assert!(split.shared_bases.is_empty());
        assert_eq!(split.entries.len(), 2);
        let open = preview(&format!(
            "{}\n[[entries]]\nrepo_id = \"example/other\"\nrevision = \"main\"\nbase_repo = \"example/base\"\n",
            entry("repo_id = \"example/model\"\nrevision = \"main\"\nbase_repo = \"example/base\"\n")
        ))
        .unwrap();
        assert!(open.shared_bases.is_empty());
        let ordered = preview(&format!(
            "{}\n[[entries]]\nrepo_id = \"example/other\"\nrevision = \"main\"\nbase_repo = \"example/base\"\nbase_revision = \"v1\"\n[[entries]]\nrepo_id = \"example/third\"\nrevision = \"main\"\nbase_repo = \"example/later\"\nbase_revision = \"v9\"\n[[entries]]\nrepo_id = \"example/fourth\"\nrevision = \"main\"\nbase_repo = \"example/later\"\nbase_revision = \"v9\"\n",
            entry("repo_id = \"example/model\"\nrevision = \"main\"\nbase_repo = \"example/base\"\nbase_revision = \"v1\"\n")
        ))
        .unwrap();
        assert_eq!(ordered.entries.len(), 4);
        assert_eq!(ordered.shared_bases.len(), 2);
        assert_eq!(ordered.shared_bases[0].base_repo, "example/base");
        assert_eq!(ordered.shared_bases[1].base_repo, "example/later");
        assert!(!ordered.shared_bases[1].deduplicated);
        let alone = preview(&entry(
            "repo_id = \"example/model\"\nrevision = \"main\"\nbase_repo = \"example/base\"\nbase_revision = \"v1\"\n",
        ))
        .unwrap();
        assert!(alone.shared_bases.is_empty());
    }

    #[test]
    fn a_ceiling_is_recorded_and_not_applied() {
        let absent = preview(&listed("")).unwrap();
        assert!(absent.ceilings.is_empty());
        let absent_text = crate::present::render_collection(&absent, false, 80);
        assert!(absent_text.contains("ceilings  0"), "{absent_text}");
        let zero = preview(&listed("[[ceilings]]\ncategory = \"text\"\nbytes = 0\n")).unwrap();
        assert_eq!(zero.ceilings[0].bytes, 0);
        assert!(!zero.ceilings[0].applied);
        let mut body = String::new();
        for category in CATEGORIES {
            body.push_str(&format!(
                "[[ceilings]]\ncategory = \"{category}\"\nbytes = 8\n"
            ));
        }
        let report = preview(&listed(&body)).unwrap();
        assert_eq!(report.ceilings.len(), CATEGORIES.len());
        for (ceiling, category) in report.ceilings.iter().zip(CATEGORIES) {
            assert_eq!(ceiling.category, *category);
            assert_eq!(ceiling.bytes, 8);
            assert!(!ceiling.applied);
        }
        assert!(!report.transfer_authorized);
        assert!(report.workspace_bytes.is_none());
        assert!(!report.downloads_started);
        let text = crate::present::render_collection(&report, false, 80);
        assert!(text.contains("coding  8 bytes  not applied"), "{text}");
        assert!(text.contains("not applied"), "{text}");
    }

    fn entries(count: usize) -> String {
        let mut text = String::from("schema_version = 1\nname = \"ok\"\n");
        for index in 0..count {
            text.push_str(&format!(
                "[[entries]]\nrepo_id = \"example/model{index}\"\nrevision = \"main\"\n"
            ));
        }
        text
    }

    fn entry(body: &str) -> String {
        format!("schema_version = 1\nname = \"ok\"\n[[entries]]\n{body}")
    }

    #[test]
    fn the_command_previews_the_file_and_leaves_it_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("collection.toml");
        let body = include_str!("../examples/collection.toml");
        std::fs::write(&path, body).unwrap();
        let command = crate::cli::Cli::try_parse_from([
            "modelprepper",
            "collection",
            "preview",
            "--file",
            path.to_str().unwrap(),
        ])
        .unwrap();
        let output = crate::cli::execute_in(command, crate::cli::View::piped()).unwrap();
        let crate::cli::CommandOutput::Json(value) = output else {
            panic!("preview is json when stdout is a pipe");
        };
        assert_eq!(value["action"], "preview");
        assert_eq!(value["transfer_authorized"], false);
        assert_eq!(value["network_contact"], false);
        assert_eq!(value["vault_mutated"], false);
        assert_eq!(value["downloads_started"], false);
        assert!(value["workspace_bytes"].is_null());
        assert_eq!(value["entries"][0]["resolved"], false);
        assert_eq!(value["entries"][0]["representation"], "original-required");
        assert!(value["entries"][0]["base_repo"].is_null());
        assert!(value["entries"][0]["base_revision"].is_null());
        assert_eq!(value["entries"][0]["base_preserved"], false);
        assert_eq!(value["entries"][1]["base_repo"], "example/model");
        assert_eq!(
            value["entries"][1]["base_revision"],
            "0123456789abcdef0123456789abcdef01234567"
        );
        assert_eq!(value["entries"][1]["base_preserved"], false);
        assert_eq!(value["entries"][1]["resolved"], false);
        assert!(value["shared_bases"].as_array().unwrap().is_empty());
        assert_eq!(value["ceilings"][0]["category"], "coding");
        assert_eq!(value["ceilings"][0]["bytes"], 500_000_000_000_u64);
        assert_eq!(value["ceilings"][0]["applied"], false);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), body);

        let text = crate::cli::Cli::try_parse_from([
            "modelprepper",
            "collection",
            "preview",
            "--file",
            path.to_str().unwrap(),
        ])
        .unwrap();
        let output = crate::cli::execute_in(
            text,
            crate::cli::View {
                text: true,
                ..crate::cli::View::piped()
            },
        )
        .unwrap();
        let crate::cli::CommandOutput::Text { body, ok } = output else {
            panic!("text preview");
        };
        assert!(ok);
        assert!(body.contains("preview"), "{body}");
        assert!(body.contains("still blocked"), "{body}");
        assert!(
            body.lines().all(|line| line.chars().count() <= 80),
            "{body}"
        );

        let bad = dir.path().join("bad.toml");
        std::fs::write(&bad, "schema_version = 1\nname = 1\n").unwrap();
        let refused = crate::cli::Cli::try_parse_from([
            "modelprepper",
            "collection",
            "preview",
            "--file",
            bad.to_str().unwrap(),
        ])
        .unwrap();
        let error = crate::cli::execute_in(refused, crate::cli::View::piped()).unwrap_err();
        assert_eq!(error.code, "invalid_collection");
        let rendered = crate::present::render_collection(
            &preview(include_str!("../examples/collection.toml")).unwrap(),
            true,
            80,
        );
        assert!(rendered.contains("offline coding"), "{rendered}");
        let unsupported = crate::present::render_collection(&preview("").unwrap(), false, 40);
        assert!(unsupported.contains("unsupported"), "{unsupported}");
        assert!(
            unsupported.lines().all(|line| line.chars().count() <= 40),
            "{unsupported}"
        );
    }
}
