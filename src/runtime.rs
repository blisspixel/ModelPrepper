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
    pub models: Vec<RuntimeModel>,
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
    pub preserved: bool,
}

impl RuntimeInventory {
    pub fn text(&self) -> String {
        let mut lines = Vec::new();
        if !self.message.is_empty() {
            lines.push(self.message.clone());
        }
        lines.push(self.summary.clone());
        for model in &self.models {
            let quant = model.quantization.as_deref().unwrap_or("unknown");
            let preserved = if model.preserved {
                "preserved"
            } else {
                "not preserved"
            };
            let license = if model.license_layer {
                "license"
            } else {
                "no license layer"
            };
            lines.push(format!(
                "{name}  {size} bytes  {format}/{quant}  {license}  {preserved}  {digests}",
                name = model.name,
                size = model.size_bytes,
                format = model.format,
                digests = model.digests.join(","),
            ));
        }
        lines.join("\n")
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
pub fn inventory(dirs: &[PathBuf], preserved: &BTreeSet<String>) -> Result<RuntimeInventory> {
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
    let preserved_count = models.iter().filter(|model| model.preserved).count();
    let unlicensed = models.iter().filter(|model| !model.license_layer).count();
    Ok(RuntimeInventory {
        schema_version: 1,
        summary: format!(
            "{} local models, {preserved_count} preserved, {unlicensed} without a license layer",
            models.len()
        ),
        message: notes.join("; "),
        models,
    })
}

fn read_store(
    dir: &Path,
    preserved: &BTreeSet<String>,
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
                model.preserved = !model.digests.is_empty()
                    && model.digests.iter().all(|digest| {
                        digest_hex(digest).is_some_and(|hex| preserved.contains(&hex))
                    });
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
    let mut size = 0_u64;
    let mut media = Vec::new();
    for layer in std::iter::once(&config).chain(layers.iter()) {
        if layer.is_null() {
            continue;
        }
        let digest = layer
            .get("digest")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if !digest.is_empty() {
            digests.push(digest);
        }
        let layer_size = layer.get("size").and_then(Value::as_u64).unwrap_or(0);
        size = size
            .checked_add(layer_size)
            .ok_or_else(|| Error::new("size_overflow", "Ollama manifest sizes overflow u64"))?;
        if let Some(kind) = layer.get("mediaType").and_then(Value::as_str) {
            media.push(kind.to_ascii_lowercase());
        }
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
        preserved: false,
    })
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
        let report = inventory(std::slice::from_ref(&store), &preserved).unwrap();
        assert!(started.elapsed().as_secs() < 1, "inventory was not quick");
        assert_eq!(fs::read(&blob).unwrap(), before);
        assert_eq!(report.models.len(), 10);
        assert!(report.summary.contains("10 local models"));
        assert!(report.summary.contains("1 preserved"));
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
        assert!(!qwen.preserved);
        assert_eq!(qwen.size_bytes, 115);
        let llama = report
            .models
            .iter()
            .find(|item| item.name == "llama3:latest")
            .unwrap();
        assert!(llama.license_layer);
        assert!(llama.preserved);
        assert!(report.text().contains("llama3:latest"));
        let missing = inventory(&[root.path().join("absent")], &BTreeSet::new()).unwrap();
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
}
