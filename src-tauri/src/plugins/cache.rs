use crate::plugins::classifier::{meta_for_sm_text, source_for_sm_text};
use crate::plugins::index_paths::{
    default_index_roots, mirror_meta_path, mirror_text_path, source_path_from_mirror_text_path,
};
use crate::plugins::meta::SmMeta;
use crate::plugins::registry::{plugin_version_satisfies_selected, RegisteredPlugin};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheStatus {
    Ready,
    MissingText,
    MissingMeta,
    StaleSourceSize,
    StaleSourceMtime,
    StalePlugin,
    InvalidMeta,
    InvalidText,
}

#[derive(Debug, Clone)]
pub struct CacheValidationResult {
    pub status: CacheStatus,
    pub source_path: PathBuf,
    pub text_path: PathBuf,
    pub meta_path: PathBuf,
    pub meta: Option<SmMeta>,
    pub problem: Option<String>,
}

impl CacheValidationResult {
    pub fn is_ready(&self) -> bool {
        self.status == CacheStatus::Ready
    }
}

pub fn expected_cache_paths(source_path: &Path) -> Result<(PathBuf, PathBuf)> {
    let _ = source_path
        .file_name()
        .and_then(|value| value.to_str())
        .context("source path must have a valid file name")?;

    for index_root in default_index_roots() {
        let text_path = mirror_text_path(&index_root, source_path);
        let meta_path = mirror_meta_path(&index_root, source_path);
        if text_path.exists() || meta_path.exists() {
            return Ok((text_path, meta_path));
        }
    }

    #[cfg(test)]
    {
        let source_name = source_path
            .file_name()
            .and_then(|value| value.to_str())
            .context("source path must have a valid file name")?;
        let parent = source_path.parent().unwrap_or_else(|| Path::new(""));
        let text_path = parent.join(format!("{source_name}.sm.txt"));
        let meta_path = parent.join(format!("{source_name}.sm.meta"));
        if text_path.exists() || meta_path.exists() {
            return Ok((text_path, meta_path));
        }
    }

    let fallback_root = default_index_roots()
        .into_iter()
        .next()
        .context("no index root is configured for this platform")?;
    Ok((
        mirror_text_path(&fallback_root, source_path),
        mirror_meta_path(&fallback_root, source_path),
    ))
}

pub fn validate_cache(source_path: &Path, plugin: &RegisteredPlugin) -> CacheValidationResult {
    validate_cache_with_plugin(source_path, Some(plugin))
}

pub fn validate_cache_paths(
    source_path: &Path,
    text_path: &Path,
    meta_path: &Path,
    plugin: Option<&RegisteredPlugin>,
) -> CacheValidationResult {
    validate_cache_at_paths(
        source_path,
        text_path.to_path_buf(),
        meta_path.to_path_buf(),
        plugin,
    )
}

pub fn validate_generated_text_standalone(text_path: &Path) -> CacheValidationResult {
    let source_path = source_path_from_mirror_text_path(text_path)
        .or_else(|| source_for_sm_text(text_path))
        .unwrap_or_else(|| text_path.to_path_buf());
    let mut validation = validate_cache_with_plugin(&source_path, None);

    if validation.text_path != text_path {
        validation.status = CacheStatus::InvalidMeta;
        return validation;
    }

    if meta_for_sm_text(text_path).is_none() {
        validation.status = CacheStatus::InvalidMeta;
    }

    validation
}

fn validate_cache_with_plugin(
    source_path: &Path,
    plugin: Option<&RegisteredPlugin>,
) -> CacheValidationResult {
    let (text_path, meta_path) = match expected_cache_paths(source_path) {
        Ok(paths) => paths,
        Err(_) => {
            return CacheValidationResult {
                status: CacheStatus::InvalidMeta,
                source_path: source_path.to_path_buf(),
                text_path: source_path.to_path_buf(),
                meta_path: source_path.to_path_buf(),
                meta: None,
                problem: Some("could not resolve expected cache paths".to_string()),
            };
        }
    };

    validate_cache_at_paths(source_path, text_path, meta_path, plugin)
}

