use crate::plugins::cache::{self, CacheStatus};
use crate::plugins::classifier::{FileClassifier, FileKind};
use crate::plugins::failure_state::{classify_failure, FailureDisplay};
use crate::plugins::index_paths::{
    default_index_roots, mirror_meta_path, mirror_meta_tmp_path, mirror_text_path,
    mirror_text_tmp_path,
};
use crate::plugins::registry::{default_plugin_roots, PluginRegistry, RegisteredPlugin};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::json;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static JOB_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexOutcome {
    Ready,
    Indexed,
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexResult {
    pub outcome: IndexOutcome,
    pub plugin_id: String,
    pub source_path: String,
    pub text_path: String,
    pub meta_path: String,
    pub cache_status: String,
}

#[derive(Debug, Clone)]
pub struct IndexFailure {
    pub display: FailureDisplay,
}

impl std::fmt::Display for IndexFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display.message)
    }
}

impl std::error::Error for IndexFailure {}

pub fn index_file_with_plugin(source_path: &Path) -> Result<IndexResult> {
    let plugin_roots = default_plugin_roots();
    let index_roots = default_index_roots();
    index_file_with_plugin_paths(source_path, &plugin_roots, &index_roots)
}

pub fn index_file_with_plugin_paths(
    source_path: &Path,
    plugin_roots: &[PathBuf],
    index_roots: &[PathBuf],
) -> Result<IndexResult> {
    let source_path = source_path
        .canonicalize()
        .with_context(|| format!("failed to resolve source path {}", source_path.display()))?;
    if !source_path.is_file() {
        bail!("source path is not a file: {}", source_path.display());
    }

    let discovery = PluginRegistry::discover(plugin_roots)?;
    let classifier = FileClassifier::new(&discovery.registry);
    let plugin = plugin_for_source(&classifier, &discovery.registry, &source_path)?;

    let cache = cache::validate_cache(&source_path, plugin);
    if cache.status == CacheStatus::Ready {
        return Ok(IndexResult {
            outcome: IndexOutcome::Ready,
            plugin_id: plugin.id.clone(),
            source_path: source_path.display().to_string(),
            text_path: cache.text_path.display().to_string(),
            meta_path: cache.meta_path.display().to_string(),
            cache_status: cache_status_name(&cache.status).to_string(),
        });
    }

    let index_root = index_roots
        .first()
        .cloned()
        .context("no index root is configured for this platform")?;
    let output_text_final_path = mirror_text_path(&index_root, &source_path);
    let output_meta_final_path = mirror_meta_path(&index_root, &source_path);
    let output_text_tmp_path = mirror_text_tmp_path(&index_root, &source_path);
    let output_meta_tmp_path = mirror_meta_tmp_path(&index_root, &source_path);

    if let Some(parent) = output_text_final_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create index directory {}", parent.display()))?;
    }

    cleanup_tmp_path(&output_text_tmp_path)?;
    cleanup_tmp_path(&output_meta_tmp_path)?;

    let job_id = next_job_id();
    let job_path = std::env::temp_dir().join(format!("{job_id}.json"));
    let job_json = json!({
        "schema": "sm.plugin-job.v1",
        "job_id": job_id,
        "plugin_id": plugin.id,
        "source_path": source_path,
        "output_text_path": output_text_tmp_path,
        "output_meta_path": output_meta_tmp_path,
        "settings": {
            "layout": true,
            "ocr": false
        }
    });
    fs::write(&job_path, serde_json::to_vec_pretty(&job_json)?)
        .with_context(|| format!("failed writing plugin job file {}", job_path.display()))?;

    let run_result = run_plugin_process(plugin, &job_path, &job_id);
    let _ = fs::remove_file(&job_path);

    if let Err(err) = run_result {
        let _ = cleanup_tmp_path(&output_text_tmp_path);
        let _ = cleanup_tmp_path(&output_meta_tmp_path);
        return Err(IndexFailure {
            display: classify_failure(&err.to_string()),
        }
        .into());
    }

    normalize_generated_meta_text_path(
        &output_meta_tmp_path,
        &output_text_tmp_path,
        Some(&output_text_final_path),
    )?;

    let validation = cache::validate_cache_paths(
        &source_path,
        &output_text_tmp_path,
        &output_meta_tmp_path,
        Some(plugin),
    );
    if validation.status != CacheStatus::Ready {
        let _ = cleanup_tmp_path(&output_text_tmp_path);
        let _ = cleanup_tmp_path(&output_meta_tmp_path);
        if let Some(problem) = validation.problem.as_deref() {
            bail!(
                "plugin outputs failed validation: {} ({problem})",
                cache_status_name(&validation.status)
            );
        } else {
            bail!(
                "plugin outputs failed validation: {}",
                cache_status_name(&validation.status)
            );
        }
    }

    rewrite_promoted_meta_paths(
        &output_meta_tmp_path,
        &output_text_tmp_path,
        &output_text_final_path,
    )?;

    promote_output(&output_text_tmp_path, &output_text_final_path)?;
    promote_output(&output_meta_tmp_path, &output_meta_final_path)?;
    Ok(IndexResult {
        outcome: IndexOutcome::Indexed,
        plugin_id: plugin.id.clone(),
        source_path: source_path.display().to_string(),
        text_path: output_text_final_path.display().to_string(),
        meta_path: output_meta_final_path.display().to_string(),
        cache_status: cache_status_name(&cache.status).to_string(),
    })
}

