//! Shared collection file. Preview lists explicit entries. Store writes that list and does not download it.
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
    /// Store replaces one named list. Preview and show leave this false.
    pub replaced: bool,
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
    /// Kit slot names the operator wrote. Empty means the file did not assign any.
    pub categories: Vec<String>,
    /// Declared base repository. Absent means the file did not name one.
    pub base_repo: Option<String>,
    /// Declared base revision. Absent means the file did not establish one.
    pub base_revision: Option<String>,
    /// Preview does not fetch or verify the declared base.
    pub base_preserved: bool,
    /// The operator's written attribution. Absent means the file did not give one.
    pub evaluation: Option<EvaluationAttribution>,
    /// The operator's statement. Absent means the file did not state it.
    pub evidence: Option<String>,
    /// Preview does not open files to check this statement.
    pub evidence_checked: bool,
    /// Conversion or training statement. Absent means the file did not give one.
    pub provenance: Option<String>,
    /// Preview does not check this statement.
    pub provenance_verified: bool,
    /// Preview does not contact a publisher, so this stays false.
    pub resolved: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct EvaluationAttribution {
    pub source: String,
    pub name: String,
    pub value: String,
    /// Preview does not order entries by this text.
    pub ranked: bool,
}

const EVIDENCE: &[&str] = &["present", "missing"];

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
        replaced: false,
        downloads_started: false,
        workspace_bytes: None,
    })
}

/// Write one parsed list into the catalog. An unknown schema is not stored.
pub fn store(config: &crate::Config, text: &str) -> Result<CollectionReport> {
    let mut report = preview(text)?;
    report.action = "store";
    if !report.supported {
        return Ok(report);
    }
    let replaced = crate::vault::write_collection(config, &report)?;
    report.action = "stored";
    report.vault_mutated = true;
    report.replaced = replaced;
    Ok(report)
}

