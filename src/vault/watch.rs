//! One bounded pass over the configured watches.
//! A new commit becomes a new plan. Older revisions stay. A fetch happens only
//! inside a current grant and the configured UTC-week byte ceiling.
use super::{grant, open_catalog, review, seal};
use crate::{Config, Error, Result, hub::Resolution, transfer::KernelSocketBuffers};
use serde::Serialize;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Injected clock and transfer client. Production measures the kernel once.
pub(crate) struct WatchIo<'a> {
    pub now: u64,
    pub http: &'a crate::http::Http,
    pub base: &'a str,
    pub kernel: KernelSocketBuffers,
    pub stop: Option<seal::SealStop>,
}

/// One pass over the configured watches. This is not a scheduler.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct WatchReport {
    pub schema_version: u32,
    pub status: &'static str,
    pub period_start: String,
    pub charged_bytes: u64,
    pub transfer_authorized: bool,
    pub workspace_bytes: Option<u64>,
    pub entries: Vec<WatchEntry>,
}

/// What one configured watch did on this pass.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct WatchEntry {
    pub repo_id: String,
    pub requested_revision: String,
    pub resolved_revision: Option<String>,
    pub plan_id: Option<String>,
    pub action: &'static str,
    pub detail: String,
}

pub fn run_watches(config: &Config, stage: &Path) -> Result<WatchReport> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    // An empty list does not open a socket and does not need a vault.
    if config.watches.is_empty() {
        return Ok(report(
            week_start(i64::try_from(now).unwrap_or(i64::MAX)),
            0,
            Vec::new(),
        ));
    }
    let kernel = crate::buffers::measure_kernel_socket_buffers()?;
    let http = crate::http::Http::default();
    run_with(
        config,
        stage,
        crate::hub::resolve,
        &WatchIo {
            now,
            http: &http,
            base: "https://huggingface.co",
            kernel,
            stop: None,
        },
    )
}

pub(crate) fn run_with(
    config: &Config,
    stage: &Path,
    mut resolve: impl FnMut(&str, &str) -> Result<Resolution>,
    io: &WatchIo<'_>,
) -> Result<WatchReport> {
    let period_start = week_start(i64::try_from(io.now).unwrap_or(i64::MAX));
    if config.watches.is_empty() {
        return Ok(report(period_start, 0, Vec::new()));
    }
    // Reject a bad stage before any watch contacts the publisher.
    let stage = seal::checked_stage(config, stage)?;
    let mut entries = Vec::with_capacity(config.watches.len());
    for watch in &config.watches {
        entries.push(one_watch(
            config,
            &stage,
            watch,
            &mut resolve,
            io,
            &period_start,
        )?);
    }
    let charged = period_total(config, &period_start)?;
    Ok(report(period_start, charged, entries))
}

fn one_watch(
    config: &Config,
    stage: &Path,
    watch: &crate::config::Watch,
    resolve: &mut impl FnMut(&str, &str) -> Result<Resolution>,
    io: &WatchIo<'_>,
    period_start: &str,
) -> Result<WatchEntry> {
    let repo_id = watch.repo_id.clone();
    let requested = watch.revision.clone();
    let resolved = match resolve(&watch.repo_id, &watch.revision) {
        Ok(resolved) => resolved,
        Err(error) => {
            return Ok(entry(
                repo_id,
                requested,
                None,
                None,
                source_action(&error)?,
                error.message,
            ));
        }
    };
    let resolved_revision = resolved.inventory.source.resolved_revision.clone();
    if !resolved.inventory.complete {
        return Ok(entry(
            repo_id,
            requested,
            Some(resolved_revision),
            None,
            "partial",
            "the upstream listing is incomplete and was not saved".into(),
        ));
    }
    let proposal = {
        let catalog = open_catalog(config, false)?;
        let current = super::report(&catalog, config)?;
        let volume = current
            .volumes
            .iter()
            .find(|volume| volume.label == config.storage.default_volume)
            .ok_or_else(|| Error::new("invalid_config", "default volume not found"))?;
        let available = volume.available_bytes.ok_or_else(|| {
            Error::new(
                "volume_unavailable",
                "connect the configured destination disk with its original identity",
            )
        })?;
        review::save(
            &catalog,
            config,
            resolved,
            (current.vault_id, volume.volume_id.clone(), available),
        )?
    };
    let plan_id = proposal.plan_id.clone();
    if sealed(config, &plan_id)? {
        return Ok(entry(
            repo_id,
            requested,
            Some(resolved_revision),
            Some(plan_id),
            "unchanged",
            "the revision is already sealed".into(),
        ));
    }
    match grant::current_grant_with(config, &plan_id, io.kernel) {
        Ok(_) => {}
        Err(error) if error.code == "grant_not_recorded" || error.code == "grant_dropped" => {
            return Ok(entry(
                repo_id,
                requested,
                Some(resolved_revision),
                Some(plan_id),
                "proposed",
                "no current grant, so nothing was fetched".into(),
            ));
        }
        Err(error) => return Err(error),
    }
    let payload = proposal.review.payload_bytes;
    let charged = period_total(config, period_start)?;
    if !charge_fits(
        charged,
        payload,
        config.transfer.max_transfer_bytes_per_period,
    )? {
        return Ok(entry(
            repo_id,
            requested,
            Some(resolved_revision),
            Some(plan_id),
            "paused",
            "the utc-week transfer ceiling leaves no room for this revision".into(),
        ));
    }
    if let Err(error) = seal::fetch_with(
        config, &plan_id, stage, io.http, io.base, io.kernel, io.stop,
    ) {
        return Ok(entry(
            repo_id,
            requested,
            Some(resolved_revision),
            Some(plan_id),
            "fetch_failed",
            error.message,
        ));
    }
    record_charge(config, period_start, &plan_id, payload)?;
    Ok(entry(
        repo_id,
        requested,
        Some(resolved_revision),
        Some(plan_id),
        "fetched",
        "sealed inside the current grant".into(),
    ))
}