fn plugin_for_source<'a>(
    classifier: &FileClassifier,
    registry: &'a PluginRegistry,
    source_path: &Path,
) -> Result<&'a RegisteredPlugin> {
    match classifier.classify(source_path) {
        FileKind::SupportedByPlugin { plugin_id } => registry
            .by_id
            .get(&plugin_id)
            .with_context(|| format!("plugin {plugin_id} is not registered")),
        other => bail!("source file is not handled by a plugin: {other:?}"),
    }
}

fn run_plugin_process(plugin: &RegisteredPlugin, job_path: &Path, job_id: &str) -> Result<()> {
    let mut child = Command::new(&plugin.command)
        .args(&plugin.args)
        .arg(job_path)
        .current_dir(&plugin.root_dir)
        .env("SM_PLUGIN_ROOT", &plugin.root_dir)
        .env("SM_PLUGIN_ID", &plugin.id)
        .env("SM_PLUGIN_VERSION", &plugin.version)
        .env("SM_JOB_ID", job_id)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn plugin {}", plugin.command.display()))?;

    let deadline = Instant::now() + Duration::from_secs(plugin.timeout_seconds);
    loop {
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("failed waiting for plugin {}", plugin.command.display()))?
        {
            let stderr = read_child_stderr(&mut child);
            if status.success() {
                return Ok(());
            }
            if stderr.is_empty() {
                bail!("plugin exited with status {}", status);
            } else {
                bail!("plugin failed: {stderr}");
            }
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let stderr = read_child_stderr(&mut child);
            if stderr.is_empty() {
                bail!("plugin timed out after {} seconds", plugin.timeout_seconds);
            } else {
                bail!(
                    "plugin timed out after {} seconds: {stderr}",
                    plugin.timeout_seconds
                );
            }
        }

        thread::sleep(Duration::from_millis(50));
    }
}

fn read_child_stderr(child: &mut std::process::Child) -> String {
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    stderr.trim().to_string()
}

fn cleanup_tmp_path(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path)
            .with_context(|| format!("failed to remove temporary file {}", path.display()))?;
    }
    Ok(())
}

fn promote_output(tmp_path: &Path, final_path: &Path) -> Result<()> {
    match fs::rename(tmp_path, final_path) {
        Ok(()) => Ok(()),
        Err(first_err) => {
            if final_path.exists() {
                fs::remove_file(final_path).with_context(|| {
                    format!("failed to replace existing output {}", final_path.display())
                })?;
                fs::rename(tmp_path, final_path).with_context(|| {
                    format!(
                        "failed to promote {} to {} after replacing existing output ({first_err})",
                        tmp_path.display(),
                        final_path.display()
                    )
                })?;
                Ok(())
            } else {
                Err(first_err).with_context(|| {
                    format!(
                        "failed to promote {} to {}",
                        tmp_path.display(),
                        final_path.display()
                    )
                })
            }
        }
    }
}

fn rewrite_promoted_meta_paths(
    meta_tmp_path: &Path,
    text_tmp_path: &Path,
    text_final_path: &Path,
) -> Result<()> {
    normalize_generated_meta_text_path(meta_tmp_path, text_final_path, Some(text_tmp_path))
}