fn validate_cache_at_paths(
    source_path: &Path,
    text_path: PathBuf,
    meta_path: PathBuf,
    plugin: Option<&RegisteredPlugin>,
) -> CacheValidationResult {
    if !text_path.is_file() {
        return CacheValidationResult {
            status: CacheStatus::MissingText,
            source_path: source_path.to_path_buf(),
            text_path,
            meta_path,
            meta: None,
            problem: None,
        };
    }

    if !meta_path.is_file() {
        return CacheValidationResult {
            status: CacheStatus::MissingMeta,
            source_path: source_path.to_path_buf(),
            text_path,
            meta_path,
            meta: None,
            problem: None,
        };
    }

    let source_metadata = match fs::metadata(source_path) {
        Ok(metadata) => metadata,
        Err(err) => {
            return CacheValidationResult {
                status: CacheStatus::InvalidMeta,
                source_path: source_path.to_path_buf(),
                text_path,
                meta_path,
                meta: None,
                problem: Some(err.to_string()),
            };
        }
    };

    let text_metadata = match fs::metadata(&text_path) {
        Ok(metadata) => metadata,
        Err(err) => {
            return CacheValidationResult {
                status: CacheStatus::InvalidText,
                source_path: source_path.to_path_buf(),
                text_path,
                meta_path,
                meta: None,
                problem: Some(err.to_string()),
            };
        }
    };

    let meta = match SmMeta::load(&meta_path) {
        Ok(meta) => meta,
        Err(err) => {
            return CacheValidationResult {
                status: CacheStatus::InvalidMeta,
                source_path: source_path.to_path_buf(),
                text_path,
                meta_path,
                meta: None,
                problem: Some(err.to_string()),
            };
        }
    };

    if !paths_equivalent(&resolve_meta_path(&meta_path, &meta.source.path), source_path) {
        return CacheValidationResult {
            status: CacheStatus::InvalidMeta,
            source_path: source_path.to_path_buf(),
            text_path,
            meta_path,
            meta: None,
            problem: Some(format!(
                "source.path mismatch: recorded={} expected={}",
                meta.source.path,
                source_path.display()
            )),
        };
    }

    let actual_mtime = match source_metadata.modified() {
        Ok(modified) => modified,
        Err(_) => {
            return CacheValidationResult {
                status: CacheStatus::InvalidMeta,
                source_path: source_path.to_path_buf(),
                text_path,
                meta_path,
                meta: Some(meta),
                problem: Some("failed to read source file modified time".to_string()),
            };
        }
    };

    let source_size_matches = meta.source.size == source_metadata.len();
    let source_mtime_matches = mtimes_match(&meta.source.mtime, actual_mtime);
    if !source_size_matches || !source_mtime_matches {
        let source_hash_matches = meta
            .source
            .hash
            .as_deref()
            .and_then(|expected| file_hash_matches(source_path, expected).ok())
            .unwrap_or(false);

        if !source_hash_matches {
            return CacheValidationResult {
                status: if !source_size_matches {
                    CacheStatus::StaleSourceSize
                } else {
                    CacheStatus::StaleSourceMtime
                },
                source_path: source_path.to_path_buf(),
                text_path,
                meta_path,
                meta: Some(meta),
                problem: None,
            };
        }
    }

    if let Some(plugin) = plugin {
        if meta.generator.plugin_id != plugin.id
            || !plugin_version_satisfies_selected(&plugin.version, &meta.generator.plugin_version)
        {
            return CacheValidationResult {
                status: CacheStatus::StalePlugin,
                source_path: source_path.to_path_buf(),
                text_path,
                meta_path,
                meta: Some(meta),
                problem: None,
            };
        }
    }

    if !paths_equivalent(&resolve_meta_path(&meta_path, &meta.text.path), &text_path) {
        let recorded_text_path = meta.text.path.clone();
        let expected_text_path = text_path.display().to_string();
        return CacheValidationResult {
            status: CacheStatus::InvalidMeta,
            source_path: source_path.to_path_buf(),
            text_path,
            meta_path,
            meta: Some(meta),
            problem: Some(format!(
                "text.path mismatch: recorded={} expected={}",
                recorded_text_path, expected_text_path
            )),
        };
    }

    let text_size_matches = meta
        .text
        .length_bytes
        .is_none_or(|length| length == text_metadata.len());
    let text_modified = text_metadata.modified().ok();
    let text_mtime_matches = meta.text.mtime.as_deref().is_none_or(|mtime| {
        text_modified
            .map(|modified| mtimes_match(mtime, modified))
            .unwrap_or(false)
    });

    if !text_size_matches || !text_mtime_matches {
        let text_hash_matches = meta
            .text
            .hash
            .as_deref()
            .and_then(|expected| file_hash_matches(&text_path, expected).ok())
            .unwrap_or(false);

        if !text_hash_matches {
            return CacheValidationResult {
                status: CacheStatus::InvalidText,
                source_path: source_path.to_path_buf(),
                text_path,
                meta_path,
                meta: Some(meta),
                problem: None,
            };
        }
    }

    CacheValidationResult {
        status: CacheStatus::Ready,
        source_path: source_path.to_path_buf(),
        text_path,
        meta_path,
        meta: Some(meta),
        problem: None,
    }
}

