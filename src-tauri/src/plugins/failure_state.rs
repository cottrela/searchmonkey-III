use crate::plugins::index_paths::mirror_failure_state_path;
use crate::plugins::registry::RegisteredPlugin;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const FAILURE_STATE_SCHEMA: &str = "sm.failure-state.v1";
const BACKOFF_SCHEDULE_SECONDS: [u64; 5] = [300, 900, 3600, 14400, 43200];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginFailureState {
    pub schema: String,
    pub plugin_id: String,
    pub plugin_version: String,
    pub source_path: String,
    pub source_size: u64,
    pub source_mtime: String,
    pub attempts: u32,
    pub last_failed_at: String,
    pub next_retry_at: String,
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub details: String,
    pub retry_after_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct FailureDisplay {
    pub code: String,
    pub message: String,
    pub details: String,
}

impl PluginFailureState {
    pub fn load(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed reading failure state {}", path.display()))?;
        let state: Self = serde_json::from_str(&contents)
            .with_context(|| format!("failed parsing failure state {}", path.display()))?;
        Ok(state)
    }
}

pub fn failure_state_path(index_root: &Path, source_path: &Path) -> PathBuf {
    mirror_failure_state_path(index_root, source_path)
}

pub fn load_failure_state(index_root: &Path, source_path: &Path) -> Option<PluginFailureState> {
    let path = failure_state_path(index_root, source_path);
    if !path.is_file() {
        return None;
    }
    PluginFailureState::load(&path).ok()
}

pub fn remove_failure_state(index_root: &Path, source_path: &Path) -> Result<()> {
    let path = failure_state_path(index_root, source_path);
    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("failed removing failure state {}", path.display()))?;
    }
    Ok(())
}

pub fn save_failure_state(
    index_root: &Path,
    source_path: &Path,
    plugin: &RegisteredPlugin,
    attempts: u32,
    display: FailureDisplay,
) -> Result<PluginFailureState> {
    let source_metadata = fs::metadata(source_path)
        .with_context(|| format!("failed reading source metadata {}", source_path.display()))?;
    let source_mtime = rfc3339_seconds(source_metadata.modified().unwrap_or(SystemTime::now()));
    let now = SystemTime::now();
    let last_failed_at = rfc3339_seconds(now);
    let retry_after_seconds = backoff_seconds(attempts);
    let next_retry_at = rfc3339_seconds(now + Duration::from_secs(retry_after_seconds));
    let state = PluginFailureState {
        schema: FAILURE_STATE_SCHEMA.to_string(),
        plugin_id: plugin.id.clone(),
        plugin_version: plugin.version.clone(),
        source_path: source_path.display().to_string(),
        source_size: source_metadata.len(),
        source_mtime,
        attempts,
        last_failed_at,
        next_retry_at,
        code: display.code,
        message: display.message,
        details: display.details,
        retry_after_seconds,
    };

    let path = failure_state_path(index_root, source_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed creating failure state directory {}",
                parent.display()
            )
        })?;
    }
    fs::write(&path, serde_json::to_vec_pretty(&state)?)
        .with_context(|| format!("failed writing failure state {}", path.display()))?;
    Ok(state)
}

pub fn retry_allowed(
    state: &PluginFailureState,
    source_path: &Path,
    plugin: &RegisteredPlugin,
    now: SystemTime,
) -> bool {
    if state.plugin_id != plugin.id || state.plugin_version != plugin.version {
        return true;
    }
    let Ok(metadata) = fs::metadata(source_path) else {
        return true;
    };
    if metadata.len() != state.source_size {
        return true;
    }
    let actual_mtime = rfc3339_seconds(metadata.modified().unwrap_or(now));
    if actual_mtime != state.source_mtime {
        return true;
    }
    parse_rfc3339(&state.next_retry_at)
        .map(|retry_at| OffsetDateTime::from(now) >= retry_at)
        .unwrap_or(true)
}