fn source_action(error: &Error) -> Result<&'static str> {
    Ok(match error.code {
        "revision_not_found" => "source_missing",
        "repository_not_found_or_private" | "access_restricted" | "source_status" => {
            "access_failed"
        }
        "invalid_source_metadata"
        | "source_identity_changed"
        | "invalid_layout"
        | "invalid_license_encoding" => "partial",
        _ => return Err(Error::new(error.code, error.message.clone())),
    })
}

fn entry(
    repo_id: String,
    requested_revision: String,
    resolved_revision: Option<String>,
    plan_id: Option<String>,
    action: &'static str,
    detail: String,
) -> WatchEntry {
    WatchEntry {
        repo_id,
        requested_revision,
        resolved_revision,
        plan_id,
        action,
        detail,
    }
}

fn report(period_start: String, charged_bytes: u64, entries: Vec<WatchEntry>) -> WatchReport {
    WatchReport {
        schema_version: 1,
        status: "watched",
        period_start,
        charged_bytes,
        transfer_authorized: false,
        workspace_bytes: None,
        entries,
    }
}

fn sealed(config: &Config, plan_id: &str) -> Result<bool> {
    let catalog = open_catalog(config, false)?;
    let count: i64 = catalog
        .db
        .query_row(
            "SELECT COUNT(*) FROM custody WHERE plan_id=?1",
            [plan_id],
            |row| row.get(0),
        )
        .map_err(super::db)?;
    Ok(count > 0)
}

fn charge_fits(charged: u64, payload: u64, ceiling: u64) -> Result<bool> {
    let next = charged
        .checked_add(payload)
        .ok_or_else(|| Error::new("size_overflow", "period charges exceed u64 capacity"))?;
    Ok(next <= ceiling)
}

fn period_total(config: &Config, period_start: &str) -> Result<u64> {
    let catalog = open_catalog(config, false)?;
    let total: i64 = catalog
        .db
        .query_row(
            "SELECT COALESCE(SUM(payload_bytes), 0) FROM period_charges WHERE period_start=?1",
            [period_start],
            |row| row.get(0),
        )
        .map_err(super::db)?;
    u64::try_from(total).map_err(|_| Error::new("size_overflow", "period charges exceed u64"))
}

fn record_charge(config: &Config, period_start: &str, plan_id: &str, payload: u64) -> Result<()> {
    let bytes = i64::try_from(payload)
        .map_err(|_| Error::new("size_overflow", "payload does not fit in the period ledger"))?;
    let catalog = open_catalog(config, false)?;
    catalog
        .db
        .execute(
            "INSERT INTO period_charges (period_start, plan_id, payload_bytes) VALUES (?1,?2,?3)
             ON CONFLICT(period_start, plan_id) DO NOTHING",
            rusqlite::params![period_start, plan_id, bytes],
        )
        .map_err(super::db)?;
    Ok(())
}

/// Monday of the UTC week containing `unix_secs`, as `YYYY-MM-DD`.
fn week_start(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86_400);
    // 1970-01-01 was a Thursday. Monday of that week is three days earlier.
    let since_monday = (days + 3).rem_euclid(7);
    let (year, month, day) = civil_from_days(days - since_monday);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Howard Hinnant's `civil_from_days`. `days` is the day count since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { y + 1 } else { y };
    (year, month, day)
}

#[cfg(test)]
#[path = "tests/watch.rs"]
mod tests;
