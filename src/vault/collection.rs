//! Catalog rows for one shared list. Writing them does not download or apply the list.
use super::{db, open_catalog};
use crate::collection::{
    CategoryCeiling, CollectionEntry, CollectionReport, EvaluationAttribution,
};
use crate::{Config, Error, Result};
use rusqlite::{OptionalExtension, params};

pub(crate) fn write_collection(config: &Config, report: &CollectionReport) -> Result<bool> {
    let mut catalog = open_catalog(config, false)?;
    let transaction = catalog
        .db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(db)?;
    let replaced: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM collections WHERE name = ?1",
            params![report.name],
            |row| row.get(0),
        )
        .map_err(db)?;
    transaction
        .execute(
            "DELETE FROM collections WHERE name = ?1",
            params![report.name],
        )
        .map_err(db)?;
    transaction
        .execute(
            "INSERT INTO collections (name, replica) VALUES (?1, ?2)",
            params![report.name, report.replica],
        )
        .map_err(db)?;
    for (ordinal, ceiling) in report.ceilings.iter().enumerate() {
        let bytes = i64::try_from(ceiling.bytes)
            .map_err(|_| Error::new("catalog_failed", "ceiling bytes do not fit the catalog"))?;
        transaction
            .execute(
                "INSERT INTO collection_ceilings (name, ordinal, category, bytes) VALUES (?1, ?2, ?3, ?4)",
                params![report.name, ordinal as i64, ceiling.category, bytes],
            )
            .map_err(db)?;
    }
    for (position, entry) in report.entries.iter().enumerate() {
        let evaluation = entry.evaluation.as_ref();
        transaction
            .execute(
                "INSERT INTO collection_entries (
                    name, position, repo_id, revision, note, representation,
                    base_repo, base_revision, evaluation_source, evaluation_name,
                    evaluation_value, evidence, provenance
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    report.name,
                    position as i64,
                    entry.repo_id,
                    entry.revision,
                    entry.note,
                    entry.representation,
                    entry.base_repo,
                    entry.base_revision,
                    evaluation.map(|item| item.source.as_str()),
                    evaluation.map(|item| item.name.as_str()),
                    evaluation.map(|item| item.value.as_str()),
                    entry.evidence,
                    entry.provenance,
                ],
            )
            .map_err(db)?;
        for (ordinal, category) in entry.categories.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO collection_entry_categories (name, position, ordinal, category) VALUES (?1, ?2, ?3, ?4)",
                    params![report.name, position as i64, ordinal as i64, category],
                )
                .map_err(db)?;
        }
    }
    transaction.commit().map_err(db)?;
    Ok(replaced > 0)
}

pub(crate) fn read_collection(config: &Config, name: &str) -> Result<CollectionReport> {
    let catalog = open_catalog(config, false)?;
    let replica = catalog
        .db
        .query_row(
            "SELECT replica FROM collections WHERE name = ?1",
            params![name],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(db)?;
    let Some(replica) = replica else {
        return Err(Error::new(
            "collection_not_stored",
            "that collection name is not in the catalog",
        ));
    };
    let mut ceilings = catalog
        .db
        .prepare("SELECT category, bytes FROM collection_ceilings WHERE name = ?1 ORDER BY ordinal")
        .map_err(db)?;
    let ceilings = ceilings
        .query_map(params![name], |row| {
            Ok(CategoryCeiling {
                category: row.get(0)?,
                bytes: row.get::<_, i64>(1)? as u64,
                applied: false,
            })
        })
        .map_err(db)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db)?;
    let mut entries = catalog
        .db
        .prepare(
            "SELECT e.position, e.repo_id, e.revision, e.note, e.representation, e.base_repo,
                    e.base_revision, e.evaluation_source, e.evaluation_name, e.evaluation_value,
                    e.evidence, e.provenance, c.category
             FROM collection_entries e
             LEFT JOIN collection_entry_categories c
               ON c.name = e.name AND c.position = e.position
             WHERE e.name = ?1
             ORDER BY e.position, c.ordinal",
        )
        .map_err(db)?;
    let rows = entries
        .query_map(params![name], |row| {
            Ok(StoredRow {
                position: row.get(0)?,
                repo_id: row.get(1)?,
                revision: row.get(2)?,
                note: row.get(3)?,
                representation: row.get(4)?,
                base_repo: row.get(5)?,
                base_revision: row.get(6)?,
                evaluation_source: row.get(7)?,
                evaluation_name: row.get(8)?,
                evaluation_value: row.get(9)?,
                evidence: row.get(10)?,
                provenance: row.get(11)?,
                category: row.get(12)?,
            })
        })
        .map_err(db)?;
    let mut loaded = Vec::new();
    let mut current = None;
    for row in rows {
        let row = row.map_err(db)?;
        if current != Some(row.position) {
            current = Some(row.position);
            loaded.push(entry_from_row(&row));
        }
        if let Some(category) = row.category {
            let index = loaded.len() - 1;
            loaded[index].categories.push(category);
        }
    }
    let entry_count = loaded.len();
    let shared_bases = crate::collection::shared_bases(&loaded);
    Ok(CollectionReport {
        schema_version: 1,
        action: "show",
        supported: true,
        name: name.to_owned(),
        replica,
        ceilings,
        entries: loaded,
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

struct StoredRow {
    position: i64,
    repo_id: String,
    revision: String,
    note: String,
    representation: Option<String>,
    base_repo: Option<String>,
    base_revision: Option<String>,
    evaluation_source: Option<String>,
    evaluation_name: Option<String>,
    evaluation_value: Option<String>,
    evidence: Option<String>,
    provenance: Option<String>,
    category: Option<String>,
}

fn entry_from_row(row: &StoredRow) -> CollectionEntry {
    let evaluation = if let (Some(source), Some(name), Some(value)) = (
        row.evaluation_source.clone(),
        row.evaluation_name.clone(),
        row.evaluation_value.clone(),
    ) {
        Some(EvaluationAttribution {
            source,
            name,
            value,
            ranked: false,
        })
    } else {
        None
    };
    CollectionEntry {
        repo_id: row.repo_id.clone(),
        revision: row.revision.clone(),
        note: row.note.clone(),
        representation: row.representation.clone(),
        categories: Vec::new(),
        base_repo: row.base_repo.clone(),
        base_revision: row.base_revision.clone(),
        base_preserved: false,
        evaluation,
        evidence: row.evidence.clone(),
        evidence_checked: false,
        provenance: row.provenance.clone(),
        provenance_verified: false,
        resolved: false,
    }
}
