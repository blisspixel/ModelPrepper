//! Read-only offline readiness for one declared model profile.
//!
//! The recipe is data. This module does not contact a publisher, execute a
//! runtime, install packages, or write the vault. A recorded drill is evidence
//! already stored in the recipe. Reading it is not a new drill.

use crate::{
    Error, Result,
    integrity::hash_exact,
    inventory::Role,
    plan::digest,
    validation::{normalize_repo, unique_paths, validate_file_path, validate_hex},
    verify,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    path::Path,
};

/// Retained files at or below this size are hashed. Larger files are counted
/// and left unverified. This is not a runtime load.
const HASH_LIMIT: u64 = 2 * 1024 * 1024;

const PROFILE: &str = "publisher-safetensors-text-v1";
const DOMAIN: &str = "modelprepper-readiness-recipe-v1";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub schema_version: u32,
    pub profile: String,
    pub model: RecipeModel,
    pub capabilities: Vec<String>,
    pub runtime: RuntimeDeclaration,
    pub rights: RightsDeclaration,
    #[serde(default)]
    pub recorded_drill: Option<DrillRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecipeModel {
    pub endpoint: String,
    pub repo_id: String,
    pub resolved_revision: String,
    pub files: Vec<RecipeFile>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecipeFile {
    pub path: String,
    pub role: Role,
    pub capability: String,
    pub need: Need,
    pub retention: Retention,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub base: Option<BasePin>,
    #[serde(default)]
    pub locator: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BasePin {
    pub repo_id: String,
    #[serde(default)]
    pub resolved_revision: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeDeclaration {
    pub name: String,
    pub version: String,
    pub os: String,
    pub arch: String,
    pub artifact: ArtifactDeclaration,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactDeclaration {
    pub retention: Retention,
    pub locator: String,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RightsDeclaration {
    pub status: RightsStatus,
    pub license_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DrillRecord {
    pub schema_version: u32,
    pub recipe_digest: String,
    pub result: DrillResult,
    pub capability: String,
    pub os: String,
    pub arch: String,
    pub runtime_version: String,
    pub network: String,
    pub cache: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Need {
    Required,
    Optional,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Retention {
    Retained,
    Named,
    Remote,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RightsStatus {
    Matched,
    Unreviewed,
    Unresolved,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DrillResult {
    LoadPassed,
    TaskPassed,
    Failed,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Report {
    pub schema_version: u32,
    pub mode: &'static str,
    pub recipe_digest: String,
    pub profile: String,
    pub repo_id: String,
    pub resolved_revision: String,
    pub transfer_authorized: bool,
    pub network_contact: bool,
    pub executed: bool,
    pub vault_mutated: bool,
    pub integrity: &'static str,
    pub provenance: &'static str,
    pub rights: &'static str,
    pub availability: &'static str,
    pub environment: &'static str,
    pub runtime_retention: &'static str,
    pub hashed_files: u32,
    pub unverified_files: u32,
    pub offline_readiness: &'static str,
    pub capabilities: Vec<CapabilityReport>,
    pub next: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct CapabilityReport {
    pub id: String,
    pub closure: &'static str,
    pub blocked_by: Vec<String>,
    pub excluded: Vec<String>,
}

impl Recipe {
    pub fn parse(input: &str) -> Result<Self> {
        let recipe: Self = serde_json::from_str(input)
            .map_err(|error| Error::new("invalid_recipe", error.to_string()))?;
        recipe.validate()?;
        Ok(recipe)
    }

    fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err(Error::new(
                "unsupported_schema",
                "readiness recipe schema_version must be 1",
            ));
        }
        if self.profile != PROFILE {
            return Err(Error::new(
                "unsupported_profile",
                "the supported readiness profile is publisher-safetensors-text-v1",
            ));
        }
        if self.model.endpoint != "https://huggingface.co" {
            return Err(Error::new(
                "unsupported_source",
                "only official Hugging Face model recipes are supported",
            ));
        }
        normalize_repo(&self.model.repo_id)?;
        validate_hex(&self.model.resolved_revision, 40)?;
        if self.capabilities.is_empty()
            || self.capabilities.iter().any(|id| id.is_empty())
            || self.capabilities.iter().any(|id| {
                self.capabilities
                    .iter()
                    .filter(|other| *other == id)
                    .count()
                    > 1
            })
        {
            return Err(Error::new(
                "invalid_recipe",
                "capabilities must be unique and non-empty",
            ));
        }
        if self.model.files.is_empty() {
            return Err(Error::new("invalid_recipe", "a recipe names its files"));
        }
        unique_paths(self.model.files.iter().map(|file| file.path.as_str()))?;
        for file in &self.model.files {
            validate_file_path(&file.path)?;
            if !self.capabilities.iter().any(|id| id == &file.capability) {
                return Err(Error::new(
                    "invalid_recipe",
                    "every file capability must be selected",
                ));
            }
            check_retention(file.retention, file.size_bytes, file.sha256.as_deref())?;
            if let Some(locator) = &file.locator {
                check_locator(file.retention, locator)?;
            }
            if let Some(base) = &file.base {
                normalize_repo(&base.repo_id)?;
                if let Some(revision) = &base.resolved_revision {
                    validate_hex(revision, 40)?;
                }
            }
        }
        for capability in &self.capabilities {
            if !self
                .model
                .files
                .iter()
                .any(|file| &file.capability == capability && file.need == Need::Required)
            {
                return Err(Error::new(
                    "invalid_recipe",
                    format!("{capability} needs at least one required file"),
                ));
            }
        }
        if self.capabilities.iter().any(|id| id == "text-generation") {
            for role in [Role::Config, Role::Tokenizer, Role::Weight, Role::Template] {
                let declared = self.model.files.iter().any(|file| {
                    file.capability == "text-generation"
                        && file.role == role
                        && file.need == Need::Required
                });
                if !declared {
                    return Err(Error::new(
                        "invalid_recipe",
                        "text-generation requires config, tokenizer, weight, and template files",
                    ));
                }
            }
        }
        validate_platform(&self.runtime.os, &self.runtime.arch)?;
        if self.runtime.name.is_empty() || self.runtime.version.is_empty() {
            return Err(Error::new(
                "invalid_recipe",
                "runtime name and version are required",
            ));
        }
        check_retention(
            self.runtime.artifact.retention,
            self.runtime.artifact.size_bytes,
            self.runtime.artifact.sha256.as_deref(),
        )?;
        check_locator(
            self.runtime.artifact.retention,
            &self.runtime.artifact.locator,
        )?;
        if self.runtime.artifact.retention == Retention::Retained
            && self
                .model
                .files
                .iter()
                .any(|file| file.path == self.runtime.artifact.locator)
        {
            return Err(Error::new(
                "invalid_recipe",
                "the runtime artifact path is not a model file",
            ));
        }
        if self.rights.license_id.is_empty() {
            return Err(Error::new("invalid_recipe", "rights need a license id"));
        }
        if let Some(drill) = &self.recorded_drill {
            if drill.schema_version != 1 {
                return Err(Error::new(
                    "unsupported_schema",
                    "recorded drill schema_version must be 1",
                ));
            }
            validate_hex(&drill.recipe_digest, 64)?;
            validate_platform(&drill.os, &drill.arch)?;
            if !self.capabilities.iter().any(|id| id == &drill.capability) {
                return Err(Error::new(
                    "invalid_recipe",
                    "a recorded drill names a selected capability",
                ));
            }
            if drill.runtime_version != self.runtime.version {
                return Err(Error::new(
                    "invalid_recipe",
                    "a recorded drill names the recipe runtime version",
                ));
            }
            if drill.network != "disabled" || drill.cache != "empty" {
                return Err(Error::new(
                    "invalid_recipe",
                    "a recorded drill must say network disabled and cache empty",
                ));
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        #[derive(Serialize)]
        struct Body<'a> {
            domain: &'static str,
            schema_version: u32,
            profile: &'a str,
            model: RecipeModel,
            capabilities: Vec<String>,
            runtime: &'a RuntimeDeclaration,
            rights: &'a RightsDeclaration,
        }
        let mut model = self.model.clone();
        model
            .files
            .sort_by(|left, right| left.path.cmp(&right.path));
        let mut capabilities = self.capabilities.clone();
        capabilities.sort();
        let body = Body {
            domain: DOMAIN,
            schema_version: self.schema_version,
            profile: &self.profile,
            model,
            capabilities,
            runtime: &self.runtime,
            rights: &self.rights,
        };
        digest(&serde_json::to_vec(&body).expect("recipe digest body"))
    }
}

pub fn preflight(recipe: &Recipe, path: &Path) -> Result<Report> {
    let recipe_digest = recipe.digest();
    let available = directory_available(path)?;
    let mut hashed_files = 0_u32;
    let mut unverified_files = 0_u32;
    let mut damaged = false;
    let mut findings = Vec::new();
    if available {
        for file in &recipe.model.files {
            let finding = inspect_retained(path, file, &mut hashed_files, &mut unverified_files)?;
            damaged |= finding.damaged;
            findings.push(finding);
        }
        if recipe.runtime.artifact.retention == Retention::Retained {
            let runtime_file = RecipeFile {
                path: recipe.runtime.artifact.locator.clone(),
                role: Role::Other,
                capability: String::new(),
                need: Need::Required,
                retention: Retention::Retained,
                size_bytes: recipe.runtime.artifact.size_bytes,
                sha256: recipe.runtime.artifact.sha256.clone(),
                base: None,
                locator: None,
            };
            let finding = inspect_retained(
                path,
                &runtime_file,
                &mut hashed_files,
                &mut unverified_files,
            )?;
            damaged |= finding.damaged;
            findings.push(finding);
        }
    }
    let capabilities = recipe
        .capabilities
        .iter()
        .map(|id| capability_report(id, &recipe.model.files, &findings))
        .collect::<Vec<_>>();
    let integrity = if !available {
        "unavailable"
    } else if damaged {
        "damaged"
    } else if unverified_files > 0 || findings.iter().any(|finding| finding.missing) {
        "not_checked"
    } else if hashed_files > 0 {
        "matched"
    } else {
        "not_checked"
    };
    let environment = if recipe.runtime.os != "any" && recipe.runtime.os != host_os()
        || recipe.runtime.arch != "any" && recipe.runtime.arch != host_arch()
    {
        "incompatible"
    } else {
        "compatible"
    };
    let runtime_retention = match recipe.runtime.artifact.retention {
        Retention::Retained => {
            // The runtime finding is recorded after the model files. A shared
            // path must use that later finding, not the model file's result.
            match findings
                .iter()
                .rev()
                .find(|finding| finding.path == recipe.runtime.artifact.locator)
            {
                Some(finding) if finding.satisfied => "retained",
                Some(finding) if finding.unverified => "unverified",
                Some(finding) if finding.damaged => "damaged",
                _ => "missing",
            }
        }
        Retention::Named => "named",
        Retention::Remote => "remote",
    };
    let rights = match recipe.rights.status {
        RightsStatus::Matched => "declared_matched",
        RightsStatus::Unreviewed => "unreviewed",
        RightsStatus::Unresolved => "unresolved",
    };
    let offline_readiness = readiness_state(
        recipe,
        &recipe_digest,
        integrity,
        environment,
        runtime_retention,
        rights,
        &capabilities,
    );
    let next = next_action(
        available,
        environment,
        rights,
        integrity,
        runtime_retention,
        offline_readiness,
        &capabilities,
    );
    Ok(Report {
        schema_version: 1,
        mode: "offline-preflight",
        recipe_digest,
        profile: recipe.profile.clone(),
        repo_id: recipe.model.repo_id.clone(),
        resolved_revision: recipe.model.resolved_revision.clone(),
        transfer_authorized: false,
        network_contact: false,
        executed: false,
        vault_mutated: false,
        integrity,
        provenance: "declared_only",
        rights,
        availability: if available { "present" } else { "unavailable" },
        environment,
        runtime_retention,
        hashed_files,
        unverified_files,
        offline_readiness,
        capabilities,
        next,
    })
}

struct Finding {
    path: String,
    satisfied: bool,
    missing: bool,
    damaged: bool,
    unverified: bool,
    optional_absent: bool,
    reason: String,
}

fn capability_report(id: &str, files: &[RecipeFile], findings: &[Finding]) -> CapabilityReport {
    let mut blocked_by = Vec::new();
    let mut excluded = Vec::new();
    let mut unknown = false;
    let mut missing = false;
    let mut unverified = false;
    for file in files.iter().filter(|file| file.capability == id) {
        if file.need == Need::Unknown {
            unknown = true;
            blocked_by.push(format!("{} unknown", file.path));
            continue;
        }
        if let Some(base) = &file.base
            && base.resolved_revision.is_none()
        {
            unknown = true;
            blocked_by.push(format!("{} unresolved_base", file.path));
        }
        let finding = findings.iter().find(|finding| finding.path == file.path);
        match finding {
            Some(finding) if finding.optional_absent => excluded.push(file.path.clone()),
            Some(finding) if finding.unverified && file.need == Need::Required => {
                unverified = true;
                blocked_by.push(format!("{} unverified", file.path));
            }
            Some(finding) if !finding.satisfied && file.need == Need::Required => {
                missing = true;
                blocked_by.push(format!("{} {}", file.path, finding.reason));
            }
            None if file.need == Need::Optional => excluded.push(file.path.clone()),
            None if file.retention != Retention::Retained => {
                missing = true;
                blocked_by.push(format!(
                    "{} {}",
                    file.path,
                    retention_reason(file.retention)
                ));
            }
            None => {
                missing = true;
                blocked_by.push(format!("{} missing", file.path));
            }
            _ => {}
        }
    }
    let closure = if unknown {
        "unknown"
    } else if missing {
        "missing"
    } else if unverified {
        "present_unverified"
    } else {
        "complete"
    };
    CapabilityReport {
        id: id.to_owned(),
        closure,
        blocked_by,
        excluded,
    }
}

fn readiness_state(
    recipe: &Recipe,
    digest: &str,
    integrity: &str,
    environment: &str,
    runtime_retention: &str,
    rights: &str,
    capabilities: &[CapabilityReport],
) -> &'static str {
    let Some(drill) = &recipe.recorded_drill else {
        return "untested";
    };
    let applies = drill.recipe_digest == digest
        && drill.os == host_os()
        && drill.arch == host_arch()
        && drill.network == "disabled"
        && drill.cache == "empty"
        && environment == "compatible"
        && integrity == "matched"
        && rights == "declared_matched"
        && runtime_retention == "retained"
        && capabilities.iter().any(|capability| {
            capability.id == drill.capability && capability.closure == "complete"
        });
    if !applies {
        return "stale";
    }
    match drill.result {
        DrillResult::Failed => "failed",
        DrillResult::LoadPassed => "load_passed",
        DrillResult::TaskPassed => "task_passed",
    }
}

fn next_action(
    available: bool,
    environment: &str,
    rights: &str,
    integrity: &str,
    runtime_retention: &str,
    readiness: &str,
    capabilities: &[CapabilityReport],
) -> String {
    if !available {
        return "pass the directory that holds the retained files; a missing directory is not created".into();
    }
    if environment == "incompatible" {
        return "this recipe declares a different operating system or architecture".into();
    }
    if rights != "declared_matched" {
        return "review the license evidence; an unresolved or unreviewed claim is not allowed"
            .into();
    }
    if let Some(capability) = capabilities
        .iter()
        .find(|capability| capability.closure == "unknown")
    {
        return format!(
            "{} closure is unknown: {}",
            capability.id,
            capability.blocked_by.join(", ")
        );
    }
    if let Some(capability) = capabilities
        .iter()
        .find(|capability| capability.closure == "missing")
    {
        return format!(
            "{} is missing: {}",
            capability.id,
            capability.blocked_by.join(", ")
        );
    }
    if integrity == "damaged" {
        return "a retained file does not match the recipe; do not treat it as ready".into();
    }
    if runtime_retention == "missing" {
        return "the retained runtime file is missing; do not treat that locator as a local package"
            .into();
    }
    if runtime_retention == "unverified" {
        return "the retained runtime file is larger than the ordinary hash limit and was not hashed"
            .into();
    }
    if runtime_retention != "retained" {
        return "the runtime is only named or remote; a lockfile or URL is not a retained package"
            .into();
    }
    if integrity == "not_checked" {
        return "a retained file is larger than the ordinary hash limit and was not hashed".into();
    }
    match readiness {
        "stale" => "the recorded drill does not apply to this recipe and machine".into(),
        "failed" => "the recorded drill failed; this command did not run the model again".into(),
        "load_passed" | "task_passed" => {
            "the recipe records a drill for this machine; this command did not run the model".into()
        }
        _ => "files match and the runtime was not run; a network-disabled empty-cache drill is still required".into(),
    }
}

fn directory_available(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if link_or_reparse(&metadata) {
                return Err(Error::new(
                    "unsafe_path",
                    "the model directory is a symlink or reparse point and was not followed",
                ));
            }
            if !metadata.is_dir() {
                return Err(Error::new(
                    "unsafe_path",
                    "the model path must be a real directory",
                ));
            }
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(Error::new(
            "input_read_failed",
            format!("{}: {error}", path.display()),
        )),
    }
}

fn inspect_retained(
    root: &Path,
    file: &RecipeFile,
    hashed_files: &mut u32,
    unverified_files: &mut u32,
) -> Result<Finding> {
    let mut finding = Finding {
        path: file.path.clone(),
        satisfied: false,
        missing: false,
        damaged: false,
        unverified: false,
        optional_absent: false,
        reason: String::new(),
    };
    if file.need == Need::Unknown {
        finding.reason = "unknown".into();
        return Ok(finding);
    }
    if file.retention != Retention::Retained {
        finding.reason = retention_reason(file.retention).into();
        return Ok(finding);
    }
    let meta = match verify::metadata_nofollow(root, &file.path) {
        Ok(Some(meta)) => meta,
        Ok(None) => {
            if file.need == Need::Optional {
                finding.optional_absent = true;
            } else {
                finding.missing = true;
                finding.reason = "missing".into();
            }
            return Ok(finding);
        }
        Err(error) if error.code == "unsafe_path" => {
            finding.damaged = true;
            finding.reason = "unsafe_link".into();
            return Ok(finding);
        }
        Err(error) => return Err(error),
    };
    if !meta.is_file() {
        finding.damaged = true;
        finding.reason = "not_a_file".into();
        return Ok(finding);
    }
    let declared = file.size_bytes.unwrap_or(0);
    if meta.len() != declared {
        finding.damaged = true;
        finding.reason = "size_mismatch".into();
        return Ok(finding);
    }
    if declared > HASH_LIMIT {
        finding.unverified = true;
        *unverified_files += 1;
        finding.reason = "unverified".into();
        return Ok(finding);
    }
    let hashed = hash_exact(
        File::open(root.join(&file.path))
            .map_err(|error| Error::new("input_read_failed", format!("{}: {error}", file.path)))?,
        declared,
        |_| {},
    )?;
    *hashed_files += 1;
    if Some(hashed.sha256.as_str()) != file.sha256.as_deref() {
        finding.damaged = true;
        finding.reason = "hash_mismatch".into();
        return Ok(finding);
    }
    finding.satisfied = true;
    Ok(finding)
}

fn retention_reason(retention: Retention) -> &'static str {
    match retention {
        Retention::Named => "named",
        Retention::Remote => "remote",
        Retention::Retained => "retained",
    }
}

fn check_retention(retention: Retention, size: Option<u64>, sha256: Option<&str>) -> Result<()> {
    if retention != Retention::Retained {
        return Ok(());
    }
    let Some(sha256) = sha256 else {
        return Err(Error::new(
            "invalid_recipe",
            "a retained file declares its sha256",
        ));
    };
    validate_hex(sha256, 64)?;
    if size.is_none() {
        return Err(Error::new(
            "invalid_recipe",
            "a retained file declares its size",
        ));
    }
    Ok(())
}

fn check_locator(retention: Retention, locator: &str) -> Result<()> {
    if locator.is_empty() || locator.len() > 240 || !locator.is_ascii() {
        return Err(Error::new(
            "invalid_recipe",
            "a locator must be short ASCII",
        ));
    }
    match retention {
        Retention::Retained => validate_file_path(locator),
        Retention::Named => {
            if locator.contains("://") || locator.chars().any(char::is_whitespace) {
                Err(Error::new(
                    "invalid_recipe",
                    "a named artifact is a package name, not a URL",
                ))
            } else {
                Ok(())
            }
        }
        Retention::Remote => {
            let url = url::Url::parse(locator).map_err(|_| {
                Error::new(
                    "invalid_recipe",
                    "a remote locator must be https without credentials, a query, or a fragment",
                )
            })?;
            if url.scheme() != "https"
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(Error::new(
                    "invalid_recipe",
                    "a remote locator must be https without credentials, a query, or a fragment",
                ));
            }
            Ok(())
        }
    }
}

fn validate_platform(os: &str, arch: &str) -> Result<()> {
    if !matches!(os, "any" | "windows" | "linux" | "macos")
        || !matches!(arch, "any" | "x86_64" | "aarch64")
    {
        return Err(Error::new(
            "invalid_recipe",
            "os must be any, windows, linux, or macos; arch must be any, x86_64, or aarch64",
        ));
    }
    Ok(())
}

fn host_os() -> &'static str {
    std::env::consts::OS
}

fn host_arch() -> &'static str {
    std::env::consts::ARCH
}

fn link_or_reparse(meta: &fs::Metadata) -> bool {
    let linked = meta.file_type().is_symlink();
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        linked || meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    linked
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

    fn sha(bytes: &[u8]) -> String {
        digest(bytes)
    }

    fn file(
        path: &str,
        role: Role,
        capability: &str,
        need: Need,
        retention: Retention,
        bytes: Option<&[u8]>,
    ) -> RecipeFile {
        RecipeFile {
            path: path.into(),
            role,
            capability: capability.into(),
            need,
            retention,
            size_bytes: bytes.map(|bytes| bytes.len() as u64),
            sha256: bytes.map(sha),
            base: None,
            locator: None,
        }
    }

    fn recipe(files: Vec<RecipeFile>, capabilities: &[&str]) -> Recipe {
        Recipe {
            schema_version: 1,
            profile: PROFILE.into(),
            model: RecipeModel {
                endpoint: "https://huggingface.co".into(),
                repo_id: "example/model".into(),
                resolved_revision: COMMIT.into(),
                files,
            },
            capabilities: capabilities.iter().map(|id| (*id).to_owned()).collect(),
            runtime: RuntimeDeclaration {
                name: "transformers".into(),
                version: "4.55.2".into(),
                os: "any".into(),
                arch: "any".into(),
                artifact: ArtifactDeclaration {
                    retention: Retention::Named,
                    locator: "transformers==4.55.2".into(),
                    size_bytes: None,
                    sha256: None,
                },
            },
            rights: RightsDeclaration {
                status: RightsStatus::Matched,
                license_id: "apache-2.0".into(),
            },
            recorded_drill: None,
        }
    }

    fn text_files() -> Vec<RecipeFile> {
        vec![
            file(
                "config.json",
                Role::Config,
                "text-generation",
                Need::Required,
                Retention::Retained,
                Some(b"{}"),
            ),
            file(
                "tokenizer.json",
                Role::Tokenizer,
                "text-generation",
                Need::Required,
                Retention::Retained,
                Some(b"{\"v\":1}"),
            ),
            file(
                "model.safetensors",
                Role::Weight,
                "text-generation",
                Need::Required,
                Retention::Retained,
                Some(b"weights"),
            ),
            file(
                "chat_template.jinja",
                Role::Template,
                "text-generation",
                Need::Required,
                Retention::Retained,
                Some(b"{{ content }}"),
            ),
        ]
    }

    fn write_files(dir: &Path, files: &[RecipeFile]) {
        fs::create_dir_all(dir).unwrap();
        for file in files {
            if file.retention != Retention::Retained {
                continue;
            }
            let bytes = match file.path.as_str() {
                "config.json" => b"{}".as_slice(),
                "tokenizer.json" => b"{\"v\":1}".as_slice(),
                "model.safetensors" => b"weights".as_slice(),
                "chat_template.jinja" => b"{{ content }}".as_slice(),
                "runtime.bin" => b"runtime".as_slice(),
                _ => b"extra".as_slice(),
            };
            fs::write(dir.join(&file.path), bytes).unwrap();
        }
    }

    #[test]
    fn complete_metadata_stays_untested_and_does_not_create_files() {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path().join("model");
        let files = text_files();
        write_files(&dir, &files);
        let before = fs::read_dir(&dir).unwrap().count();
        let report = preflight(&recipe(files, &["text-generation"]), &dir).unwrap();
        assert_eq!(report.offline_readiness, "untested");
        assert_eq!(report.integrity, "matched");
        assert_eq!(report.availability, "present");
        assert_eq!(report.runtime_retention, "named");
        assert_eq!(report.rights, "declared_matched");
        assert_eq!(report.provenance, "declared_only");
        assert!(!report.transfer_authorized);
        assert!(!report.network_contact);
        assert!(!report.executed);
        assert!(!report.vault_mutated);
        assert_eq!(report.capabilities[0].closure, "complete");
        assert_eq!(report.hashed_files, 4);
        assert!(report.next.contains("named") || report.next.contains("lockfile"));
        let text = crate::present::render_readiness(&report, false, 80);
        assert!(text.starts_with("untested"));
        assert!(text.contains("matched"));
        assert!(text.contains("named"));
        assert!(text.contains("declared_matched"));
        assert!(text.contains("next:"));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), before);
        assert!(!temp.path().join("catalog").exists());
    }

    #[test]
    fn missing_template_blocks_only_text_generation() {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path().join("model");
        let mut files = text_files();
        write_files(&dir, &files);
        fs::remove_file(dir.join("chat_template.jinja")).unwrap();
        let report = preflight(&recipe(files.split_off(0), &["text-generation"]), &dir).unwrap();
        assert_eq!(report.capabilities[0].closure, "missing");
        assert!(
            report.capabilities[0]
                .blocked_by
                .iter()
                .any(|item| item.contains("chat_template.jinja"))
        );
        assert_eq!(report.offline_readiness, "untested");
        assert_ne!(report.integrity, "matched");
    }

    #[test]
    fn missing_optional_capability_does_not_block_text_generation() {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path().join("model");
        let mut files = text_files();
        write_files(&dir, &files);
        files.push(file(
            "transcribe.bin",
            Role::Weight,
            "transcription",
            Need::Required,
            Retention::Retained,
            Some(b"audio"),
        ));
        let report =
            preflight(&recipe(files, &["text-generation", "transcription"]), &dir).unwrap();
        let text = report
            .capabilities
            .iter()
            .find(|item| item.id == "text-generation")
            .unwrap();
        let audio = report
            .capabilities
            .iter()
            .find(|item| item.id == "transcription")
            .unwrap();
        assert_eq!(text.closure, "complete");
        assert_eq!(audio.closure, "missing");
    }

    #[test]
    fn unresolved_base_is_unknown_rather_than_complete() {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path().join("model");
        let mut files = text_files();
        write_files(&dir, &files);
        let mut adapter = file(
            "adapter.safetensors",
            Role::Weight,
            "text-generation",
            Need::Required,
            Retention::Retained,
            Some(b"adapter"),
        );
        fs::write(dir.join("adapter.safetensors"), b"adapter").unwrap();
        adapter.base = Some(BasePin {
            repo_id: "example/base".into(),
            resolved_revision: None,
        });
        files.push(adapter);
        let report = preflight(&recipe(files, &["text-generation"]), &dir).unwrap();
        assert_eq!(report.capabilities[0].closure, "unknown");
        assert!(
            report.capabilities[0]
                .blocked_by
                .iter()
                .any(|item| item.contains("unresolved_base"))
        );
    }

    #[test]
    fn changed_bytes_are_damaged_and_a_recorded_pass_becomes_stale() {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path().join("model");
        let files = text_files();
        write_files(&dir, &files);
        fs::write(dir.join("model.safetensors"), b"changed").unwrap();
        let mut recipe = recipe(files, &["text-generation"]);
        recipe.recorded_drill = Some(DrillRecord {
            schema_version: 1,
            recipe_digest: recipe.digest(),
            result: DrillResult::TaskPassed,
            capability: "text-generation".into(),
            os: host_os().into(),
            arch: host_arch().into(),
            runtime_version: recipe.runtime.version.clone(),
            network: "disabled".into(),
            cache: "empty".into(),
        });
        let report = preflight(&recipe, &dir).unwrap();
        assert_eq!(report.integrity, "damaged");
        assert_eq!(report.offline_readiness, "stale");
    }

    #[test]
    fn incompatible_platform_and_unresolved_rights_stay_distinct() {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path().join("model");
        let files = text_files();
        write_files(&dir, &files);
        let mut recipe = recipe(files, &["text-generation"]);
        recipe.runtime.os = if host_os() == "windows" {
            "linux"
        } else {
            "windows"
        }
        .into();
        recipe.rights.status = RightsStatus::Unresolved;
        let report = preflight(&recipe, &dir).unwrap();
        assert_eq!(report.environment, "incompatible");
        assert_eq!(report.rights, "unresolved");
        assert_eq!(report.offline_readiness, "untested");
        assert!(report.next.contains("operating system") || report.next.contains("license"));
    }

    #[test]
    fn unavailable_directory_is_reported_and_not_created() {
        let temp = tempfile::TempDir::new().unwrap();
        let missing = temp.path().join("absent");
        let report = preflight(&recipe(text_files(), &["text-generation"]), &missing).unwrap();
        assert_eq!(report.availability, "unavailable");
        assert_eq!(report.integrity, "unavailable");
        assert_eq!(report.capabilities[0].closure, "missing");
        assert!(!missing.exists());
    }

    #[test]
    fn unknown_schema_and_remote_or_named_artifacts_do_not_count_as_complete() {
        let mut broken = recipe(text_files(), &["text-generation"]);
        broken.schema_version = 2;
        let text = serde_json::to_string(&broken).unwrap();
        assert_eq!(Recipe::parse(&text).unwrap_err().code, "unsupported_schema");
        let mut recipe = recipe(text_files(), &["text-generation"]);
        recipe.model.files[3].retention = Retention::Remote;
        recipe.model.files[3].size_bytes = None;
        recipe.model.files[3].sha256 = None;
        recipe.model.files[3].locator = Some("https://example.invalid/template".into());
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path().join("model");
        write_files(&dir, &recipe.model.files);
        let report = preflight(&recipe, &dir).unwrap();
        assert_eq!(report.capabilities[0].closure, "missing");
        assert!(
            report.capabilities[0]
                .blocked_by
                .iter()
                .any(|item| item.contains("remote"))
        );
        assert_eq!(report.runtime_retention, "named");
    }

    #[test]
    fn a_matching_recorded_drill_applies_only_to_retained_bytes() {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path().join("model");
        let mut files = text_files();
        write_files(&dir, &files);
        fs::write(dir.join("runtime.bin"), b"runtime").unwrap();
        let mut recipe = recipe(files.split_off(0), &["text-generation"]);
        recipe.runtime.artifact = ArtifactDeclaration {
            retention: Retention::Retained,
            locator: "runtime.bin".into(),
            size_bytes: Some(b"runtime".len() as u64),
            sha256: Some(sha(b"runtime")),
        };
        let digest = recipe.digest();
        let untested = preflight(&recipe, &dir).unwrap();
        assert_eq!(untested.offline_readiness, "untested");
        assert!(untested.next.contains("drill"));
        recipe.recorded_drill = Some(DrillRecord {
            schema_version: 1,
            recipe_digest: "ab".repeat(32),
            result: DrillResult::TaskPassed,
            capability: "text-generation".into(),
            os: host_os().into(),
            arch: host_arch().into(),
            runtime_version: recipe.runtime.version.clone(),
            network: "disabled".into(),
            cache: "empty".into(),
        });
        let stale = preflight(&recipe, &dir).unwrap();
        assert_eq!(stale.offline_readiness, "stale");
        recipe.recorded_drill.as_mut().unwrap().recipe_digest = digest;
        let report = preflight(&recipe, &dir).unwrap();
        assert_eq!(report.runtime_retention, "retained");
        assert_eq!(report.integrity, "matched");
        assert_eq!(report.offline_readiness, "task_passed");
        assert!(report.next.contains("did not run the model"));
    }

    #[test]
    fn a_large_retained_file_is_counted_and_not_hashed() {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path().join("model");
        let mut files = text_files();
        write_files(&dir, &files);
        let large = HASH_LIMIT + 1;
        files[2].size_bytes = Some(large);
        files[2].sha256 = Some("ab".repeat(32));
        fs::File::create(dir.join("model.safetensors"))
            .unwrap()
            .set_len(large)
            .unwrap();
        let report = preflight(&recipe(files, &["text-generation"]), &dir).unwrap();
        assert_eq!(report.unverified_files, 1);
        assert_eq!(report.hashed_files, 3);
        assert_eq!(report.integrity, "not_checked");
        assert_eq!(report.capabilities[0].closure, "present_unverified");
        assert_eq!(report.offline_readiness, "untested");
    }

    #[test]
    fn a_directory_link_is_not_followed() {
        let temp = tempfile::TempDir::new().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("link");
        fs::create_dir(&target).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        #[cfg(windows)]
        {
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(&target)
                .status()
                .unwrap();
            assert!(status.success());
        }
        assert_eq!(
            preflight(&recipe(text_files(), &["text-generation"]), &link)
                .unwrap_err()
                .code,
            "unsafe_path"
        );
        assert!(fs::read_dir(&target).unwrap().next().is_none());
    }

    fn reject(edit: impl FnOnce(&mut Recipe), code: &str, fragment: &str) {
        let mut recipe = recipe(text_files(), &["text-generation"]);
        edit(&mut recipe);
        let error = Recipe::parse(&serde_json::to_string(&recipe).unwrap()).unwrap_err();
        assert_eq!(error.code, code, "{}", error.message);
        assert!(
            error.message.contains(fragment),
            "{} missing from {}",
            fragment,
            error.message
        );
    }

    fn as_remote(recipe: &mut Recipe, locator: &str) {
        let file = &mut recipe.model.files[3];
        file.retention = Retention::Remote;
        file.size_bytes = None;
        file.sha256 = None;
        file.locator = Some(locator.into());
    }

    fn retained_runtime(recipe: &mut Recipe) {
        recipe.runtime.artifact = ArtifactDeclaration {
            retention: Retention::Retained,
            locator: "runtime.bin".into(),
            size_bytes: Some(b"runtime".len() as u64),
            sha256: Some(sha(b"runtime")),
        };
    }

    fn other_than(current: &str, left: &str, right: &str) -> String {
        if current == left { right } else { left }.into()
    }

    fn matching_drill(recipe: &Recipe, result: DrillResult) -> DrillRecord {
        DrillRecord {
            schema_version: 1,
            recipe_digest: recipe.digest(),
            result,
            capability: recipe.capabilities[0].clone(),
            os: host_os().into(),
            arch: host_arch().into(),
            runtime_version: recipe.runtime.version.clone(),
            network: "disabled".into(),
            cache: "empty".into(),
        }
    }

    fn prepared_runtime() -> (tempfile::TempDir, std::path::PathBuf, Recipe) {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path().join("model");
        let files = text_files();
        write_files(&dir, &files);
        fs::write(dir.join("runtime.bin"), b"runtime").unwrap();
        let mut recipe = recipe(files, &["text-generation"]);
        retained_runtime(&mut recipe);
        (temp, dir, recipe)
    }

    #[test]
    fn invalid_recipes_name_the_failing_rule() {
        assert_eq!(Recipe::parse("{").unwrap_err().code, "invalid_recipe");
        assert_eq!(
            Recipe::parse(r#"{"schema_version":1,"unexpected":true}"#)
                .unwrap_err()
                .code,
            "invalid_recipe"
        );
        reject(
            |recipe| recipe.profile = "other".into(),
            "unsupported_profile",
            "publisher-safetensors-text-v1",
        );
        reject(
            |recipe| recipe.model.endpoint = "https://example.com".into(),
            "unsupported_source",
            "Hugging Face",
        );
        reject(
            |recipe| recipe.model.repo_id = "nope".into(),
            "invalid_repo",
            "owner/model",
        );
        reject(
            |recipe| recipe.model.resolved_revision = "abc".into(),
            "invalid_hash",
            "40",
        );
        reject(
            |recipe| recipe.capabilities.clear(),
            "invalid_recipe",
            "unique",
        );
        reject(
            |recipe| recipe.capabilities = vec![String::new()],
            "invalid_recipe",
            "unique",
        );
        reject(
            |recipe| recipe.capabilities.push("text-generation".into()),
            "invalid_recipe",
            "unique",
        );
        reject(
            |recipe| recipe.model.files.clear(),
            "invalid_recipe",
            "names its files",
        );
        reject(
            |recipe| recipe.model.files.push(recipe.model.files[0].clone()),
            "path_collision",
            "duplicate",
        );
        reject(
            |recipe| recipe.model.files[0].path = "..".into(),
            "unsafe_path",
            "unsafe",
        );
        reject(
            |recipe| recipe.model.files[0].capability = "other".into(),
            "invalid_recipe",
            "selected",
        );
        reject(
            |recipe| recipe.model.files[0].sha256 = None,
            "invalid_recipe",
            "sha256",
        );
        reject(
            |recipe| recipe.model.files[0].size_bytes = None,
            "invalid_recipe",
            "size",
        );
        reject(
            |recipe| recipe.model.files[0].sha256 = Some("zz".repeat(32)),
            "invalid_hash",
            "64",
        );
        reject(
            |recipe| recipe.model.files[0].locator = Some("..".into()),
            "unsafe_path",
            "unsafe",
        );
        reject(
            |recipe| {
                as_remote(recipe, "https://example.com/template");
                recipe.model.files[3].retention = Retention::Named;
                recipe.model.files[3].locator = Some("https://example.com/template".into());
            },
            "invalid_recipe",
            "package name",
        );
        reject(
            |recipe| {
                as_remote(recipe, "ignored");
                recipe.model.files[3].retention = Retention::Named;
                recipe.model.files[3].locator = Some("bad name".into());
            },
            "invalid_recipe",
            "package name",
        );
        for locator in [
            "not a url",
            "http://example.com/template",
            "https://user@example.com/template",
            "https://:secret@example.com/template",
            "https://example.com/template?x=1",
            "https://example.com/template#frag",
        ] {
            let mut recipe = recipe(text_files(), &["text-generation"]);
            as_remote(&mut recipe, locator);
            let error = Recipe::parse(&serde_json::to_string(&recipe).unwrap()).unwrap_err();
            assert_eq!(error.code, "invalid_recipe", "{locator}: {}", error.message);
            assert!(error.message.contains("https"), "{}", error.message);
            assert!(!error.message.contains("secret"), "{}", error.message);
        }
        reject(
            |recipe| recipe.runtime.artifact.locator.clear(),
            "invalid_recipe",
            "ASCII",
        );
        reject(
            |recipe| recipe.runtime.artifact.locator = "a".repeat(241),
            "invalid_recipe",
            "ASCII",
        );
        reject(
            |recipe| recipe.runtime.artifact.locator = "rä".into(),
            "invalid_recipe",
            "ASCII",
        );
        reject(
            |recipe| {
                recipe.model.files[0].base = Some(BasePin {
                    repo_id: "nope".into(),
                    resolved_revision: None,
                });
            },
            "invalid_repo",
            "owner/model",
        );
        reject(
            |recipe| {
                recipe.model.files[0].base = Some(BasePin {
                    repo_id: "example/base".into(),
                    resolved_revision: Some("abc".into()),
                });
            },
            "invalid_hash",
            "40",
        );
        reject(
            |recipe| {
                recipe.capabilities.push("embeddings".into());
                recipe.model.files.push(file(
                    "vectors.bin",
                    Role::Other,
                    "embeddings",
                    Need::Optional,
                    Retention::Named,
                    None,
                ));
            },
            "invalid_recipe",
            "embeddings",
        );
        reject(
            |recipe| recipe.model.files[3].need = Need::Optional,
            "invalid_recipe",
            "text-generation requires",
        );
        reject(
            |recipe| recipe.runtime.os = "freebsd".into(),
            "invalid_recipe",
            "os must",
        );
        reject(
            |recipe| recipe.runtime.arch = "wasm32".into(),
            "invalid_recipe",
            "arch must",
        );
        reject(
            |recipe| recipe.runtime.name.clear(),
            "invalid_recipe",
            "runtime name",
        );
        reject(
            |recipe| recipe.runtime.version.clear(),
            "invalid_recipe",
            "runtime name",
        );
        reject(
            |recipe| {
                recipe.runtime.artifact = ArtifactDeclaration {
                    retention: Retention::Retained,
                    locator: "config.json".into(),
                    size_bytes: Some(2),
                    sha256: Some(sha(b"{}")),
                };
            },
            "invalid_recipe",
            "not a model file",
        );
        reject(
            |recipe| recipe.rights.license_id.clear(),
            "invalid_recipe",
            "license id",
        );
        reject(
            |recipe| {
                let mut drill = matching_drill(recipe, DrillResult::Failed);
                drill.schema_version = 2;
                recipe.recorded_drill = Some(drill);
            },
            "unsupported_schema",
            "drill",
        );
        reject(
            |recipe| {
                let mut drill = matching_drill(recipe, DrillResult::Failed);
                drill.recipe_digest = "ab".into();
                recipe.recorded_drill = Some(drill);
            },
            "invalid_hash",
            "64",
        );
        reject(
            |recipe| {
                let mut drill = matching_drill(recipe, DrillResult::Failed);
                drill.os = "freebsd".into();
                recipe.recorded_drill = Some(drill);
            },
            "invalid_recipe",
            "os must",
        );
        reject(
            |recipe| {
                let mut drill = matching_drill(recipe, DrillResult::Failed);
                drill.capability = "other".into();
                recipe.recorded_drill = Some(drill);
            },
            "invalid_recipe",
            "selected capability",
        );
        reject(
            |recipe| {
                let mut drill = matching_drill(recipe, DrillResult::Failed);
                drill.runtime_version = "0".into();
                recipe.recorded_drill = Some(drill);
            },
            "invalid_recipe",
            "runtime version",
        );
        reject(
            |recipe| {
                let mut drill = matching_drill(recipe, DrillResult::Failed);
                drill.network = "open".into();
                recipe.recorded_drill = Some(drill);
            },
            "invalid_recipe",
            "network disabled",
        );
        reject(
            |recipe| {
                let mut drill = matching_drill(recipe, DrillResult::Failed);
                drill.cache = "warm".into();
                recipe.recorded_drill = Some(drill);
            },
            "invalid_recipe",
            "cache empty",
        );
        assert_eq!(retention_reason(Retention::Retained), "retained");
        assert_eq!(retention_reason(Retention::Named), "named");
        assert_eq!(retention_reason(Retention::Remote), "remote");
    }

    #[test]
    fn a_recipe_may_declare_a_base_a_drill_and_a_remote_template() {
        let mut declared = recipe(text_files(), &["text-generation"]);
        declared.model.files[0].base = Some(BasePin {
            repo_id: "example/base".into(),
            resolved_revision: Some(COMMIT.into()),
        });
        declared.model.files[1].base = Some(BasePin {
            repo_id: "example/base".into(),
            resolved_revision: None,
        });
        as_remote(&mut declared, "https://example.com/template");
        retained_runtime(&mut declared);
        declared.recorded_drill = Some(matching_drill(&declared, DrillResult::LoadPassed));
        let parsed = Recipe::parse(&serde_json::to_string(&declared).unwrap()).unwrap();
        assert_eq!(parsed.digest(), declared.digest());

        let embeddings = recipe(
            vec![file(
                "model.safetensors",
                Role::Weight,
                "embeddings",
                Need::Required,
                Retention::Named,
                None,
            )],
            &["embeddings"],
        );
        assert!(Recipe::parse(&serde_json::to_string(&embeddings).unwrap()).is_ok());

        let mut reversed = declared.clone();
        reversed.model.files.reverse();
        reversed.capabilities.reverse();
        reversed.recorded_drill = None;
        let mut forward = declared.clone();
        forward.recorded_drill = None;
        assert_eq!(forward.digest(), reversed.digest());
    }

    #[test]
    fn preflight_distinguishes_each_retained_file_failure() {
        let (_temp, dir, mut declared) = prepared_runtime();
        declared.model.files.push(file(
            "README.md",
            Role::Documentation,
            "text-generation",
            Need::Optional,
            Retention::Retained,
            Some(b"note"),
        ));
        declared.model.files.push(file(
            "notes.md",
            Role::Documentation,
            "text-generation",
            Need::Unknown,
            Retention::Retained,
            Some(b"note"),
        ));
        let unknown = preflight(&declared, &dir).unwrap();
        assert_eq!(unknown.capabilities[0].closure, "unknown");
        assert!(
            unknown.capabilities[0]
                .excluded
                .iter()
                .any(|item| item == "README.md")
        );
        assert!(unknown.next.contains("unknown"));

        let mut damaged = declared.clone();
        damaged.model.files.pop();
        fs::write(dir.join("README.md"), b"NOTE").unwrap();
        let damaged = preflight(&damaged, &dir).unwrap();
        assert_eq!(damaged.capabilities[0].closure, "complete");
        assert_eq!(damaged.integrity, "damaged");
        assert!(damaged.next.contains("does not match"));

        let mut files = text_files();
        for file in &mut files {
            file.retention = Retention::Named;
            file.size_bytes = None;
            file.sha256 = None;
        }
        let named = preflight(&recipe(files, &["text-generation"]), &dir).unwrap();
        assert_eq!(named.hashed_files, 0);
        assert_eq!(named.integrity, "not_checked");
        assert_eq!(named.capabilities[0].closure, "missing");
        assert!(
            named.capabilities[0]
                .blocked_by
                .iter()
                .any(|item| item.contains("named"))
        );

        fs::write(dir.join("config.json"), b"x").unwrap();
        let mut sized = declared.clone();
        sized.model.files.retain(|file| file.need != Need::Unknown);
        let sized = preflight(&sized, &dir).unwrap();
        assert!(
            sized.capabilities[0]
                .blocked_by
                .iter()
                .any(|item| item.contains("size_mismatch"))
        );

        fs::remove_file(dir.join("tokenizer.json")).unwrap();
        fs::create_dir(dir.join("tokenizer.json")).unwrap();
        let directory = preflight(&retained_copy(), &dir).unwrap();
        assert!(
            directory.capabilities[0]
                .blocked_by
                .iter()
                .any(|item| item.contains("not_a_file"))
        );

        let temp = tempfile::TempDir::new().unwrap();
        let linked = temp.path().join("model");
        write_files(&linked, &text_files());
        fs::write(linked.join("runtime.bin"), b"runtime").unwrap();
        fs::remove_file(linked.join("config.json")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(linked.join("tokenizer.json"), linked.join("config.json"))
            .unwrap();
        #[cfg(windows)]
        {
            let target = temp.path().join("elsewhere");
            fs::create_dir(&target).unwrap();
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(linked.join("config.json"))
                .arg(&target)
                .status()
                .unwrap();
            assert!(status.success());
        }
        let unsafe_link = preflight(&retained_copy(), &linked).unwrap();
        assert!(
            unsafe_link.capabilities[0]
                .blocked_by
                .iter()
                .any(|item| item.contains("unsafe_link"))
        );
    }

    fn retained_copy() -> Recipe {
        let mut recipe = recipe(text_files(), &["text-generation"]);
        retained_runtime(&mut recipe);
        recipe
    }

    #[test]
    fn a_missing_or_unreadable_directory_is_not_created() {
        let temp = tempfile::TempDir::new().unwrap();
        let missing = temp.path().join("absent");
        let mut files = text_files();
        files.push(file(
            "README.md",
            Role::Documentation,
            "text-generation",
            Need::Optional,
            Retention::Retained,
            Some(b"note"),
        ));
        files.push(file(
            "remote.bin",
            Role::Weight,
            "text-generation",
            Need::Required,
            Retention::Remote,
            None,
        ));
        files.last_mut().unwrap().locator = Some("https://example.com/remote.bin".into());
        let report = preflight(&recipe(files, &["text-generation"]), &missing).unwrap();
        assert!(
            report.capabilities[0]
                .excluded
                .iter()
                .any(|item| item == "README.md")
        );
        assert!(
            report.capabilities[0]
                .blocked_by
                .iter()
                .any(|item| item.contains("remote"))
        );
        assert!(!missing.exists());

        let file_path = temp.path().join("file");
        fs::write(&file_path, b"x").unwrap();
        assert_eq!(
            preflight(&recipe(text_files(), &["text-generation"]), &file_path)
                .unwrap_err()
                .code,
            "unsafe_path"
        );
        let nested_path = file_path.join("child");
        let nested = preflight(&recipe(text_files(), &["text-generation"]), &nested_path);
        #[cfg(unix)]
        {
            let error = nested.unwrap_err();
            assert_eq!(error.code, "input_read_failed", "{}", error.message);
        }
        #[cfg(windows)]
        {
            let report = nested.unwrap();
            assert_eq!(report.availability, "unavailable");
            assert!(!nested_path.exists());
        }
    }

    #[test]
    fn runtime_retention_and_platform_choose_the_next_step() {
        let (_temp, dir, recipe) = prepared_runtime();
        fs::remove_file(dir.join("runtime.bin")).unwrap();
        let missing = preflight(&recipe, &dir).unwrap();
        assert_eq!(missing.runtime_retention, "missing");
        assert_eq!(missing.capabilities[0].closure, "complete");
        assert!(missing.next.contains("missing"));

        let mut remote = recipe.clone();
        remote.runtime.artifact = ArtifactDeclaration {
            retention: Retention::Remote,
            locator: "https://example.com/runtime".into(),
            size_bytes: None,
            sha256: None,
        };
        fs::write(dir.join("runtime.bin"), b"runtime").unwrap();
        let remote = preflight(&remote, &dir).unwrap();
        assert_eq!(remote.runtime_retention, "remote");
        assert!(remote.next.contains("remote") || remote.next.contains("lockfile"));

        let mut large = recipe.clone();
        retained_runtime(&mut large);
        let optional = HASH_LIMIT + 1;
        large.model.files.push(file(
            "extra.bin",
            Role::Other,
            "text-generation",
            Need::Optional,
            Retention::Retained,
            Some(b"extra"),
        ));
        large.model.files.last_mut().unwrap().size_bytes = Some(optional);
        fs::File::create(dir.join("extra.bin"))
            .unwrap()
            .set_len(optional)
            .unwrap();
        let large = preflight(&large, &dir).unwrap();
        assert_eq!(large.capabilities[0].closure, "complete");
        assert_eq!(large.integrity, "not_checked");
        assert!(large.next.contains("not hashed"));

        let mut host = recipe.clone();
        host.runtime.os = host_os().into();
        host.runtime.arch = "any".into();
        assert_eq!(preflight(&host, &dir).unwrap().environment, "compatible");
        host.runtime.os = "any".into();
        host.runtime.arch = host_arch().into();
        assert_eq!(preflight(&host, &dir).unwrap().environment, "compatible");
        host.runtime.arch = other_than(host_arch(), "x86_64", "aarch64");
        let incompatible = preflight(&host, &dir).unwrap();
        assert_eq!(incompatible.environment, "incompatible");
        assert!(
            incompatible.next.contains("architecture")
                || incompatible.next.contains("operating system")
        );
    }

    #[test]
    fn a_present_runtime_is_not_reported_missing() {
        let (_temp, dir, mut recipe) = prepared_runtime();
        let large = HASH_LIMIT + 1;
        recipe.runtime.artifact.size_bytes = Some(large);
        recipe.runtime.artifact.sha256 = Some("ab".repeat(32));
        fs::File::create(dir.join("runtime.bin"))
            .unwrap()
            .set_len(large)
            .unwrap();
        let oversized = preflight(&recipe, &dir).unwrap();
        assert_eq!(oversized.runtime_retention, "unverified");
        assert_eq!(oversized.integrity, "not_checked");
        assert_eq!(oversized.unverified_files, 1);
        assert_eq!(oversized.capabilities[0].closure, "complete");
        assert!(oversized.next.contains("not hashed"), "{}", oversized.next);
        assert!(!oversized.next.contains("missing"), "{}", oversized.next);
        recipe.recorded_drill = Some(matching_drill(&recipe, DrillResult::TaskPassed));
        let drilled = preflight(&recipe, &dir).unwrap();
        assert_eq!(drilled.runtime_retention, "unverified");
        assert_eq!(drilled.offline_readiness, "stale");

        let (_temp, dir, recipe) = prepared_runtime();
        fs::write(dir.join("runtime.bin"), b"changed").unwrap();
        let damaged = preflight(&recipe, &dir).unwrap();
        assert_eq!(damaged.runtime_retention, "damaged");
        assert_eq!(damaged.integrity, "damaged");
        assert!(damaged.next.contains("does not match"), "{}", damaged.next);
        assert!(!damaged.next.contains("missing"), "{}", damaged.next);
    }

    #[test]
    fn a_recorded_drill_applies_only_when_every_condition_matches() {
        let (_temp, dir, mut recipe) = prepared_runtime();
        let expect = |recipe: &Recipe, readiness: &str, next: &str| {
            let report = preflight(recipe, &dir).unwrap();
            assert_eq!(report.offline_readiness, readiness, "{}", report.next);
            assert!(report.next.contains(next), "{}", report.next);
            report
        };

        recipe.recorded_drill = Some(matching_drill(&recipe, DrillResult::Failed));
        let failed = expect(&recipe, "failed", "did not run");
        assert!(crate::present::render_readiness(&failed, true, 40).contains("failed"));

        recipe.recorded_drill.as_mut().unwrap().result = DrillResult::LoadPassed;
        let loaded = expect(&recipe, "load_passed", "did not run");
        assert!(crate::present::render_readiness(&loaded, true, 80).contains("load_passed"));
        assert!(crate::present::render_readiness(&loaded, false, 80).contains("load_passed"));

        let mut stale = recipe.clone();
        stale.recorded_drill.as_mut().unwrap().os = other_than(host_os(), "windows", "linux");
        expect(&stale, "stale", "does not apply");
        stale.recorded_drill.as_mut().unwrap().os = host_os().into();
        stale.recorded_drill.as_mut().unwrap().arch = other_than(host_arch(), "x86_64", "aarch64");
        expect(&stale, "stale", "does not apply");
        stale.recorded_drill.as_mut().unwrap().arch = host_arch().into();
        stale.recorded_drill.as_mut().unwrap().network = "enabled".into();
        expect(&stale, "stale", "does not apply");
        stale.recorded_drill.as_mut().unwrap().network = "disabled".into();
        stale.recorded_drill.as_mut().unwrap().cache = "warm".into();
        expect(&stale, "stale", "does not apply");

        let mut rights = recipe.clone();
        rights.rights.status = RightsStatus::Unreviewed;
        rights.recorded_drill = Some(matching_drill(&rights, DrillResult::TaskPassed));
        let rights = expect(&rights, "stale", "license");
        assert_eq!(rights.rights, "unreviewed");

        let mut platform = recipe.clone();
        platform.runtime.os = other_than(host_os(), "windows", "linux");
        platform.recorded_drill = Some(matching_drill(&platform, DrillResult::TaskPassed));
        let platform = expect(&platform, "stale", "operating system");
        assert_eq!(platform.environment, "incompatible");

        let mut named = recipe.clone();
        named.runtime.artifact.retention = Retention::Named;
        named.runtime.artifact.locator = "transformers==4.55.2".into();
        named.runtime.artifact.size_bytes = None;
        named.runtime.artifact.sha256 = None;
        named.recorded_drill = Some(matching_drill(&named, DrillResult::TaskPassed));
        expect(&named, "stale", "named");

        let mut other = recipe.clone();
        other.recorded_drill.as_mut().unwrap().capability = "other".into();
        expect(&other, "stale", "does not apply");
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_retained_file_is_an_input_failure() {
        use std::os::unix::fs::PermissionsExt;
        let (_temp, dir, recipe) = prepared_runtime();
        let path = dir.join("config.json");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let opened = preflight(&recipe, &dir);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let error = opened.unwrap_err();
        assert_eq!(error.code, "input_read_failed", "{}", error.message);

        let locked = dir.join("locked");
        fs::create_dir(&locked).unwrap();
        fs::write(locked.join("hidden.bin"), b"hidden").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let mut hidden = recipe.clone();
        hidden.model.files.push(file(
            "locked/hidden.bin",
            Role::Other,
            "text-generation",
            Need::Optional,
            Retention::Retained,
            Some(b"hidden"),
        ));
        let opened = preflight(&hidden, &dir);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        let error = opened.unwrap_err();
        assert_eq!(error.code, "input_read_failed", "{}", error.message);
    }
}