fn normalize_generated_meta_text_path(
    meta_path: &Path,
    desired_text_path: &Path,
    alternate_text_path: Option<&Path>,
) -> Result<()> {
    let contents = fs::read_to_string(meta_path)
        .with_context(|| format!("failed reading plugin meta output {}", meta_path.display()))?;
    let mut json: serde_json::Value = serde_json::from_str(&contents)
        .with_context(|| format!("failed parsing plugin meta output {}", meta_path.display()))?;

    let Some(text) = json.get_mut("text").and_then(|value| value.as_object_mut()) else {
        bail!("plugin meta output is missing text object");
    };

    let current_path = text
        .get("path")
        .and_then(|value| value.as_str())
        .context("plugin meta output is missing text.path")?;

    let resolved_current =
        normalize_lexical_path(resolve_recorded_meta_path(meta_path, current_path));
    let desired_normalized = normalize_lexical_path(desired_text_path.to_path_buf());
    let alternate_normalized =
        alternate_text_path.map(|path| normalize_lexical_path(path.to_path_buf()));

    if resolved_current == desired_normalized
        || alternate_normalized
            .as_ref()
            .is_some_and(|alternate| *alternate == resolved_current)
        || sibling_name_matches(&resolved_current, desired_text_path, alternate_text_path)
    {
        text.insert(
            "path".to_string(),
            serde_json::Value::String(desired_text_path.display().to_string()),
        );
        fs::write(meta_path, serde_json::to_vec_pretty(&json)?).with_context(|| {
            format!(
                "failed rewriting plugin meta output {} for promotion",
                meta_path.display()
            )
        })?;
    }

    Ok(())
}

fn sibling_name_matches(
    resolved_current: &Path,
    desired_text_path: &Path,
    alternate_text_path: Option<&Path>,
) -> bool {
    let Some(current_parent) = resolved_current.parent() else {
        return false;
    };
    let Some(desired_parent) = desired_text_path.parent() else {
        return false;
    };
    if normalize_lexical_path(current_parent.to_path_buf())
        != normalize_lexical_path(desired_parent.to_path_buf())
    {
        return false;
    }

    let current_name = resolved_current.file_name();
    let desired_name = desired_text_path.file_name();
    let alternate_name = alternate_text_path.and_then(|path| path.file_name());
    current_name.is_some() && (current_name == desired_name || current_name == alternate_name)
}

fn resolve_recorded_meta_path(meta_path: &Path, recorded_path: &str) -> PathBuf {
    let recorded = Path::new(recorded_path);
    if recorded.is_absolute() {
        return recorded.to_path_buf();
    }

    meta_path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(recorded)
}

fn normalize_lexical_path(path: PathBuf) -> PathBuf {
    use std::path::Component;

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn next_job_id() -> String {
    let count = JOB_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("job_{millis}_{count}")
}

fn cache_status_name(status: &CacheStatus) -> &'static str {
    match status {
        CacheStatus::Ready => "ready",
        CacheStatus::MissingText => "missing_text",
        CacheStatus::MissingMeta => "missing_meta",
        CacheStatus::StaleSourceSize => "stale_source_size",
        CacheStatus::StaleSourceMtime => "stale_source_mtime",
        CacheStatus::StalePlugin => "stale_plugin",
        CacheStatus::InvalidMeta => "invalid_meta",
        CacheStatus::InvalidText => "invalid_text",
    }
}

#[cfg(test)]
mod tests {
    use super::{index_file_with_plugin_paths, normalize_generated_meta_text_path, IndexOutcome};
    use crate::plugins::index_paths::mirror_text_path;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use tempfile::tempdir;

