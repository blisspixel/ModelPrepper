//! Read-only inventory of local runtime stores. Never pulls, deletes, or writes.
use crate::{Error, Result};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

const QUANTIZATION: &[&str] = &[
    "q8_0", "q6_k", "q5_k_m", "q5_k_s", "q5_1", "q5_0", "q4_k_m", "q4_k_s", "q4_1", "q4_0",
    "q3_k_l", "q3_k_m", "q3_k_s", "q2_k", "iq4_xs", "iq3_xxs", "fp16", "f16", "f32",
];

#[derive(Debug, Serialize)]
pub struct RuntimeInventory {
    pub schema_version: u32,
    pub summary: String,
    pub message: String,
    /// What a catalog match does and does not prove.
    pub note: String,
    pub models: Vec<RuntimeModel>,
}

#[derive(Debug, Serialize)]
pub struct RuntimeComponent {
    pub role: String,
    pub media_type: String,
    pub digest: String,
    /// None when no catalog was requested. Never means "hashed just now".
    pub recorded: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct RuntimeModel {
    pub name: String,
    pub store: PathBuf,
    pub size_bytes: u64,
    pub digests: Vec<String>,
    pub format: String,
    pub quantization: Option<String>,
    pub license_layer: bool,
    pub template_layer: bool,
    /// Null when no catalog was compared. A true value is historical byte identity only.
    pub preserved: Option<bool>,
    pub coverage: String,
    pub matched_digests: u32,
    pub total_digests: u32,
    pub missing_roles: Vec<String>,
    pub components: Vec<RuntimeComponent>,
}

impl RuntimeInventory {
    pub fn text(&self) -> String {
        let verdict = if self.models.is_empty() {
            "empty"
        } else if self
            .models
            .iter()
            .all(|model| model.coverage == "not_checked")
        {
            "unchecked"
        } else if self
            .models
            .iter()
            .all(|model| model.coverage == "all_components_recorded")
        {
            "recorded"
        } else if self
            .models
            .iter()
            .all(|model| model.coverage == "none_recorded")
        {
            "none"
        } else {
            "partial"
        };
        let mut lines = vec![format!("{verdict}  {}", self.summary)];
        if !self.message.is_empty() {
            lines.push(format!("  {}", self.message));
        }
        if !self.note.is_empty() {
            lines.push(format!("  {}", self.note));
        }
        for model in &self.models {
            let quant = model.quantization.as_deref().unwrap_or("unknown");
            let license = if model.license_layer {
                "license"
            } else {
                "no license layer"
            };
            let missing = if model.missing_roles.is_empty() {
                String::new()
            } else {
                format!("  missing {}", model.missing_roles.join(", "))
            };
            lines.push(format!(
                "  {name}  {coverage}  {matched}/{total}  {format}/{quant}  {license}{missing}",
                name = model.name,
                coverage = coverage_word(&model.coverage),
                matched = model.matched_digests,
                total = model.total_digests,
                format = model.format,
            ));
        }
        let next = if self
            .models
            .iter()
            .any(|model| model.coverage == "not_checked")
        {
            "modelprepper local list --config <file>"
        } else if self.models.iter().any(|model| {
            matches!(
                model.coverage.as_str(),
                "partially_recorded" | "none_recorded"
            )
        }) {
            "missing components are not in the catalog; this is not an Ollama import"
        } else {
            "modelprepper status --config <file>"
        };
        lines.push(format!("next: {next}"));
        lines.join("\n")
    }
}

fn coverage_word(coverage: &str) -> &str {
    match coverage {
        "all_components_recorded" => "recorded",
        "partially_recorded" => "partial",
        "none_recorded" => "none recorded",
        "not_checked" => "not checked",
        other => other,
    }
}

pub fn default_store_dirs(env: impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(value) = env("OLLAMA_MODELS").filter(|value| !value.is_empty()) {
        dirs.push(PathBuf::from(value));
    }
    if let Some(home) = env("HOME").or_else(|| env("USERPROFILE")) {
        dirs.push(PathBuf::from(home).join(".ollama").join("models"));
    }
    dirs.push(PathBuf::from("/usr/share/ollama/.ollama/models"));
    dirs
}

/// Parse Ollama manifests only. Blob files are not opened or modified.
/// `preserved` is None when the caller did not ask for a catalog comparison.
pub fn inventory(
    dirs: &[PathBuf],
    preserved: Option<&BTreeSet<String>>,
) -> Result<RuntimeInventory> {
    let mut models = Vec::new();
    let mut notes = Vec::new();
    let mut seen = BTreeSet::new();
    if dirs.is_empty() {
        notes.push(
            "no Ollama model store was configured; looked for OLLAMA_MODELS, ~/.ollama/models, and /usr/share/ollama/.ollama/models"
                .to_owned(),
        );
    }
    for dir in dirs {
        let key = fs::canonicalize(dir).unwrap_or_else(|_| dir.clone());
        if !seen.insert(key) {
            continue;
        }
        match read_store(dir, preserved) {
            Ok((found, note)) => {
                if let Some(note) = note {
                    notes.push(note);
                }
                models.extend(found);
            }
            Err(error) => notes.push(format!("{}: {}", dir.display(), error.message)),
        }
    }
    models.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.store.cmp(&right.store))
    });
    let recorded = models
        .iter()
        .filter(|model| model.coverage == "all_components_recorded")
        .count();
    let partial = models
        .iter()
        .filter(|model| model.coverage == "partially_recorded")
        .count();
    let unchecked = models
        .iter()
        .filter(|model| model.coverage == "not_checked")
        .count();
    let unlicensed = models.iter().filter(|model| !model.license_layer).count();
    let note = if preserved.is_none() {
        "No vault was compared. Pass --config to compare digests. Nothing was hashed."
    } else {
        "Catalog coverage is recorded byte identity, not a fresh hash, a mounted disk, or proof the model runs offline."
    };
    Ok(RuntimeInventory {
        schema_version: 2,
        summary: format!(
            "{} local models, {recorded} fully recorded, {partial} partial, {unchecked} not checked, {unlicensed} without a license layer",
            models.len()
        ),
        message: notes.join("; "),
        note: note.to_owned(),
        models,
    })
}

