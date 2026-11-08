//! Search the reviewed source catalog. A match is not a download or an approval.
use crate::{Error, Result};
use serde::Serialize;

const CATALOG: &str = include_str!("../resources/model-sources.json");
const MAX_CONSTRAINT: usize = 80;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct DiscoveryReport {
    pub schema_version: u32,
    pub action: &'static str,
    pub supported: bool,
    pub role: Option<String>,
    pub status: Option<String>,
    pub source_id: Option<String>,
    pub matches: Vec<DiscoveryMatch>,
    pub next_after: Option<String>,
    /// Catalog order is not a rank.
    pub ranked: bool,
    pub transfer_authorized: bool,
    pub network_contact: bool,
    pub vault_mutated: bool,
    pub downloads_started: bool,
    pub workspace_bytes: Option<u64>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct DiscoveryMatch {
    pub id: String,
    pub roles: Vec<String>,
    /// Recorded catalog status. It is not a qualification result.
    pub status: String,
}

#[derive(Debug)]
struct Source {
    id: String,
    roles: Vec<String>,
    status: String,
}

#[derive(Debug)]
struct Constraints {
    role: Option<String>,
    status: Option<String>,
    source_id: Option<String>,
}

/// Search explicit constraints against the reviewed catalog. Nothing is downloaded.
pub fn search(text: &str, after: Option<&str>, limit: u32) -> Result<DiscoveryReport> {
    if !(1..=100).contains(&limit) {
        return Err(Error::new(
            "invalid_discovery",
            "limit must be from 1 to 100",
        ));
    }
    let table: toml::Table =
        toml::from_str(text).map_err(|error| Error::new("invalid_discovery", error.to_string()))?;
    if table
        .get("schema_version")
        .and_then(toml::Value::as_integer)
        != Some(1)
    {
        return Ok(empty(false, None));
    }
    let constraints = constraints(&table)?;
    let sources = catalog()?;
    if let Some(role) = &constraints.role
        && !sources
            .iter()
            .any(|source| source.roles.iter().any(|item| item == role))
    {
        return Err(Error::new(
            "invalid_discovery",
            format!("unknown role {role}"),
        ));
    }
    if let Some(status) = &constraints.status
        && !sources.iter().any(|source| source.status == *status)
    {
        return Err(Error::new(
            "invalid_discovery",
            format!("unknown status {status}"),
        ));
    }
    if let Some(id) = &constraints.source_id
        && !sources.iter().any(|source| source.id == *id)
    {
        return Err(Error::new(
            "invalid_discovery",
            format!("unknown source {id}"),
        ));
    }
    let after_index = match after {
        None => None,
        Some("") => {
            return Err(Error::new("invalid_discovery", "after must name a source"));
        }
        Some(id) => match sources.iter().position(|source| source.id == id) {
            Some(index) => Some(index),
            None => {
                return Err(Error::new(
                    "invalid_discovery",
                    format!("unknown source {id}"),
                ));
            }
        },
    };
    let filtered: Vec<&Source> = sources
        .iter()
        .enumerate()
        .filter(|(index, source)| {
            after_index.is_none_or(|after| *index > after)
                && constraints
                    .role
                    .as_ref()
                    .is_none_or(|role| source.roles.iter().any(|item| item == role))
                && constraints
                    .status
                    .as_ref()
                    .is_none_or(|status| source.status == *status)
                && constraints
                    .source_id
                    .as_ref()
                    .is_none_or(|id| source.id == *id)
        })
        .map(|(_, source)| source)
        .collect();
    let more = filtered.len() > limit as usize;
    let page: Vec<DiscoveryMatch> = filtered
        .into_iter()
        .take(limit as usize)
        .map(|source| DiscoveryMatch {
            id: source.id.clone(),
            roles: source.roles.clone(),
            status: source.status.clone(),
        })
        .collect();
    let next_after = more
        .then(|| page.last().map(|item| item.id.clone()))
        .flatten();
    Ok(DiscoveryReport {
        schema_version: 1,
        action: "discover",
        supported: true,
        role: constraints.role,
        status: constraints.status,
        source_id: constraints.source_id,
        matches: page,
        next_after,
        ranked: false,
        transfer_authorized: false,
        network_contact: false,
        vault_mutated: false,
        downloads_started: false,
        workspace_bytes: None,
    })
}

fn empty(supported: bool, constraints: Option<Constraints>) -> DiscoveryReport {
    let constraints = constraints.unwrap_or(Constraints {
        role: None,
        status: None,
        source_id: None,
    });
    DiscoveryReport {
        schema_version: 1,
        action: "discover",
        supported,
        role: constraints.role,
        status: constraints.status,
        source_id: constraints.source_id,
        matches: Vec::new(),
        next_after: None,
        ranked: false,
        transfer_authorized: false,
        network_contact: false,
        vault_mutated: false,
        downloads_started: false,
        workspace_bytes: None,
    }
}

fn constraints(table: &toml::Table) -> Result<Constraints> {
    for key in table.keys() {
        if !matches!(key.as_str(), "schema_version" | "role" | "status" | "id") {
            return Err(Error::new(
                "invalid_discovery",
                format!("unknown field {key}"),
            ));
        }
    }
    let role = optional_text(table, "role")?;
    let status = optional_text(table, "status")?;
    let source_id = optional_text(table, "id")?;
    if role.is_none() && status.is_none() && source_id.is_none() {
        return Err(Error::new(
            "invalid_discovery",
            "name a role, status, or source id",
        ));
    }
    Ok(Constraints {
        role,
        status,
        source_id,
    })
}

fn optional_text(table: &toml::Table, key: &str) -> Result<Option<String>> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::String(value)) => {
            if value.is_empty()
                || value.chars().count() > MAX_CONSTRAINT
                || value.chars().any(|character| character.is_control())
            {
                return Err(Error::new(
                    "invalid_discovery",
                    format!("{key} must be 1 to {MAX_CONSTRAINT} characters"),
                ));
            }
            Ok(Some(value.clone()))
        }
        Some(_) => Err(Error::new(
            "invalid_discovery",
            format!("{key} must be a string"),
        )),
    }
}