pub fn validate_generated_text_cache(
    text_path: &Path,
    plugin: &RegisteredPlugin,
) -> CacheValidationResult {
    let source_path = source_path_from_mirror_text_path(text_path)
        .or_else(|| source_for_sm_text(text_path))
        .unwrap_or_else(|| text_path.to_path_buf());
    let mut validation = validate_cache_with_plugin(&source_path, Some(plugin));

    if !paths_equivalent(&validation.text_path, text_path) {
        validation.status = CacheStatus::InvalidMeta;
        return validation;
    }

    if meta_for_sm_text(text_path).is_none() {
        validation.status = CacheStatus::InvalidMeta;
    }

    validation
}

fn mtimes_match(expected_rfc3339: &str, actual: SystemTime) -> bool {
    let Ok(expected) = OffsetDateTime::parse(expected_rfc3339, &Rfc3339) else {
        return false;
    };
    let actual = OffsetDateTime::from(actual);
    normalize_mtime(expected) == normalize_mtime(actual)
}

fn file_hash_matches(path: &Path, expected_sha256: &str) -> Result<bool> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("failed to open file for hashing: {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed reading file for hashing: {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    let actual = format!("sha256:{:x}", hasher.finalize());
    Ok(actual.eq_ignore_ascii_case(expected_sha256))
}

fn resolve_meta_path(meta_path: &Path, recorded_path: &str) -> PathBuf {
    let recorded = Path::new(recorded_path);
    if recorded.is_absolute() {
        return recorded.to_path_buf();
    }

    meta_path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(recorded)
}

fn paths_equivalent(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }

    #[cfg(windows)]
    {
        if let (Ok(left), Ok(right)) = (left.canonicalize(), right.canonicalize()) {
            return left == right;
        }
    }

    false
}

fn normalize_mtime(value: OffsetDateTime) -> OffsetDateTime {
    value
        .to_offset(time::UtcOffset::UTC)
        .replace_nanosecond(0)
        .expect("zero nanoseconds should always be valid")
}

#[cfg(test)]
mod tests {
    use super::{validate_cache, validate_generated_text_standalone, CacheStatus};
    use crate::plugins::registry::RegisteredPlugin;
    use crate::plugins::{manifest::PluginCapabilities, manifest::PluginPermission};
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;