fn read_store(
    dir: &Path,
    preserved: Option<&BTreeSet<String>>,
) -> Result<(Vec<RuntimeModel>, Option<String>)> {
    let meta = match fs::symlink_metadata(dir) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((
                Vec::new(),
                Some(format!(
                    "Ollama store not found at {}; nothing was changed",
                    dir.display()
                )),
            ));
        }
        Err(error) => {
            return Ok((
                Vec::new(),
                Some(format!(
                    "could not read Ollama store at {}: {error}",
                    dir.display()
                )),
            ));
        }
    };
    if !meta.is_dir() {
        return Ok((
            Vec::new(),
            Some(format!(
                "{} is not an Ollama models directory",
                dir.display()
            )),
        ));
    }
    let manifests = dir.join("manifests");
    if !manifests.exists() {
        return Ok((
            Vec::new(),
            Some(format!(
                "{} has no manifests directory; blobs were not opened",
                dir.display()
            )),
        ));
    }
    let mut models = Vec::new();
    let mut notes = Vec::new();
    let mut paths = Vec::new();
    collect_files(&manifests, &mut paths, &mut notes)?;
    paths.sort();
    for path in paths {
        match read_manifest(&path) {
            Ok(mut model) => {
                model.store = dir.to_path_buf();
                apply_coverage(&mut model, preserved);
                models.push(model);
            }
            Err(error) => notes.push(format!(
                "skipped unreadable manifest {}: {}",
                path.display(),
                error.message
            )),
        }
    }
    Ok((
        models,
        if notes.is_empty() {
            None
        } else {
            Some(notes.join("; "))
        },
    ))
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>, notes: &mut Vec<String>) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            notes.push(format!("could not read {}: {error}", dir.display()));
            return Ok(());
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                notes.push(format!(
                    "could not read an entry in {}: {error}",
                    dir.display()
                ));
                continue;
            }
        };
        let path = entry.path();
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(error) => {
                notes.push(format!("could not read {}: {error}", path.display()));
                continue;
            }
        };
        if meta.file_type().is_symlink() {
            notes.push(format!("skipped symlink {}", path.display()));
        } else if meta.is_dir() {
            collect_files(&path, files, notes)?;
        } else if meta.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn read_manifest(path: &Path) -> Result<RuntimeModel> {
    let file = File::open(path)
        .map_err(|error| Error::new("input_read_failed", format!("{}: {error}", path.display())))?;
    let mut bytes = Vec::new();
    file.take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Error::new("input_read_failed", error.to_string()))?;
    if bytes.len() as u64 > 2 * 1024 * 1024 {
        return Err(Error::new(
            "metadata_too_large",
            "Ollama manifest exceeds 2 MiB",
        ));
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| Error::new("invalid_manifest", "Ollama manifest is not JSON"))?;
    let config = value.get("config").cloned().unwrap_or(Value::Null);
    let layers = value
        .get("layers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut digests = Vec::new();
    let mut components = Vec::new();
    let mut size = 0_u64;
    let mut media = Vec::new();
    for (index, layer) in std::iter::once(&config).chain(layers.iter()).enumerate() {
        if layer.is_null() {
            continue;
        }
        let digest = layer
            .get("digest")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if !digest.is_empty() {
            digests.push(digest.clone());
        }
        let layer_size = layer.get("size").and_then(Value::as_u64).unwrap_or(0);
        size = size
            .checked_add(layer_size)
            .ok_or_else(|| Error::new("size_overflow", "Ollama manifest sizes overflow u64"))?;
        let kind = layer
            .get("mediaType")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if !kind.is_empty() {
            media.push(kind.to_ascii_lowercase());
        }
        components.push(RuntimeComponent {
            role: component_role(index == 0, &kind),
            media_type: kind,
            digest,
            recorded: None,
        });
    }
    let name = model_name(path);
    Ok(RuntimeModel {
        quantization: quantization(&name),
        format: detect_format(&media),
        license_layer: media.iter().any(|kind| kind.contains("license")),
        template_layer: media.iter().any(|kind| kind.contains("template")),
        name,
        store: PathBuf::new(),
        size_bytes: size,
        digests,
        preserved: None,
        coverage: "not_checked".into(),
        matched_digests: 0,
        total_digests: 0,
        missing_roles: Vec::new(),
        components,
    })
}