fn catalog() -> Result<Vec<Source>> {
    let value: serde_json::Value = serde_json::from_str(CATALOG)
        .map_err(|_| Error::new("invalid_discovery", "reviewed source catalog is unreadable"))?;
    if value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
    {
        return Err(Error::new(
            "invalid_discovery",
            "reviewed source catalog is unreadable",
        ));
    }
    let Some(items) = value.get("sources").and_then(serde_json::Value::as_array) else {
        return Err(Error::new(
            "invalid_discovery",
            "reviewed source catalog is unreadable",
        ));
    };
    let mut sources = Vec::with_capacity(items.len());
    for item in items {
        let Some(id) = item.get("id").and_then(serde_json::Value::as_str) else {
            return Err(Error::new(
                "invalid_discovery",
                "reviewed source catalog is unreadable",
            ));
        };
        let Some(roles) = item.get("roles").and_then(serde_json::Value::as_array) else {
            return Err(Error::new(
                "invalid_discovery",
                "reviewed source catalog is unreadable",
            ));
        };
        let Some(status) = item
            .get("modelprepper_status")
            .and_then(serde_json::Value::as_str)
        else {
            return Err(Error::new(
                "invalid_discovery",
                "reviewed source catalog is unreadable",
            ));
        };
        let mut parsed_roles = Vec::with_capacity(roles.len());
        for role in roles {
            let Some(role) = role.as_str() else {
                return Err(Error::new(
                    "invalid_discovery",
                    "reviewed source catalog is unreadable",
                ));
            };
            parsed_roles.push(role.to_string());
        }
        sources.push(Source {
            id: id.to_string(),
            roles: parsed_roles,
            status: status.to_string(),
        });
    }
    Ok(sources)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn search_role(role: &str) -> DiscoveryReport {
        search(
            &format!("schema_version = 1\nrole = \"{role}\"\n"),
            None,
            20,
        )
        .unwrap()
    }

    #[test]
    fn an_explicit_role_searches_the_catalog_and_does_not_rank_or_download() {
        let report = search(include_str!("../examples/discovery.toml"), None, 20).unwrap();
        assert!(report.supported);
        assert_eq!(report.action, "discover");
        assert_eq!(report.role.as_deref(), Some("discovery"));
        assert!(!report.ranked);
        assert!(!report.transfer_authorized);
        assert!(!report.network_contact);
        assert!(!report.vault_mutated);
        assert!(!report.downloads_started);
        assert!(report.workspace_bytes.is_none());
        assert!(report.next_after.is_none());
        assert_eq!(report.matches[0].id, "huggingface");
        assert_eq!(report.matches[0].status, "inspection_only");
        assert!(report.matches[0].roles.contains(&"discovery".to_string()));
        assert!(report.matches.iter().any(|item| item.id == "ollama"));
        assert!(
            !report
                .matches
                .iter()
                .any(|item| item.id == "github-releases")
        );
        assert!(
            !report
                .matches
                .iter()
                .any(|item| item.id == "publisher-direct")
        );
        let page = search(include_str!("../examples/discovery.toml"), None, 2).unwrap();
        assert_eq!(page.matches.len(), 2);
        assert_eq!(page.matches[0].id, "huggingface");
        assert_eq!(page.matches[1].id, "modelscope");
        assert_eq!(page.next_after.as_deref(), Some("modelscope"));
        let next = search(
            include_str!("../examples/discovery.toml"),
            Some("modelscope"),
            2,
        )
        .unwrap();
        assert_eq!(next.matches[0].id, "nvidia-ngc");
        assert_eq!(next.matches[1].id, "kaggle-models");
        let tail = search(
            include_str!("../examples/discovery.toml"),
            Some("pirateface"),
            20,
        )
        .unwrap();
        assert!(tail.matches.is_empty());
        assert!(tail.next_after.is_none());
        let both = search(
            "schema_version = 1\nrole = \"discovery\"\nstatus = \"inspection_only\"\n",
            None,
            20,
        )
        .unwrap();
        assert_eq!(both.matches.len(), 1);
        assert_eq!(both.matches[0].id, "huggingface");
        let host = search_role("host");
        assert!(host.matches.iter().any(|item| item.id == "github-releases"));
        let text = crate::present::render_discovery(&report, false, 80);
        assert!(text.contains("matched"), "{text}");
        assert!(text.contains("not a download"), "{text}");
        assert!(text.contains("not a download or an approval"), "{text}");
        assert!(
            text.lines().all(|line| line.chars().count() <= 80),
            "{text}"
        );
        let narrow = crate::present::render_discovery(&page, false, 80);
        assert!(narrow.contains("next  modelscope"), "{narrow}");
    }

    #[test]
    fn an_unknown_schema_is_not_searched_and_a_bad_constraint_is_refused() {
        let unsupported = search("schema_version = 2\nrole = \"nope\"\n", None, 20).unwrap();
        assert!(!unsupported.supported);
        assert!(unsupported.matches.is_empty());
        assert!(unsupported.role.is_none());
        let header = search("[1]\n", None, 20).unwrap();
        assert!(!header.supported);
        let unsupported_text = crate::present::render_discovery(&unsupported, false, 80);
        assert!(
            unsupported_text.contains("unsupported"),
            "{unsupported_text}"
        );
        assert!(
            unsupported_text.contains("this schema is not searched"),
            "{unsupported_text}"
        );
        assert_eq!(
            search("schema_version = 1\n", None, 20).unwrap_err().code,
            "invalid_discovery"
        );
        assert_eq!(
            search("schema_version = 1\nrole = \"leaderboard\"\n", None, 20)
                .unwrap_err()
                .code,
            "invalid_discovery"
        );
        assert_eq!(
            search("schema_version = 1\nid = \"missing\"\n", None, 20)
                .unwrap_err()
                .code,
            "invalid_discovery"
        );
        assert_eq!(
            search("schema_version = 1\nstatus = \"enabled\"\n", None, 20)
                .unwrap_err()
                .code,
            "invalid_discovery"
        );
        assert_eq!(
            search("schema_version = 1\nrole = 1\n", None, 20)
                .unwrap_err()
                .code,
            "invalid_discovery"
        );
        assert_eq!(
            search("schema_version = 1\nalpha = 1\nrole = \"host\"\n", None, 20)
                .unwrap_err()
                .code,
            "invalid_discovery"
        );
        assert_eq!(search("[", None, 20).unwrap_err().code, "invalid_discovery");
        assert_eq!(
            search("schema_version = 1\nrole = \"host\"\n", Some(""), 20)
                .unwrap_err()
                .code,
            "invalid_discovery"
        );
        assert_eq!(
            search("schema_version = 1\nrole = \"host\"\n", Some("missing"), 20)
                .unwrap_err()
                .code,
            "invalid_discovery"
        );
        assert_eq!(
            search("schema_version = 1\nrole = \"host\"\n", None, 0)
                .unwrap_err()
                .code,
            "invalid_discovery"
        );
        assert_eq!(
            search("schema_version = 1\nrole = \"host\"\n", None, 101)
                .unwrap_err()
                .code,
            "invalid_discovery"
        );
        let one = search("schema_version = 1\nid = \"ollama\"\n", None, 20).unwrap();
        assert_eq!(one.matches.len(), 1);
        assert_eq!(one.source_id.as_deref(), Some("ollama"));
        assert!(one.matches[0].roles.contains(&"local_import".to_string()));
        let none = crate::present::render_discovery(
            &search(
                "schema_version = 1\nrole = \"publisher\"\nstatus = \"inspection_only\"\n",
                None,
                20,
            )
            .unwrap(),
            false,
            80,
        );
        assert!(none.contains("none"), "{none}");
    }

    #[test]
    fn the_command_searches_a_file_and_does_not_open_a_vault() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), include_str!("../examples/discovery.toml")).unwrap();
        let command = crate::cli::Cli::try_parse_from([
            "modelprepper",
            "discover",
            "--file",
            file.path().to_str().unwrap(),
            "--limit",
            "1",
        ])
        .unwrap();
        let output = crate::cli::execute_in(command, crate::cli::View::piped()).unwrap();
        let crate::cli::CommandOutput::Json(value) = output else {
            panic!("discover is json when stdout is a pipe");
        };
        assert_eq!(value["action"], "discover");
        assert_eq!(value["ranked"], false);
        assert_eq!(value["network_contact"], false);
        assert_eq!(value["transfer_authorized"], false);
        assert!(value["workspace_bytes"].is_null());
        assert_eq!(value["matches"][0]["id"], "huggingface");
        assert_eq!(value["next_after"], "huggingface");
        assert!(
            !file
                .path()
                .parent()
                .unwrap()
                .join("catalog.sqlite3")
                .exists()
        );
    }
}