    #[test]
    fn returns_ready_for_matching_cache() {
        let temp = tempdir().unwrap();
        let source_path = temp.path().join("report.pdf");
        let text_path = temp.path().join("report.pdf.sm.txt");
        let meta_path = temp.path().join("report.pdf.sm.meta");

        fs::write(&source_path, b"pdf").unwrap();
        fs::write(&text_path, b"hello world").unwrap();
        let source_mtime =
            OffsetDateTime::from(fs::metadata(&source_path).unwrap().modified().unwrap())
                .format(&Rfc3339)
                .unwrap();

        fs::write(
            &meta_path,
            format!(
                r#"{{
                  "schema": "sm.meta.v1",
                  "source": {{
                    "path": "{}",
                    "size": 3,
                    "mtime": "{}"
                  }},
                  "generator": {{
                    "plugin_id": "sm.plugin.pdf",
                    "plugin_version": "1.2.3"
                  }},
                  "text": {{
                    "path": "{}",
                    "encoding": "utf-8",
                    "length_bytes": 11
                  }},
                  "ranges": [
                    {{ "type": "page", "start": 0, "end": 11, "page": 8 }},
                    {{ "type": "block", "start": 0, "end": 11, "index": 2 }}
                  ]
                }}"#,
                source_path.display(),
                source_mtime,
                text_path.display()
            ),
        )
        .unwrap();

        let validation = validate_cache(&source_path, &plugin());
        assert_eq!(validation.status, CacheStatus::Ready);
        assert!(validation.is_ready());
        assert!(validation.meta.is_some());
    }

    #[cfg(windows)]
    #[test]
    fn accepts_verbatim_paths_recorded_in_meta() {
        let temp = tempdir().unwrap();
        let source_path = temp.path().join("report.pdf");
        let text_path = temp.path().join("report.pdf.sm.txt");
        let meta_path = temp.path().join("report.pdf.sm.meta");

        fs::write(&source_path, b"pdf").unwrap();
        fs::write(&text_path, b"hello world").unwrap();
        let source_mtime =
            OffsetDateTime::from(fs::metadata(&source_path).unwrap().modified().unwrap())
                .format(&Rfc3339)
                .unwrap();
        let source_path_json =
            serde_json::to_string(&source_path.canonicalize().unwrap().to_string_lossy())
                .unwrap();
        let text_path_json =
            serde_json::to_string(&text_path.canonicalize().unwrap().to_string_lossy()).unwrap();

        fs::write(
            &meta_path,
            format!(
                r#"{{
                  "schema": "sm.meta.v1",
                  "source": {{
                    "path": {source_path_json},
                    "size": 3,
                    "mtime": "{source_mtime}"
                  }},
                  "generator": {{
                    "plugin_id": "sm.plugin.pdf",
                    "plugin_version": "1.2.3"
                  }},
                  "text": {{
                    "path": {text_path_json},
                    "encoding": "utf-8",
                    "length_bytes": 11
                  }},
                  "ranges": [
                    {{ "type": "page", "start": 0, "end": 11, "page": 8 }}
                  ]
                }}"#
            ),
        )
        .unwrap();

        let validation = validate_cache(&source_path, &plugin());
        assert_eq!(validation.status, CacheStatus::Ready);
    }

    #[test]
    fn returns_stale_plugin_for_generator_mismatch() {
        let temp = tempdir().unwrap();
        let source_path = temp.path().join("report.pdf");
        let text_path = temp.path().join("report.pdf.sm.txt");
        let meta_path = temp.path().join("report.pdf.sm.meta");

        fs::write(&source_path, b"pdf").unwrap();
        fs::write(&text_path, b"hello world").unwrap();
        let source_mtime =
            OffsetDateTime::from(fs::metadata(&source_path).unwrap().modified().unwrap())
                .format(&Rfc3339)
                .unwrap();

        fs::write(
            &meta_path,
            format!(
                r#"{{
                  "schema": "sm.meta.v1",
                  "source": {{
                    "path": "{}",
                    "size": 3,
                    "mtime": "{}"
                  }},
                  "generator": {{
                    "plugin_id": "sm.plugin.pdf",
                    "plugin_version": "9.9.9"
                  }},
                  "text": {{
                    "path": "{}",
                    "encoding": "utf-8",
                    "length_bytes": 11
                  }},
                  "ranges": [
                    {{ "type": "page", "start": 0, "end": 11, "page": 8 }}
                  ]
                }}"#,
                source_path.display(),
                source_mtime,
                text_path.display()
            ),
        )
        .unwrap();

        let validation = validate_cache(&source_path, &plugin());
        assert_eq!(validation.status, CacheStatus::StalePlugin);
    }

    #[test]
    fn returns_invalid_text_for_length_mismatch() {
        let temp = tempdir().unwrap();
        let source_path = temp.path().join("report.pdf");
        let text_path = temp.path().join("report.pdf.sm.txt");
        let meta_path = temp.path().join("report.pdf.sm.meta");

        fs::write(&source_path, b"pdf").unwrap();
        fs::write(&text_path, b"hello").unwrap();
        let source_mtime =
            OffsetDateTime::from(fs::metadata(&source_path).unwrap().modified().unwrap())
                .format(&Rfc3339)
                .unwrap();

        fs::write(
            &meta_path,
            format!(
                r#"{{
                  "schema": "sm.meta.v1",
                  "source": {{
                    "path": "{}",
                    "size": 3,
                    "mtime": "{}"
                  }},
                  "generator": {{
                    "plugin_id": "sm.plugin.pdf",
                    "plugin_version": "1.2.3"
                  }},
                  "text": {{
                    "path": "{}",
                    "encoding": "utf-8",
                    "length_bytes": 11
                  }},
                  "ranges": [
                    {{ "type": "page", "start": 0, "end": 5, "page": 8 }}
                  ]
                }}"#,
                source_path.display(),
                source_mtime,
                text_path.display()
            ),
        )
        .unwrap();

        let validation = validate_cache(&source_path, &plugin());
        assert_eq!(validation.status, CacheStatus::InvalidText);
    }

    #[test]
    fn standalone_validation_accepts_valid_generated_text_without_plugin() {
        let temp = tempdir().unwrap();
        let source_path = temp.path().join("valid.pdf");
        let text_path = temp.path().join("valid.pdf.sm.txt");
        let meta_path = temp.path().join("valid.pdf.sm.meta");

        fs::write(&source_path, b"pdf").unwrap();
        fs::write(&text_path, b"hello world").unwrap();
        let source_mtime =
            OffsetDateTime::from(fs::metadata(&source_path).unwrap().modified().unwrap())
                .format(&Rfc3339)
                .unwrap();

        fs::write(
            &meta_path,
            format!(
                r#"{{
                  "schema": "sm.meta.v1",
                  "source": {{
                    "path": "{}",
                    "size": 3,
                    "mtime": "{}"
                  }},
                  "generator": {{
                    "plugin_id": "sm.plugin.pdf",
                    "plugin_version": "9.9.9"
                  }},
                  "text": {{
                    "path": "{}",
                    "encoding": "utf-8",
                    "length_bytes": 11
                  }},
                  "ranges": [
                    {{ "type": "page", "start": 0, "end": 11, "page": 8 }}
                  ]
                }}"#,
                source_path.display(),
                source_mtime,
                text_path.display()
            ),
        )
        .unwrap();

        let validation = validate_generated_text_standalone(&text_path);
        assert_eq!(validation.status, CacheStatus::Ready);
    }

    #[test]
    fn accepts_relative_paths_in_meta_when_colocated() {
        let temp = tempdir().unwrap();
        let source_path = temp.path().join("valid.pdf");
        let text_path = temp.path().join("valid.pdf.sm.txt");
        let meta_path = temp.path().join("valid.pdf.sm.meta");

        fs::write(&source_path, b"pdf").unwrap();
        fs::write(&text_path, b"hello world").unwrap();
        let source_mtime =
            OffsetDateTime::from(fs::metadata(&source_path).unwrap().modified().unwrap())
                .format(&Rfc3339)
                .unwrap();

        fs::write(
            &meta_path,
            format!(
                r#"{{
                  "schema": "sm.meta.v1",
                  "source": {{
                    "path": "valid.pdf",
                    "size": 3,
                    "mtime": "{}"
                  }},
                  "generator": {{
                    "plugin_id": "sm.plugin.pdf",
                    "plugin_version": "1.2.3"
                  }},
                  "text": {{
                    "path": "valid.pdf.sm.txt",
                    "encoding": "utf-8",
                    "length_bytes": 11
                  }},
                  "ranges": [
                    {{ "type": "page", "start": 0, "end": 11, "page": 8 }}
                  ]
                }}"#,
                source_mtime
            ),
        )
        .unwrap();

        let validation = validate_cache(&source_path, &plugin());
        assert_eq!(validation.status, CacheStatus::Ready);
    }

    #[test]
    fn mtime_match_uses_utc_second_precision() {
        let actual = OffsetDateTime::parse("2026-05-10T12:16:15.295727Z", &Rfc3339)
            .unwrap()
            .into();

        assert!(super::mtimes_match("2026-05-10T12:16:15Z", actual));
        assert!(super::mtimes_match("2026-05-10T13:16:15+01:00", actual));
        assert!(!super::mtimes_match("2026-05-10T12:16:16Z", actual));
    }

    #[test]
    fn validation_accepts_subsecond_mtime_difference_with_same_second() {
        let temp = tempdir().unwrap();
        let source_path = temp.path().join("valid.pdf");
        let text_path = temp.path().join("valid.pdf.sm.txt");
        let meta_path = temp.path().join("valid.pdf.sm.meta");

        fs::write(&source_path, b"pdf").unwrap();
        fs::write(&text_path, b"hello world").unwrap();
        let source_mtime =
            OffsetDateTime::from(fs::metadata(&source_path).unwrap().modified().unwrap());
        let rounded_source_mtime = source_mtime
            .replace_nanosecond(0)
            .unwrap()
            .format(&Rfc3339)
            .unwrap();

        fs::write(
            &meta_path,
            format!(
                r#"{{
                  "schema": "sm.meta.v1",
                  "source": {{
                    "path": "valid.pdf",
                    "size": 3,
                    "mtime": "{}"
                  }},
                  "generator": {{
                    "plugin_id": "sm.plugin.pdf",
                    "plugin_version": "1.2.3"
                  }},
                  "text": {{
                    "path": "valid.pdf.sm.txt",
                    "encoding": "utf-8",
                    "length_bytes": 11
                  }},
                  "ranges": [
                    {{ "type": "page", "start": 0, "end": 11, "page": 8 }}
                  ]
                }}"#,
                rounded_source_mtime
            ),
        )
        .unwrap();

        let validation = validate_cache(&source_path, &plugin());
        assert_eq!(validation.status, CacheStatus::Ready);
    }

    #[test]
    fn source_hash_fallback_accepts_mtime_mismatch_when_content_matches() {
        let temp = tempdir().unwrap();
        let source_path = temp.path().join("report.pdf");
        let text_path = temp.path().join("report.pdf.sm.txt");
        let meta_path = temp.path().join("report.pdf.sm.meta");

        fs::write(&source_path, b"pdf").unwrap();
        fs::write(&text_path, b"hello world").unwrap();
        let text_mtime =
            OffsetDateTime::from(fs::metadata(&text_path).unwrap().modified().unwrap())
                .format(&Rfc3339)
                .unwrap();

        fs::write(
            &meta_path,
            format!(
                r#"{{
                  "schema": "sm.meta.v1",
                  "source": {{
                    "path": "report.pdf",
                    "size": 999,
                    "mtime": "2026-05-10T00:00:00Z",
                    "hash": "{}"
                  }},
                  "generator": {{
                    "plugin_id": "sm.plugin.pdf",
                    "plugin_version": "1.2.3"
                  }},
                  "text": {{
                    "path": "report.pdf.sm.txt",
                    "encoding": "utf-8",
                    "length_bytes": 11,
                    "mtime": "{}"
                  }},
                  "ranges": [
                    {{ "type": "page", "start": 0, "end": 11, "page": 1 }}
                  ]
                }}"#,
                sha256_of(b"pdf"),
                text_mtime
            ),
        )
        .unwrap();

        let validation = validate_cache(&source_path, &plugin());
        assert_eq!(validation.status, CacheStatus::Ready);
    }

    #[test]
    fn text_hash_fallback_accepts_metadata_mismatch_when_content_matches() {
        let temp = tempdir().unwrap();
        let source_path = temp.path().join("report.pdf");
        let text_path = temp.path().join("report.pdf.sm.txt");
        let meta_path = temp.path().join("report.pdf.sm.meta");

        fs::write(&source_path, b"pdf").unwrap();
        fs::write(&text_path, b"hello world").unwrap();
        let source_mtime =
            OffsetDateTime::from(fs::metadata(&source_path).unwrap().modified().unwrap())
                .format(&Rfc3339)
                .unwrap();

        fs::write(
            &meta_path,
            format!(
                r#"{{
                  "schema": "sm.meta.v1",
                  "source": {{
                    "path": "report.pdf",
                    "size": 3,
                    "mtime": "{}"
                  }},
                  "generator": {{
                    "plugin_id": "sm.plugin.pdf",
                    "plugin_version": "1.2.3"
                  }},
                  "text": {{
                    "path": "report.pdf.sm.txt",
                    "encoding": "utf-8",
                    "length_bytes": 99,
                    "mtime": "2026-05-10T00:00:00Z",
                    "hash": "{}"
                  }},
                  "ranges": [
                    {{ "type": "page", "start": 0, "end": 11, "page": 1 }}
                  ]
                }}"#,
                source_mtime,
                sha256_of(b"hello world")
            ),
        )
        .unwrap();

        let validation = validate_cache(&source_path, &plugin());
        assert_eq!(validation.status, CacheStatus::Ready);
    }

    #[test]
    fn text_mismatch_without_hash_still_fails_fast() {
        let temp = tempdir().unwrap();
        let source_path = temp.path().join("report.pdf");
        let text_path = temp.path().join("report.pdf.sm.txt");
        let meta_path = temp.path().join("report.pdf.sm.meta");

        fs::write(&source_path, b"pdf").unwrap();
        fs::write(&text_path, b"hello world").unwrap();
        let source_mtime =
            OffsetDateTime::from(fs::metadata(&source_path).unwrap().modified().unwrap())
                .format(&Rfc3339)
                .unwrap();

        fs::write(
            &meta_path,
            format!(
                r#"{{
                  "schema": "sm.meta.v1",
                  "source": {{
                    "path": "report.pdf",
                    "size": 3,
                    "mtime": "{}"
                  }},
                  "generator": {{
                    "plugin_id": "sm.plugin.pdf",
                    "plugin_version": "1.2.3"
                  }},
                  "text": {{
                    "path": "report.pdf.sm.txt",
                    "encoding": "utf-8",
                    "length_bytes": 99,
                    "mtime": "2026-05-10T00:00:00Z"
                  }},
                  "ranges": [
                    {{ "type": "page", "start": 0, "end": 11, "page": 1 }}
                  ]
                }}"#,
                source_mtime
            ),
        )
        .unwrap();

        let validation = validate_cache(&source_path, &plugin());
        assert_eq!(validation.status, CacheStatus::InvalidText);
    }

    fn plugin() -> RegisteredPlugin {
        RegisteredPlugin {
            id: "sm.plugin.pdf".to_string(),
            name: "PDF Plugin".to_string(),
            version: "1.2.3".to_string(),
            root_dir: PathBuf::from("/plugins/sm.plugin.pdf"),
            command: PathBuf::from("/plugins/sm.plugin.pdf/bin/linux-x64/sm-plugin-pdf"),
            args: vec!["--job".to_string()],
            handles: vec![".pdf".to_string()],
            requires_entitlement: false,
            timeout_seconds: 60,
            capabilities: PluginCapabilities::default(),
            permissions: vec![
                PluginPermission::ReadSourceFile,
                PluginPermission::WriteSmOutputs,
            ],
        }
    }

    fn sha256_of(bytes: &[u8]) -> String {
        format!("sha256:{:x}", Sha256::digest(bytes))
    }
}