fn component_role(config_blob: bool, media: &str) -> String {
    let kind = media.to_ascii_lowercase();
    if kind.is_empty() {
        return if config_blob {
            "config".into()
        } else {
            "unknown".into()
        };
    }
    if kind.contains("license") {
        "license".into()
    } else if kind.contains("template") {
        "template".into()
    } else if kind.contains("system") {
        "system".into()
    } else if kind.contains("params") || kind.contains("parameter") {
        "parameters".into()
    } else if kind.contains("model") || kind.contains("gguf") || kind.contains("safetensors") {
        "weights".into()
    } else if config_blob || kind.contains("config") {
        "config".into()
    } else {
        "other".into()
    }
}

fn apply_coverage(model: &mut RuntimeModel, preserved: Option<&BTreeSet<String>>) {
    let mut valid = BTreeSet::new();
    let mut malformed = 0_u32;
    for component in &mut model.components {
        let hex = digest_hex(&component.digest);
        let usable =
            hex.is_some() && !component.media_type.is_empty() && component.role != "unknown";
        if let Some(hex) = hex.filter(|_| usable) {
            valid.insert(hex);
        } else {
            malformed += 1;
        }
        component.recorded = preserved.map(|set| {
            usable && digest_hex(&component.digest).is_some_and(|hex| set.contains(&hex))
        });
    }
    let matched = preserved
        .map(|set| valid.iter().filter(|hex| set.contains(*hex)).count() as u32)
        .unwrap_or(0);
    let total = valid.len() as u32 + malformed;
    model.matched_digests = if preserved.is_none() { 0 } else { matched };
    model.total_digests = total;
    model.missing_roles = if preserved.is_none() {
        Vec::new()
    } else {
        model
            .components
            .iter()
            .filter(|component| component.recorded != Some(true))
            .map(|component| component.role.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    model.coverage = if preserved.is_none() {
        "not_checked".into()
    } else if malformed > 0 {
        if matched == 0 {
            "none_recorded".into()
        } else {
            "partially_recorded".into()
        }
    } else if total == 0 || matched == 0 {
        "none_recorded".into()
    } else if matched == total {
        "all_components_recorded".into()
    } else {
        "partially_recorded".into()
    };
    model.preserved = preserved.map(|_| model.coverage == "all_components_recorded");
}

fn model_name(path: &Path) -> String {
    let mut parts: Vec<String> = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();
    if let Some(index) = parts.iter().position(|part| part == "manifests") {
        parts = parts.split_off(index + 1);
    }
    if parts.first().is_some_and(|part| part.contains('.')) {
        parts.remove(0);
    }
    if parts.first().is_some_and(|part| part == "library") {
        parts.remove(0);
    }
    if parts.len() >= 2 {
        let tag = parts.pop().unwrap_or_default();
        format!("{}:{tag}", parts.join("/"))
    } else {
        parts.join("/")
    }
}

fn detect_format(media: &[String]) -> String {
    if media.iter().any(|kind| kind.contains("safetensors")) {
        "safetensors".into()
    } else if media
        .iter()
        .any(|kind| kind.contains("gguf") || kind.contains("image.model"))
    {
        "gguf".into()
    } else {
        "unknown".into()
    }
}

fn quantization(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    QUANTIZATION
        .iter()
        .copied()
        .find(|quant| {
            lower.split([':', '-', '_']).any(|part| part == *quant) || lower.ends_with(quant)
        })
        .map(str::to_owned)
}

fn digest_hex(digest: &str) -> Option<String> {
    let hex = digest.rsplit([':', '-']).next()?;
    if hex.len() == 64 && hex.chars().all(|char| char.is_ascii_hexdigit()) {
        Some(hex.to_ascii_lowercase())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn manifest(model: &str, template: bool, license: bool) -> String {
        let mut layers = vec![format!(
            r#"{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{model}","size":100}}"#
        )];
        if template {
            layers.push(format!(
                r#"{{"mediaType":"application/vnd.ollama.image.template","digest":"sha256:{model}","size":5}}"#
            ));
        }
        if license {
            layers.push(
                r#"{"mediaType":"application/vnd.ollama.image.license","digest":"sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","size":7}"#
                    .into(),
            );
        }
        format!(
            r#"{{"schemaVersion":2,"config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":10}},"layers":[{}]}}"#,
            layers.join(",")
        )
    }

    #[test]
    fn fixture_store_is_listed_without_writes_or_blob_reads() {
        let root = tempfile::TempDir::new().unwrap();
        let store = root.path().join("models");
        let model = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let other = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        let llama = store.join("manifests/registry.ollama.ai/library/llama3");
        let qwen = store.join("manifests/registry.ollama.ai/library/qwen2.5");
        fs::create_dir_all(&llama).unwrap();
        fs::create_dir_all(&qwen).unwrap();
        fs::write(llama.join("latest"), manifest(model, true, true)).unwrap();
        fs::write(qwen.join("q4_k_m"), manifest(other, true, false)).unwrap();
        let extra = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        for index in 0..8 {
            let dir = store.join(format!("manifests/registry.ollama.ai/library/extra{index}"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("latest"), manifest(extra, false, true)).unwrap();
        }
        let blob = store.join(format!("blobs/sha256-{model}"));
        fs::create_dir_all(blob.parent().unwrap()).unwrap();
        fs::write(&blob, b"do-not-touch").unwrap();
        let before = fs::read(&blob).unwrap();
        let started = Instant::now();
        let mut preserved = BTreeSet::new();
        preserved.insert(model.to_owned());
        preserved.insert("a".repeat(64));
        preserved.insert("dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".into());
        let report = inventory(std::slice::from_ref(&store), Some(&preserved)).unwrap();
        assert!(started.elapsed().as_secs() < 1, "inventory was not quick");
        assert_eq!(fs::read(&blob).unwrap(), before);
        assert_eq!(report.models.len(), 10);
        assert!(report.summary.contains("10 local models"));
        assert!(report.summary.contains("1 fully recorded"));
        assert!(report.summary.contains("1 without a license layer"));
        let qwen = report
            .models
            .iter()
            .find(|item| item.name == "qwen2.5:q4_k_m")
            .unwrap();
        assert_eq!(qwen.quantization.as_deref(), Some("q4_k_m"));
        assert_eq!(qwen.format, "gguf");
        assert!(!qwen.license_layer);
        assert!(qwen.template_layer);
        assert_eq!(qwen.preserved, Some(false));
        assert_eq!(qwen.coverage, "partially_recorded");
        assert!(qwen.missing_roles.iter().any(|role| role == "weights"));
        assert_eq!(qwen.size_bytes, 115);
        let llama = report
            .models
            .iter()
            .find(|item| item.name == "llama3:latest")
            .unwrap();
        assert!(llama.license_layer);
        assert_eq!(llama.preserved, Some(true));
        assert_eq!(llama.coverage, "all_components_recorded");
        assert!(llama.total_digests < llama.components.len() as u32);
        assert!(report.text().contains("llama3:latest"));
        let missing = inventory(&[root.path().join("absent")], Some(&BTreeSet::new())).unwrap();
        let unchecked = inventory(std::slice::from_ref(&store), None).unwrap();
        assert!(
            unchecked
                .models
                .iter()
                .all(|model| model.coverage == "not_checked")
        );
        assert!(
            unchecked
                .models
                .iter()
                .all(|model| model.preserved.is_none())
        );
        assert!(unchecked.note.contains("--config"));
        assert!(!unchecked.text().contains("\u{1b}"));
        assert!(missing.message.contains("not found"));
        assert!(missing.summary.contains("0 local models"));
        assert!(!missing.message.trim().is_empty());
    }

    #[test]
    fn default_locations_follow_the_environment() {
        let dirs = default_store_dirs(|key| match key {
            "OLLAMA_MODELS" => Some("/var/ollama".into()),
            "HOME" => Some("/home/example".into()),
            _ => None,
        });
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/var/ollama"),
                PathBuf::from("/home/example/.ollama/models"),
                PathBuf::from("/usr/share/ollama/.ollama/models"),
            ]
        );
    }

    #[test]
    fn unknown_media_can_be_complete_and_bad_identity_cannot() {
        let root = tempfile::TempDir::new().unwrap();
        let store = root.path().join("models");
        let weight = "c".repeat(64);
        let projector = "b".repeat(64);
        let config = "a".repeat(64);
        let write = |name: &str, body: &str| {
            let dir = store.join(format!("manifests/registry.ollama.ai/library/{name}"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("latest"), body).unwrap();
        };
        write(
            "proj",
            &format!(
                r#"{{"schemaVersion":2,"config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{config}","size":4}},"layers":[{{"mediaType":"application/vnd.example.projector","digest":"sha256:{projector}","size":8}}]}}"#
            ),
        );
        write(
            "broken",
            &format!(
                r#"{{"schemaVersion":2,"config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{config}","size":4}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{weight}","size":9}},{{"mediaType":"application/vnd.ollama.image.template","digest":"sha256:not-hex","size":1}},{{"mediaType":"","digest":"sha256:{weight}","size":1}}]}}"#
            ),
        );
        write(
            "weights-only",
            &format!(
                r#"{{"schemaVersion":2,"config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{config}","size":4}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{weight}","size":9}},{{"mediaType":"application/vnd.ollama.image.template","digest":"sha256:{projector}","size":2}}]}}"#
            ),
        );
        let mut preserved = BTreeSet::new();
        preserved.insert(weight);
        preserved.insert(config.clone());
        preserved.insert(projector.clone());
        let report = inventory(std::slice::from_ref(&store), Some(&preserved)).unwrap();
        let proj = report
            .models
            .iter()
            .find(|model| model.name == "proj:latest")
            .unwrap();
        assert!(
            proj.components
                .iter()
                .any(|item| item.role == "other" && item.media_type.contains("projector"))
        );
        assert_eq!(proj.coverage, "all_components_recorded");
        assert_eq!(proj.preserved, Some(true));
        let broken = report
            .models
            .iter()
            .find(|model| model.name == "broken:latest")
            .unwrap();
        assert!(
            broken
                .components
                .iter()
                .any(|item| item.digest.contains("not-hex"))
        );
        assert!(
            broken
                .components
                .iter()
                .any(|item| item.media_type.is_empty())
        );
        assert_ne!(broken.coverage, "all_components_recorded");
        assert_eq!(broken.preserved, Some(false));
        assert!(
            broken
                .missing_roles
                .iter()
                .any(|role| role == "template" || role == "unknown")
        );
        preserved.remove(&projector);
        let reread = inventory(std::slice::from_ref(&store), Some(&preserved)).unwrap();
        let partial = reread
            .models
            .iter()
            .find(|model| model.name == "weights-only:latest")
            .unwrap();
        assert_eq!(partial.coverage, "partially_recorded");
        assert!(partial.missing_roles.iter().any(|role| role == "template"));
        assert!(reread.text().contains("missing template"));
        assert!(!reread.text().contains("\u{1b}"));
    }
}