/// Read one stored list. The catalog is not rewritten.
pub fn show(config: &crate::Config, name: &str) -> Result<CollectionReport> {
    check_text(name, 1, MAX_NAME, "name")?;
    crate::vault::read_collection(config, name)
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct CollectionList {
    pub schema_version: u32,
    pub action: &'static str,
    pub collections: Vec<CollectionSummary>,
    pub next_after: Option<String>,
    /// Name order is not an evaluation rank.
    pub ranked: bool,
    pub transfer_authorized: bool,
    pub network_contact: bool,
    pub vault_mutated: bool,
    pub downloads_started: bool,
    pub workspace_bytes: Option<u64>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct CollectionSummary {
    pub name: String,
    pub replica: Option<String>,
    pub entry_count: u64,
}

/// Page stored names in name order. This does not rank or download them.
pub fn list(config: &crate::Config, after: Option<&str>, limit: u32) -> Result<CollectionList> {
    if !(1..=100).contains(&limit) {
        return Err(Error::new(
            "invalid_collection",
            "collection page limit must be 1 through 100",
        ));
    }
    if let Some(after) = after {
        check_text(after, 1, MAX_NAME, "name")?;
    }
    let rows = crate::vault::list_collections(config, after.unwrap_or(""), limit + 1)?;
    let next_after = if rows.len() > limit as usize {
        Some(rows[limit as usize - 1].name.clone())
    } else {
        None
    };
    Ok(CollectionList {
        schema_version: 1,
        action: "list",
        collections: rows.into_iter().take(limit as usize).collect(),
        next_after,
        ranked: false,
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
        replaced: false,
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
                "repo_id"
                    | "revision"
                    | "note"
                    | "representation"
                    | "categories"
                    | "base_repo"
                    | "base_revision"
                    | "evaluation_source"
                    | "evaluation_name"
                    | "evaluation_value"
                    | "evidence"
                    | "provenance"
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
    let categories = parse_categories(table)?;
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
    let evaluation = parse_evaluation(table)?;
    let evidence = match table.get("evidence") {
        None => None,
        Some(toml::Value::String(value)) if EVIDENCE.contains(&value.as_str()) => {
            Some(value.clone())
        }
        Some(toml::Value::String(_)) => {
            return Err(Error::new(
                "invalid_collection",
                "evidence must be present or missing",
            ));
        }
        Some(_) => {
            return Err(Error::new(
                "invalid_collection",
                "evidence must be a string",
            ));
        }
    };
    let provenance = match table.get("provenance") {
        None => None,
        Some(toml::Value::String(value)) => {
            check_text(value, 1, MAX_NOTE, "provenance")?;
            Some(value.clone())
        }
        Some(_) => {
            return Err(Error::new(
                "invalid_collection",
                "provenance must be a string",
            ));
        }
    };
    Ok(CollectionEntry {
        repo_id,
        revision,
        note,
        representation,
        categories,
        base_repo,
        base_revision,
        base_preserved: false,
        evaluation,
        evidence,
        evidence_checked: false,
        provenance,
        provenance_verified: false,
        resolved: false,
    })
}

fn parse_evaluation(table: &toml::Table) -> Result<Option<EvaluationAttribution>> {
    let source = optional_text(table, "evaluation_source")?;
    let name = optional_text(table, "evaluation_name")?;
    let value = optional_text(table, "evaluation_value")?;
    match (source, name, value) {
        (None, None, None) => Ok(None),
        (Some(source), Some(name), Some(value)) => Ok(Some(EvaluationAttribution {
            source,
            name,
            value,
            ranked: false,
        })),
        _ => Err(Error::new(
            "invalid_collection",
            "evaluation requires source, name, and value",
        )),
    }
}

fn optional_text(table: &toml::Table, key: &str) -> Result<Option<String>> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::String(value)) => {
            check_text(value, 1, MAX_NAME, key)?;
            Ok(Some(value.clone()))
        }
        Some(_) => Err(Error::new(
            "invalid_collection",
            format!("{key} must be a string"),
        )),
    }
}

fn parse_categories(table: &toml::Table) -> Result<Vec<String>> {
    let Some(value) = table.get("categories") else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_array() else {
        return Err(Error::new(
            "invalid_collection",
            "categories must be an array",
        ));
    };
    if items.len() > CATEGORIES.len() {
        return Err(Error::new(
            "invalid_collection",
            format!("an entry lists at most {} categories", CATEGORIES.len()),
        ));
    }
    let mut categories = Vec::with_capacity(items.len());
    let mut seen = BTreeSet::new();
    for item in items {
        let category = match item {
            toml::Value::String(value) if CATEGORIES.contains(&value.as_str()) => value.clone(),
            toml::Value::String(_) => {
                return Err(Error::new(
                    "invalid_collection",
                    "category must be text, coding, vision, image_generation, video_generation, speech_to_text, text_to_speech, embeddings, or reference",
                ));
            }
            _ => {
                return Err(Error::new(
                    "invalid_collection",
                    "categories must contain strings",
                ));
            }
        };
        if !seen.insert(category.clone()) {
            return Err(Error::new(
                "invalid_collection",
                format!("duplicate category {category}"),
            ));
        }
        categories.push(category);
    }
    Ok(categories)
}

pub(crate) fn shared_bases(entries: &[CollectionEntry]) -> Vec<SharedBase> {
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
        let text = crate::present::render_collection(&report, false, 220);
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
        assert!(report.entries[0].categories.is_empty());
        assert!(report.entries[0].evaluation.is_none());
        assert!(report.entries[0].evidence.is_none());
        assert!(!report.entries[0].evidence_checked);
        assert!(report.entries[0].provenance.is_none());
        assert!(!report.entries[0].provenance_verified);
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
        assert_eq!(
            report.entries[1].categories,
            ["coding".to_string(), "text".to_string()]
        );
        let evaluation = report.entries[1].evaluation.as_ref().unwrap();
        assert_eq!(evaluation.source, "operator");
        assert_eq!(evaluation.name, "manual-review");
        assert_eq!(evaluation.value, "not scored");
        assert!(!evaluation.ranked);
        assert_eq!(report.entries[1].evidence.as_deref(), Some("missing"));
        assert!(!report.entries[1].evidence_checked);
        assert_eq!(
            report.entries[1].provenance.as_deref(),
            Some("stated conversion")
        );
        assert!(!report.entries[1].provenance_verified);
        assert!(!report.ceilings[0].applied);
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
        assert_eq!(
            code(&entry(
                "repo_id = \"example/model\"\nrevision = \"main\"\ncategories = 1\n"
            )),
            "invalid_collection"
        );
        assert_eq!(
            code(&entry(
                "repo_id = \"example/model\"\nrevision = \"main\"\ncategories = [1]\n"
            )),
            "invalid_collection"
        );
        assert_eq!(
            code(&entry(
                "repo_id = \"example/model\"\nrevision = \"main\"\ncategories = [\"reasoning\"]\n"
            )),
            "invalid_collection"
        );
        let duplicate_category = preview(&entry(
            "repo_id = \"example/model\"\nrevision = \"main\"\ncategories = [\"coding\", \"coding\"]\n",
        ))
        .unwrap_err();
        assert!(
            duplicate_category
                .message
                .contains("duplicate category coding")
        );
        let mut too_many_categories =
            String::from("repo_id = \"example/model\"\nrevision = \"main\"\ncategories = [");
        for _ in 0..10 {
            too_many_categories.push_str("\"coding\",");
        }
        too_many_categories.push_str("]\n");
        assert!(
            preview(&entry(&too_many_categories))
                .unwrap_err()
                .message
                .contains("at most 9")
        );
        for partial in [
            "evaluation_source = \"operator\"\n",
            "evaluation_name = \"manual-review\"\n",
            "evaluation_value = \"not scored\"\n",
            "evaluation_source = \"operator\"\nevaluation_name = \"manual-review\"\n",
        ] {
            let error = preview(&entry(&format!(
                "repo_id = \"example/model\"\nrevision = \"main\"\n{partial}"
            )))
            .unwrap_err();
            assert_eq!(error.code, "invalid_collection");
            assert!(
                error
                    .message
                    .contains("evaluation requires source, name, and value")
            );
        }
        assert_eq!(
            code(&entry(
                "repo_id = \"example/model\"\nrevision = \"main\"\nevaluation_source = \"operator\"\nevaluation_name = \"manual-review\"\nevaluation_value = 1\n"
            )),
            "invalid_collection"
        );
        assert_eq!(
            code(&entry(
                "repo_id = \"example/model\"\nrevision = \"main\"\nevaluation_source = \"\"\nevaluation_name = \"manual-review\"\nevaluation_value = \"not scored\"\n"
            )),
            "invalid_collection"
        );
        assert_eq!(
            code(&entry(&format!(
                "repo_id = \"example/model\"\nrevision = \"main\"\nevaluation_source = \"operator\"\nevaluation_name = \"{}\"\nevaluation_value = \"not scored\"\n",
                "n".repeat(81)
            ))),
            "invalid_collection"
        );
        assert_eq!(
            code(&entry(
                "repo_id = \"example/model\"\nrevision = \"main\"\nevaluation_source = \"bad\\nsource\"\nevaluation_name = \"manual-review\"\nevaluation_value = \"not scored\"\n"
            )),
            "invalid_collection"
        );
        assert_eq!(
            code(&entry(
                "repo_id = \"example/model\"\nrevision = \"main\"\nevidence = 1\n"
            )),
            "invalid_collection"
        );
        assert_eq!(
            code(&entry(
                "repo_id = \"example/model\"\nrevision = \"main\"\nevidence = \"unknown\"\n"
            )),
            "invalid_collection"
        );
        assert_eq!(
            code(&entry(
                "repo_id = \"example/model\"\nrevision = \"main\"\nprovenance = 1\n"
            )),
            "invalid_collection"
        );
        assert_eq!(
            code(&entry(
                "repo_id = \"example/model\"\nrevision = \"main\"\nprovenance = \"\"\n"
            )),
            "invalid_collection"
        );
        assert_eq!(
            code(&entry(&format!(
                "repo_id = \"example/model\"\nrevision = \"main\"\nprovenance = \"{}\"\n",
                "p".repeat(201)
            ))),
            "invalid_collection"
        );
        assert_eq!(
            code(&entry(
                "repo_id = \"example/model\"\nrevision = \"main\"\nprovenance = \"bad\\ntext\"\n"
            )),
            "invalid_collection"
        );
    }

    #[test]
    fn provenance_is_recorded_and_not_verified() {
        let report = preview(&entry(
            "repo_id = \"example/model\"\nrevision = \"main\"\nprovenance = \"stated conversion\"\n",
        ))
        .unwrap();
        assert_eq!(
            report.entries[0].provenance.as_deref(),
            Some("stated conversion")
        );
        assert!(!report.entries[0].provenance_verified);
        assert!(!report.downloads_started);
        assert!(report.workspace_bytes.is_none());
        let text = crate::present::render_collection(&report, false, 280);
        assert!(
            text.contains("provenance stated conversion not verified"),
            "{text}"
        );
        let absent = preview(&entry("repo_id = \"example/model\"\nrevision = \"main\"\n")).unwrap();
        assert!(absent.entries[0].provenance.is_none());
        let absent_text = crate::present::render_collection(&absent, false, 280);
        assert!(
            absent_text.contains("provenance not stated"),
            "{absent_text}"
        );
    }

    #[test]
    fn evidence_is_recorded_and_not_checked() {
        for value in ["present", "missing"] {
            let report = preview(&entry(&format!(
                "repo_id = \"example/model\"\nrevision = \"main\"\nevidence = \"{value}\"\n"
            )))
            .unwrap();
            assert_eq!(report.entries[0].evidence.as_deref(), Some(value));
            assert!(!report.entries[0].evidence_checked);
            assert!(!report.downloads_started);
            assert!(report.workspace_bytes.is_none());
            let text = crate::present::render_collection(&report, false, 220);
            assert!(
                text.contains(&format!("evidence {value} not checked")),
                "{text}"
            );
        }
        let absent = preview(&entry("repo_id = \"example/model\"\nrevision = \"main\"\n")).unwrap();
        assert!(absent.entries[0].evidence.is_none());
        let absent_text = crate::present::render_collection(&absent, false, 220);
        assert!(absent_text.contains("evidence not stated"), "{absent_text}");
    }

    #[test]
    fn an_evaluation_is_recorded_and_not_ranked() {
        let report = preview(&entry(
            "repo_id = \"example/model\"\nrevision = \"main\"\nevaluation_source = \"operator\"\nevaluation_name = \"manual-review\"\nevaluation_value = \"not scored\"\n",
        ))
        .unwrap();
        let evaluation = report.entries[0].evaluation.as_ref().unwrap();
        assert_eq!(evaluation.source, "operator");
        assert_eq!(evaluation.name, "manual-review");
        assert_eq!(evaluation.value, "not scored");
        assert!(!evaluation.ranked);
        assert!(!report.entries[0].resolved);
        assert!(!report.downloads_started);
        assert!(report.workspace_bytes.is_none());
        let text = crate::present::render_collection(&report, false, 280);
        assert!(
            text.contains("evaluation operator: manual-review = not scored not ranked"),
            "{text}"
        );
        let absent = preview(&entry("repo_id = \"example/model\"\nrevision = \"main\"\n")).unwrap();
        assert!(absent.entries[0].evaluation.is_none());
        let absent_text = crate::present::render_collection(&absent, false, 280);
        assert!(
            absent_text.contains("evaluation not attributed"),
            "{absent_text}"
        );
    }

    #[test]
    fn categories_are_recorded_and_do_not_charge_a_ceiling() {
        let absent = preview(&entry("repo_id = \"example/model\"\nrevision = \"main\"\n")).unwrap();
        assert!(absent.entries[0].categories.is_empty());
        let absent_text = crate::present::render_collection(&absent, false, 220);
        assert!(absent_text.contains("uncategorized"), "{absent_text}");
        let empty = preview(&entry(
            "repo_id = \"example/model\"\nrevision = \"main\"\ncategories = []\n",
        ))
        .unwrap();
        assert!(empty.entries[0].categories.is_empty());
        let mut names = String::new();
        for (index, category) in CATEGORIES.iter().enumerate() {
            if index > 0 {
                names.push_str(", ");
            }
            names.push('"');
            names.push_str(category);
            names.push('"');
        }
        let listed = preview(&format!(
            "schema_version = 1\nname = \"ok\"\n[[ceilings]]\ncategory = \"coding\"\nbytes = 8\n[[entries]]\nrepo_id = \"example/model\"\nrevision = \"main\"\ncategories = [{names}]\n"
        ))
        .unwrap();
        assert_eq!(listed.entries[0].categories.len(), CATEGORIES.len());
        for (got, expected) in listed.entries[0].categories.iter().zip(CATEGORIES) {
            assert_eq!(got, expected);
        }
        assert!(!listed.ceilings[0].applied);
        assert!(!listed.downloads_started);
        assert!(listed.workspace_bytes.is_none());
        let overlap = preview(&entry(
            "repo_id = \"example/model\"\nrevision = \"main\"\ncategories = [\"coding\", \"text\"]\n",
        ))
        .unwrap();
        assert_eq!(
            overlap.entries[0].categories,
            ["coding".to_string(), "text".to_string()]
        );
        let text = crate::present::render_collection(&overlap, false, 220);
        assert!(text.contains("categories coding,text"), "{text}");
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
        let text = crate::present::render_collection(&named, false, 220);
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
        let open_text = crate::present::render_collection(&open, false, 220);
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
        let text = crate::present::render_collection(&one, false, 220);
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
        assert!(
            value["entries"][0]["categories"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(value["entries"][0]["base_repo"].is_null());
        assert!(value["entries"][0]["base_revision"].is_null());
        assert_eq!(value["entries"][0]["base_preserved"], false);
        assert_eq!(value["entries"][1]["base_repo"], "example/model");
        assert_eq!(
            value["entries"][1]["base_revision"],
            "0123456789abcdef0123456789abcdef01234567"
        );
        assert_eq!(value["entries"][1]["base_preserved"], false);
        assert_eq!(value["entries"][1]["categories"][0], "coding");
        assert_eq!(value["entries"][1]["categories"][1], "text");
        assert!(value["entries"][0]["evaluation"].is_null());
        assert_eq!(value["entries"][1]["evaluation"]["source"], "operator");
        assert_eq!(value["entries"][1]["evaluation"]["name"], "manual-review");
        assert_eq!(value["entries"][1]["evaluation"]["value"], "not scored");
        assert_eq!(value["entries"][1]["evaluation"]["ranked"], false);
        assert!(value["entries"][0]["evidence"].is_null());
        assert_eq!(value["entries"][0]["evidence_checked"], false);
        assert_eq!(value["entries"][1]["evidence"], "missing");
        assert_eq!(value["entries"][1]["evidence_checked"], false);
        assert!(value["entries"][0]["provenance"].is_null());
        assert_eq!(value["entries"][0]["provenance_verified"], false);
        assert_eq!(value["entries"][1]["provenance"], "stated conversion");
        assert_eq!(value["entries"][1]["provenance_verified"], false);
        assert_eq!(value["ceilings"][0]["applied"], false);
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

    fn ready() -> (tempfile::TempDir, crate::Config, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let config_path = root.join("config.toml");
        std::fs::write(&config_path, include_str!("../examples/config.toml")).unwrap();
        let config = crate::vault::locate(
            &crate::Config::parse(&std::fs::read_to_string(&config_path).unwrap()).unwrap(),
            &config_path,
        )
        .unwrap();
        crate::vault::initialize(&config).unwrap();
        (dir, config, config_path)
    }

    fn rows(config: &crate::Config, table: &str) -> i64 {
        let connection =
            rusqlite::Connection::open(config.catalog_path.join("catalog.sqlite3")).unwrap();
        connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    #[test]
    fn store_writes_the_list_and_show_reads_it_without_applying_it() {
        let (_dir, config, config_path) = ready();
        let before = [
            "vault",
            "volumes",
            "proposals",
            "custody",
            "grants",
            "replicas",
            "period_charges",
        ]
        .map(|table| rows(&config, table));
        let body = include_str!("../examples/collection.toml");
        let path = config_path.parent().unwrap().join("collection.toml");
        std::fs::write(&path, body).unwrap();
        let stored = store(&config, body).unwrap();
        assert_eq!(stored.action, "stored");
        assert!(stored.vault_mutated);
        assert!(!stored.replaced);
        assert!(!stored.transfer_authorized);
        assert!(!stored.network_contact);
        assert!(!stored.downloads_started);
        assert!(stored.workspace_bytes.is_none());
        assert!(!stored.entries[0].resolved);
        assert!(!stored.entries[1].provenance_verified);
        assert!(!stored.ceilings[0].applied);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
        for (table, count) in [
            "vault",
            "volumes",
            "proposals",
            "custody",
            "grants",
            "replicas",
            "period_charges",
        ]
        .into_iter()
        .zip(before)
        {
            assert_eq!(rows(&config, table), count, "{table}");
        }

        let shown = show(&config, "offline coding").unwrap();
        assert_eq!(shown.action, "show");
        assert!(!shown.vault_mutated);
        assert!(!shown.replaced);
        assert_eq!(shown.replica.as_deref(), Some("backup"));
        assert_eq!(shown.entries.len(), 2);
        assert!(shown.entries[0].provenance.is_none());
        assert!(!shown.entries[0].provenance_verified);
        assert!(shown.entries[0].evidence.is_none());
        assert!(!shown.entries[0].evidence_checked);
        assert!(shown.entries[0].evaluation.is_none());
        assert!(!shown.entries[0].base_preserved);
        assert_eq!(
            shown.entries[1].provenance.as_deref(),
            Some("stated conversion")
        );
        assert!(!shown.entries[1].provenance_verified);
        assert_eq!(shown.entries[1].evidence.as_deref(), Some("missing"));
        assert!(!shown.entries[1].evidence_checked);
        assert!(!shown.entries[1].evaluation.as_ref().unwrap().ranked);
        assert_eq!(
            shown.entries[1].categories,
            vec!["coding".to_string(), "text".to_string()]
        );
        assert!(!shown.entries[1].base_preserved);
        assert!(!shown.ceilings[0].applied);
        assert!(!shown.downloads_started);
        assert!(shown.workspace_bytes.is_none());
        assert!(shown.shared_bases.is_empty());

        let shared = "\
schema_version = 1
name = \"shared\"
[[entries]]
repo_id = \"example/one\"
revision = \"main\"
base_repo = \"example/base\"
base_revision = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"
[[entries]]
repo_id = \"example/two\"
revision = \"main\"
base_repo = \"example/base\"
base_revision = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"
";
        let second = store(&config, shared).unwrap();
        assert!(!second.replaced);
        let group = show(&config, "shared").unwrap();
        assert_eq!(group.shared_bases.len(), 1);
        assert_eq!(group.shared_bases[0].entry_count, 2);
        assert!(!group.shared_bases[0].deduplicated);
        assert_eq!(show(&config, "offline coding").unwrap().entries.len(), 2);

        let replacement = "schema_version = 1\nname = \"offline coding\"\nentries = []\n";
        let again = store(&config, replacement).unwrap();
        assert!(again.replaced);
        assert!(again.entries.is_empty());
        assert!(show(&config, "offline coding").unwrap().entries.is_empty());
        assert_eq!(show(&config, "shared").unwrap().entries.len(), 2);
        assert_eq!(rows(&config, "custody"), before[3]);

        let previewed = preview(body).unwrap();
        assert_eq!(previewed.action, "preview");
        assert!(!previewed.vault_mutated);
        assert!(!previewed.replaced);

        let command = crate::cli::Cli::try_parse_from([
            "modelprepper",
            "collection",
            "store",
            "--config",
            config_path.to_str().unwrap(),
            "--file",
            path.to_str().unwrap(),
        ])
        .unwrap();
        let output = crate::cli::execute_in(command, crate::cli::View::piped()).unwrap();
        let crate::cli::CommandOutput::Json(value) = output else {
            panic!("store is json when stdout is a pipe");
        };
        assert_eq!(value["action"], "stored");
        assert_eq!(value["replaced"], true);
        assert_eq!(value["vault_mutated"], true);
        assert_eq!(value["transfer_authorized"], false);
        assert_eq!(value["downloads_started"], false);
        assert!(value["workspace_bytes"].is_null());
        assert_eq!(value["entries"][1]["provenance_verified"], false);
        assert_eq!(value["ceilings"][0]["applied"], false);
        assert_eq!(value["schema_version"], 1);

        let text = crate::cli::Cli::try_parse_from([
            "modelprepper",
            "collection",
            "show",
            "--config",
            config_path.to_str().unwrap(),
            "--name",
            "offline coding",
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
        let crate::cli::CommandOutput::Text {
            body: shown_text,
            ok,
        } = output
        else {
            panic!("text show");
        };
        assert!(ok);
        assert!(shown_text.contains("shown"), "{shown_text}");
        assert!(
            shown_text.contains("this list is not resolved or downloaded"),
            "{shown_text}"
        );
        assert!(
            shown_text.lines().all(|line| line.chars().count() <= 80),
            "{shown_text}"
        );
        let stored_text = crate::present::render_collection(&again, false, 80);
        assert!(stored_text.contains("replaced  yes"), "{stored_text}");
        assert!(
            stored_text.contains("this list is not resolved or downloaded"),
            "{stored_text}"
        );
        let first_text = crate::present::render_collection(&stored, false, 80);
        assert!(first_text.contains("replaced  no"), "{first_text}");

        let unsupported = store(&config, "schema_version = 2\n").unwrap();
        assert!(!unsupported.supported);
        assert_eq!(unsupported.action, "store");
        assert!(!unsupported.vault_mutated);
        let unsupported_text = crate::present::render_collection(&unsupported, false, 80);
        assert!(
            unsupported_text.contains("this schema is not stored"),
            "{unsupported_text}"
        );

        let empty = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(empty.path()).unwrap();
        std::fs::create_dir_all(root.join("catalog")).unwrap();
        std::fs::create_dir_all(root.join("models")).unwrap();
        let missing = crate::Config::default_for(root.join("catalog"), root.join("models"));
        assert_eq!(
            store(&missing, body).unwrap_err().code,
            "vault_not_initialized"
        );
        assert!(!root.join("catalog").join("catalog.sqlite3").exists());
        assert_eq!(
            show(&missing, "offline coding").unwrap_err().code,
            "vault_not_initialized"
        );
        assert_eq!(show(&config, "").unwrap_err().code, "invalid_collection");
        assert_eq!(
            show(&config, "missing").unwrap_err().code,
            "collection_not_stored"
        );

        let mut overflow =
            preview(&listed("[[ceilings]]\ncategory = \"coding\"\nbytes = 1\n")).unwrap();
        overflow.ceilings[0].bytes = u64::MAX;
        assert_eq!(
            crate::vault::write_collection(&config, &overflow)
                .unwrap_err()
                .code,
            "catalog_failed"
        );
        assert_eq!(show(&config, "offline coding").unwrap().entries.len(), 2);
        assert_eq!(show(&config, "shared").unwrap().entries.len(), 2);
    }

    #[test]
    fn an_old_catalog_gains_collection_tables_when_a_list_is_stored() {
        let (_dir, config, _) = ready();
        let path = config.catalog_path.join("catalog.sqlite3");
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "DROP TABLE collection_entry_categories;
                 DROP TABLE collection_entries;
                 DROP TABLE collection_ceilings;
                 DROP TABLE collections;
                 PRAGMA user_version=6;",
            )
            .unwrap();
        drop(connection);
        let stored = store(&config, &one("example/model", "main", "pinned")).unwrap();
        assert_eq!(stored.action, "stored");
        assert_eq!(show(&config, "offline coding").unwrap().entries.len(), 1);
        let connection = rusqlite::Connection::open(&path).unwrap();
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 7);
        let entries: i64 = connection
            .query_row("SELECT COUNT(*) FROM collection_entries", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(entries, 1);
    }

    #[test]
    fn list_pages_stored_names_in_name_order_and_does_not_rank_them() {
        let (_dir, config, config_path) = ready();
        let empty = list(&config, None, 20).unwrap();
        assert!(empty.collections.is_empty());
        assert!(empty.next_after.is_none());
        assert!(!empty.ranked);
        assert!(!empty.vault_mutated);
        assert!(empty.workspace_bytes.is_none());
        let none = crate::present::render_collection_list(&empty, false, 80);
        assert!(none.contains("none"), "{none}");
        assert!(none.contains("collection store"), "{none}");

        store(&config, "schema_version = 1\nname = \"b-list\"\nreplica = \"backup\"\n[[entries]]\nrepo_id = \"example/b\"\nrevision = \"main\"\n").unwrap();
        store(
            &config,
            "schema_version = 1\nname = \"a-list\"\nentries = []\n",
        )
        .unwrap();
        store(
            &config,
            "schema_version = 1\nname = \"c-list\"\n[[entries]]\nrepo_id = \"example/c1\"\nrevision = \"main\"\n[[entries]]\nrepo_id = \"example/c2\"\nrevision = \"main\"\n",
        )
        .unwrap();
        let page = list(&config, None, 2).unwrap();
        assert_eq!(page.action, "list");
        assert!(!page.ranked);
        assert!(!page.transfer_authorized);
        assert!(!page.downloads_started);
        assert_eq!(page.collections.len(), 2);
        assert_eq!(page.collections[0].name, "a-list");
        assert!(page.collections[0].replica.is_none());
        assert_eq!(page.collections[0].entry_count, 0);
        assert_eq!(page.collections[1].name, "b-list");
        assert_eq!(page.collections[1].replica.as_deref(), Some("backup"));
        assert_eq!(page.collections[1].entry_count, 1);
        assert_eq!(page.next_after.as_deref(), Some("b-list"));
        let text = crate::present::render_collection_list(&page, false, 160);
        assert!(text.contains("not ranked"), "{text}");
        assert!(text.contains("no replica"), "{text}");
        assert!(text.contains("after"), "{text}");

        let rest = list(&config, page.next_after.as_deref(), 2).unwrap();
        assert_eq!(rest.collections.len(), 1);
        assert_eq!(rest.collections[0].name, "c-list");
        assert_eq!(rest.collections[0].entry_count, 2);
        assert!(rest.next_after.is_none());
        assert_eq!(rows(&config, "custody"), 0);

        assert_eq!(
            list(&config, None, 0).unwrap_err().code,
            "invalid_collection"
        );
        assert_eq!(
            list(&config, None, 101).unwrap_err().code,
            "invalid_collection"
        );
        assert_eq!(
            list(&config, Some(""), 20).unwrap_err().code,
            "invalid_collection"
        );

        let command = crate::cli::Cli::try_parse_from([
            "modelprepper",
            "collection",
            "list",
            "--config",
            config_path.to_str().unwrap(),
            "--limit",
            "1",
        ])
        .unwrap();
        let output = crate::cli::execute_in(command, crate::cli::View::piped()).unwrap();
        let crate::cli::CommandOutput::Json(value) = output else {
            panic!("list is json when stdout is a pipe");
        };
        assert_eq!(value["action"], "list");
        assert_eq!(value["ranked"], false);
        assert_eq!(value["vault_mutated"], false);
        assert!(value["workspace_bytes"].is_null());
        assert_eq!(value["collections"][0]["name"], "a-list");
        assert_eq!(value["next_after"], "a-list");

        let text_command = crate::cli::Cli::try_parse_from([
            "modelprepper",
            "collection",
            "list",
            "--config",
            config_path.to_str().unwrap(),
        ])
        .unwrap();
        let output = crate::cli::execute_in(
            text_command,
            crate::cli::View {
                text: true,
                ..crate::cli::View::piped()
            },
        )
        .unwrap();
        let crate::cli::CommandOutput::Text { body, ok } = output else {
            panic!("text list");
        };
        assert!(ok);
        assert!(body.contains("listed"), "{body}");
        assert!(
            body.lines().all(|line| line.chars().count() <= 80),
            "{body}"
        );

        let unready = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(unready.path()).unwrap();
        std::fs::create_dir_all(root.join("catalog")).unwrap();
        std::fs::create_dir_all(root.join("models")).unwrap();
        let missing = crate::Config::default_for(root.join("catalog"), root.join("models"));
        assert_eq!(
            list(&missing, None, 20).unwrap_err().code,
            "vault_not_initialized"
        );
    }
}