    #[cfg(unix)]
    #[test]
    fn indexes_one_file_through_plugin_process() {
        let temp = tempdir().unwrap();
        let source_path = temp.path().join("docs/valid.pdf");
        let plugin_root = temp.path().join("plugins/sm.plugin.pdf/0.1.0");
        let index_root = temp.path().join("index");
        fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        fs::create_dir_all(plugin_root.join("bin")).unwrap();
        fs::write(&source_path, b"%PDF-test").unwrap();

        fs::write(
            plugin_root.join("plugin.toml"),
            r#"
schema = "sm.plugin.v1"
id = "sm.plugin.pdf"
name = "PDF Plugin"
version = "0.1.0"
handles = [".pdf"]
platforms = ["macos-arm64", "macos-x64", "linux-x64"]

[entry]
kind = "process"
command = "sm-plugin-pdf"
args = ["--job"]
"#,
        )
        .unwrap();

        let script = r#"#!/bin/sh
JOB_PATH="$2"
python3 - "$JOB_PATH" <<'PY'
import hashlib
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path

job = json.load(open(sys.argv[1], "r", encoding="utf-8"))
source = Path(job["source_path"])
text = Path(job["output_text_path"])
meta = Path(job["output_meta_path"])
text.parent.mkdir(parents=True, exist_ok=True)
content = "hello plugin world\n"
text.write_text(content, encoding="utf-8")
source_stat = source.stat()
text_stat = text.stat()
source_hash = "sha256:" + hashlib.sha256(source.read_bytes()).hexdigest()
text_hash = "sha256:" + hashlib.sha256(text.read_bytes()).hexdigest()
def mtime(path_stat):
    return datetime.fromtimestamp(path_stat.st_mtime, timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")
meta.write_text(json.dumps({
    "schema": "sm.meta.v1",
    "source": {
        "path": str(source),
        "size": source_stat.st_size,
        "mtime": mtime(source_stat),
        "hash": source_hash
    },
    "generator": {
        "plugin_id": job["plugin_id"],
        "plugin_version": "0.1.0"
    },
    "text": {
        "path": str(text),
        "encoding": "utf-8",
        "length_bytes": text_stat.st_size,
        "mtime": mtime(text_stat),
        "hash": text_hash,
        "offsets": "utf8-bytes"
    },
    "ranges": [
        {"type": "document", "start": 0, "end": text_stat.st_size, "index": 1},
        {"type": "page", "start": 0, "end": text_stat.st_size, "page": 1, "index": 1}
    ]
}, indent=2), encoding="utf-8")
PY
"#;
        let script_path = plugin_root.join("bin/sm-plugin-pdf");
        fs::write(&script_path, script).unwrap();
        let mut perms = fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).unwrap();

        let result = index_file_with_plugin_paths(
            &source_path,
            &[temp.path().join("plugins")],
            &[index_root.clone()],
        )
        .unwrap();

        assert!(matches!(result.outcome, IndexOutcome::Indexed));
        assert_eq!(
            result.text_path,
            mirror_text_path(&index_root, &source_path.canonicalize().unwrap())
                .display()
                .to_string()
        );
        assert!(Path::new(&result.text_path).is_file());
        assert!(Path::new(&result.meta_path).is_file());
        let meta = fs::read_to_string(&result.meta_path).unwrap();
        assert!(meta.contains(&format!("\"path\": \"{}\"", result.text_path)));
        assert!(!Path::new(&format!("{}.tmp", result.text_path)).exists());
        assert!(!Path::new(&format!("{}.tmp", result.meta_path)).exists());
    }

    #[test]
    fn normalizes_meta_path_from_final_to_tmp_and_back() {
        let temp = tempdir().unwrap();
        let meta_path = temp.path().join("report.pdf.sm.meta.tmp");
        let tmp_text_path = temp.path().join("report.pdf.sm.txt.tmp");
        let final_text_path = temp.path().join("report.pdf.sm.txt");

        fs::write(
            &meta_path,
            format!(
                r#"{{
  "schema": "sm.meta.v1",
  "source": {{
    "path": "/tmp/report.pdf",
    "size": 3,
    "mtime": "2026-05-10T12:00:00Z"
  }},
  "generator": {{
    "plugin_id": "sm.plugin.pdf",
    "plugin_version": "0.1.0"
  }},
  "text": {{
    "path": "{}",
    "encoding": "utf-8",
    "length_bytes": 11,
    "offsets": "utf8-bytes"
  }},
  "ranges": [{{ "type": "document", "start": 0, "end": 11, "index": 1 }}]
}}"#,
                final_text_path.display()
            ),
        )
        .unwrap();

        normalize_generated_meta_text_path(&meta_path, &tmp_text_path, Some(&final_text_path))
            .unwrap();
        let tmp_contents = fs::read_to_string(&meta_path).unwrap();
        assert!(tmp_contents.contains(&format!(r#""path": "{}""#, tmp_text_path.display())));

        normalize_generated_meta_text_path(&meta_path, &final_text_path, Some(&tmp_text_path))
            .unwrap();
        let final_contents = fs::read_to_string(&meta_path).unwrap();
        assert!(final_contents.contains(&format!(r#""path": "{}""#, final_text_path.display())));
    }
}