pub fn classify_failure(raw_error: &str) -> FailureDisplay {
    let trimmed = raw_error.trim();
    if let Some(display) = parse_structured_failure(trimmed) {
        return display;
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("cloud")
        || lower.contains("onedrive")
        || lower.contains("always keep on this device")
    {
        return FailureDisplay {
            code: "cloud_file_unavailable".to_string(),
            message: "Cloud file unavailable".to_string(),
            details: trimmed.to_string(),
        };
    }
    if lower.contains("encrypted") {
        return FailureDisplay {
            code: "encrypted_pdf".to_string(),
            message: "Encrypted PDF".to_string(),
            details: trimmed.to_string(),
        };
    }
    if lower.contains("formaterror") || lower.contains("corrupt") {
        return FailureDisplay {
            code: "corrupt_pdf".to_string(),
            message: "Corrupt PDF".to_string(),
            details: trimmed.to_string(),
        };
    }
    if lower.contains("failed to open pdf") || lower.contains("could not be opened") {
        return FailureDisplay {
            code: "pdf_open_failed".to_string(),
            message: "PDF could not be opened".to_string(),
            details: trimmed.to_string(),
        };
    }
    if lower.contains("timed out") {
        return FailureDisplay {
            code: "plugin_timeout".to_string(),
            message: "Plugin timed out".to_string(),
            details: trimmed.to_string(),
        };
    }

    FailureDisplay {
        code: "plugin_failed".to_string(),
        message: "Plugin failed".to_string(),
        details: trimmed.to_string(),
    }
}

fn parse_structured_failure(raw_error: &str) -> Option<FailureDisplay> {
    let start = raw_error.find('{')?;
    let end = raw_error.rfind('}')?;
    let value: Value = serde_json::from_str(&raw_error[start..=end]).ok()?;
    if value.get("status")?.as_str()? != "failed" {
        return None;
    }
    let code = value
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("plugin_failed");
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("Plugin failed");
    let hint = value.get("hint").and_then(Value::as_str).unwrap_or("");
    let details = if hint.is_empty() {
        raw_error.trim().to_string()
    } else {
        format!("{message}\n{hint}")
    };

    Some(FailureDisplay {
        code: code.to_string(),
        message: message.to_string(),
        details,
    })
}

fn backoff_seconds(attempts: u32) -> u64 {
    let index = attempts.saturating_sub(1) as usize;
    *BACKOFF_SCHEDULE_SECONDS
        .get(index)
        .unwrap_or(BACKOFF_SCHEDULE_SECONDS.last().unwrap_or(&43200))
}

fn rfc3339_seconds(value: SystemTime) -> String {
    let datetime = OffsetDateTime::from(value)
        .to_offset(time::UtcOffset::UTC)
        .replace_nanosecond(0)
        .expect("zero nanoseconds should be valid");
    datetime
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn parse_rfc3339(value: &str) -> Result<OffsetDateTime> {
    Ok(OffsetDateTime::parse(value, &Rfc3339)?)
}

#[cfg(test)]
mod tests {
    use super::{classify_failure, retry_allowed, rfc3339_seconds, PluginFailureState};
    use crate::plugins::manifest::{PluginCapabilities, PluginPermission};
    use crate::plugins::registry::RegisteredPlugin;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};
    use tempfile::tempdir;

    fn plugin() -> RegisteredPlugin {
        RegisteredPlugin {
            id: "sm.plugin.pdf".to_string(),
            name: "PDF".to_string(),
            version: "0.1.0".to_string(),
            root_dir: PathBuf::from("/tmp/plugin"),
            command: PathBuf::from("/tmp/plugin/bin/sm-plugin-pdf"),
            args: vec!["--job".to_string()],
            handles: vec![".pdf".to_string()],
            requires_entitlement: false,
            timeout_seconds: 30,
            capabilities: PluginCapabilities {
                text: true,
                layout: true,
                ocr: false,
            },
            permissions: vec![PluginPermission::ReadSourceFile],
        }
    }

    #[test]
    fn classifies_cloud_file_failures() {
        let failure = classify_failure(
            "plugin failed: Error: failed to open PDF /Users/a/CloudStorage/foo.pdf",
        );
        assert_eq!(failure.code, "cloud_file_unavailable");
        assert_eq!(failure.message, "Cloud file unavailable");
    }

    #[test]
    fn respects_retry_holdoff_for_unchanged_files() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("report.pdf");
        fs::write(&source, b"pdf").unwrap();
        let metadata = fs::metadata(&source).unwrap();
        let modified = metadata.modified().unwrap();
        let state = PluginFailureState {
            schema: "sm.failure-state.v1".to_string(),
            plugin_id: "sm.plugin.pdf".to_string(),
            plugin_version: "0.1.0".to_string(),
            source_path: source.display().to_string(),
            source_size: metadata.len(),
            source_mtime: rfc3339_seconds(modified),
            attempts: 1,
            last_failed_at: rfc3339_seconds(SystemTime::now()),
            next_retry_at: rfc3339_seconds(SystemTime::now() + Duration::from_secs(3600)),
            code: "plugin_failed".to_string(),
            message: "Plugin failed".to_string(),
            details: "detail".to_string(),
            retry_after_seconds: 3600,
        };
        assert!(!retry_allowed(
            &state,
            &source,
            &plugin(),
            SystemTime::now()
        ));
    }
}
