use crate::plugins::index_paths::{default_state_db_path, mirror_meta_path, mirror_text_path};
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const STATUS_READY: &str = "ready";
const STATUS_QUEUED: &str = "queued";
const STATUS_PROCESSING: &str = "processing";
const STATUS_FAILED: &str = "failed";
const STATUS_STALE: &str = "stale";
const STATUS_MISSING: &str = "missing";
const STATUS_SKIPPED: &str = "skipped";
const STATUS_IGNORED: &str = "ignored";

#[derive(Debug, Clone, Default)]
pub struct PluginCounts {
    pub indexed_count: usize,
    pub attention_count: usize,
    pub ignored_count: usize,
    pub queued_count: usize,
    pub processing_count: usize,
    pub blocked_count: usize,
}

#[derive(Debug, Clone)]
pub struct IndexedFileRow {
    pub source_path: String,
    pub plugin_id: String,
    pub source_size: i64,
    pub source_mtime: String,
    pub cache_text_path: Option<String>,
    pub cache_meta_path: Option<String>,
    pub status: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub error_hint: Option<String>,
    pub attempts: u32,
    pub retry_after: Option<String>,
    pub plugin_version: String,
    pub indexed_at: Option<String>,
    pub checked_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct PluginIssueRow {
    pub source_path: String,
    pub plugin_id: String,
    pub status: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub error_hint: Option<String>,
    pub attempts: u32,
    pub retry_after: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct PluginIssueCountRow {
    pub status: String,
    pub error_code: String,
    pub count: usize,
}

#[derive(Debug, Clone)]
pub struct RetryReadyRow {
    pub source_path: String,
    pub plugin_id: String,
    pub attempts: u32,
}

#[derive(Debug, Clone)]
pub struct RecoverableJobRow {
    pub source_path: String,
    pub plugin_id: String,
    pub attempts: u32,
}

#[derive(Debug, Clone)]
pub struct PluginRunRecord {
    pub id: String,
    pub plugin_id: String,
    pub source_path: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PluginIssuePreference {
    pub plugin_id: String,
    pub error_code: String,
}

#[derive(Debug, Clone)]
pub struct StateDb {
    path: PathBuf,
    index_root: PathBuf,
}

impl StateDb {
    pub fn new(index_roots: &[PathBuf]) -> Result<Self> {
        let index_root = index_roots
            .first()
            .cloned()
            .context("no plugin index root configured")?;
        let path =
            default_state_db_path().unwrap_or_else(|| index_root.join("searchmonkey.sqlite"));

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed creating sqlite directory {}", parent.display())
            })?;
        }

        let db = Self { path, index_root };
        db.init()?;
        Ok(db)
    }

    pub fn clear_all(&self) -> Result<()> {
        let conn = self.open()?;
        conn.execute("DELETE FROM indexed_files", [])?;
        conn.execute("DELETE FROM plugin_runs", [])?;
        conn.execute("DELETE FROM plugin_issue_preferences", [])?;
        conn.execute("DELETE FROM scan_roots", [])?;
        Ok(())
    }

    pub fn clear_plugin(&self, plugin_id: &str) -> Result<()> {
        let conn = self.open()?;
        conn.execute(
            "DELETE FROM indexed_files WHERE plugin_id = ?1",
            params![plugin_id],
        )?;
        conn.execute(
            "DELETE FROM plugin_runs WHERE plugin_id = ?1",
            params![plugin_id],
        )?;
        conn.execute(
            "DELETE FROM plugin_issue_preferences WHERE plugin_id = ?1",
            params![plugin_id],
        )?;
        Ok(())
    }

    pub fn preferred_plugin_versions(&self) -> Result<HashMap<String, String>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare("SELECT plugin_id, active_version FROM plugin_preferences")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<std::result::Result<HashMap<_, _>, _>>()
            .map_err(Into::into)
    }

    pub fn disabled_plugin_ids(&self) -> Result<Vec<String>> {
        let conn = self.open()?;
        let mut stmt =
            conn.prepare("SELECT plugin_id FROM plugin_preferences WHERE enabled = 0")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn set_issue_auto_ignore(
        &self,
        plugin_id: &str,
        error_code: &str,
        enabled: bool,
    ) -> Result<()> {
        let conn = self.open()?;
        if enabled {
            conn.execute(
                "INSERT INTO plugin_issue_preferences (plugin_id, error_code, auto_ignore)
                 VALUES (?1, ?2, 1)
                 ON CONFLICT(plugin_id, error_code) DO UPDATE SET auto_ignore = 1",
                params![plugin_id, error_code],
            )?;
        } else {
            conn.execute(
                "DELETE FROM plugin_issue_preferences WHERE plugin_id = ?1 AND error_code = ?2",
                params![plugin_id, error_code],
            )?;
        }
        Ok(())
    }

    pub fn list_issue_preferences(&self) -> Result<Vec<PluginIssuePreference>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT plugin_id, error_code
             FROM plugin_issue_preferences
             WHERE auto_ignore = 1
             ORDER BY plugin_id ASC, error_code ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(PluginIssuePreference {
                plugin_id: row.get(0)?,
                error_code: row.get(1)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn ignore_issue_type(&self, plugin_id: &str, error_code: &str) -> Result<usize> {
        let conn = self.open()?;
        conn.execute(
            "UPDATE indexed_files
             SET status = ?3,
                 retry_after = NULL,
                 checked_at = ?4,
                 updated_at = ?4
             WHERE plugin_id = ?1
               AND COALESCE(error_code, status) = ?2
               AND status IN ('failed', 'missing', 'skipped')",
            params![plugin_id, error_code, STATUS_IGNORED, now_rfc3339()],
        )
        .map_err(Into::into)
    }

    pub fn set_preferred_plugin_version(&self, plugin_id: &str, version: &str) -> Result<()> {
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO plugin_preferences (plugin_id, active_version, enabled)
             VALUES (?1, ?2, 1)
             ON CONFLICT(plugin_id) DO UPDATE SET active_version = excluded.active_version",
            params![plugin_id, version],
        )?;
        Ok(())
    }

    pub fn set_plugin_enabled(&self, plugin_id: &str, enabled: bool) -> Result<()> {
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO plugin_preferences (plugin_id, active_version, enabled)
             VALUES (?1, COALESCE((SELECT active_version FROM plugin_preferences WHERE plugin_id = ?1), ''), ?2)
             ON CONFLICT(plugin_id) DO UPDATE SET enabled = excluded.enabled",
            params![plugin_id, if enabled { 1 } else { 0 }],
        )?;
        Ok(())
    }

    pub fn clear_preferred_plugin_version(&self, plugin_id: &str) -> Result<()> {
        let conn = self.open()?;
        conn.execute(
            "DELETE FROM plugin_preferences WHERE plugin_id = ?1",
            params![plugin_id],
        )?;
        Ok(())
    }

    pub fn get_indexed_file(
        &self,
        source_path: &Path,
        plugin_id: &str,
    ) -> Result<Option<IndexedFileRow>> {
        let conn = self.open()?;
        conn.query_row(
            "SELECT source_path, plugin_id, source_size, source_mtime, cache_text_path, cache_meta_path, \
                    status, error_code, error_message, error_hint, attempts, retry_after, plugin_version, \
                    indexed_at, checked_at, updated_at \
             FROM indexed_files WHERE source_path = ?1 AND plugin_id = ?2",
            params![source_path.display().to_string(), plugin_id],
            map_indexed_file_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_root_rows(&self, root_path: &Path) -> Result<Vec<IndexedFileRow>> {
        let conn = self.open()?;
        let root_string = root_path.display().to_string();
        let mut stmt = conn.prepare(
            "SELECT source_path, plugin_id, source_size, source_mtime, cache_text_path, cache_meta_path, \
                    status, error_code, error_message, error_hint, attempts, retry_after, plugin_version, \
                    indexed_at, checked_at, updated_at \
             FROM indexed_files WHERE source_path = ?1 OR source_path LIKE ?2",
        )?;
        let rows = stmt.query_map(
            params![root_string, format!("{}/%", root_path.display())],
            map_indexed_file_row,
        )?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn list_plugin_rows(&self, plugin_id: &str) -> Result<Vec<IndexedFileRow>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT source_path, plugin_id, source_size, source_mtime, cache_text_path, cache_meta_path, \
                    status, error_code, error_message, error_hint, attempts, retry_after, plugin_version, \
                    indexed_at, checked_at, updated_at \
             FROM indexed_files WHERE plugin_id = ?1",
        )?;
        let rows = stmt.query_map(params![plugin_id], map_indexed_file_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn upsert_discovered_file(
        &self,
        source_path: &Path,
        plugin_id: &str,
        plugin_version: &str,
        source_size: i64,
        source_mtime: &str,
        status: &str,
        attempts: u32,
    ) -> Result<()> {
        let now = now_rfc3339();
        let source_path_string = source_path.display().to_string();
        let text_path = mirror_text_path(&self.index_root, source_path)
            .display()
            .to_string();
        let meta_path = mirror_meta_path(&self.index_root, source_path)
            .display()
            .to_string();
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO indexed_files (
                source_path, plugin_id, source_size, source_mtime, cache_text_path, cache_meta_path,
                status, error_code, error_message, error_hint, attempts, retry_after, plugin_version,
                indexed_at, checked_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, NULL, ?8, NULL, ?9, NULL, ?10, ?10)
            ON CONFLICT(source_path, plugin_id) DO UPDATE SET
                source_size = excluded.source_size,
                source_mtime = excluded.source_mtime,
                cache_text_path = excluded.cache_text_path,
                cache_meta_path = excluded.cache_meta_path,
                status = excluded.status,
                error_code = NULL,
                error_message = NULL,
                error_hint = NULL,
                attempts = excluded.attempts,
                retry_after = NULL,
                plugin_version = excluded.plugin_version,
                checked_at = excluded.checked_at,
                updated_at = excluded.updated_at",
            params![
                source_path_string,
                plugin_id,
                source_size,
                source_mtime,
                text_path,
                meta_path,
                status,
                attempts,
                plugin_version,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn mark_queued(
        &self,
        source_path: &Path,
        plugin_id: &str,
        plugin_version: &str,
        source_size: i64,
        source_mtime: &str,
        attempts: u32,
    ) -> Result<()> {
        let now = now_rfc3339();
        let text_path = mirror_text_path(&self.index_root, source_path)
            .display()
            .to_string();
        let meta_path = mirror_meta_path(&self.index_root, source_path)
            .display()
            .to_string();
        let conn = self.open()?;
        conn.execute(
            "UPDATE indexed_files
             SET source_size = ?3,
                 source_mtime = ?4,
                 cache_text_path = ?5,
                 cache_meta_path = ?6,
                 status = ?7,
                 attempts = ?8,
                 retry_after = NULL,
                 plugin_version = ?9,
                 checked_at = ?10,
                 updated_at = ?10
             WHERE source_path = ?1 AND plugin_id = ?2",
            params![
                source_path.display().to_string(),
                plugin_id,
                source_size,
                source_mtime,
                text_path,
                meta_path,
                STATUS_QUEUED,
                attempts,
                plugin_version,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn touch_checked_at(&self, source_path: &Path, plugin_id: &str) -> Result<()> {
        let conn = self.open()?;
        conn.execute(
            "UPDATE indexed_files SET checked_at = ?3 WHERE source_path = ?1 AND plugin_id = ?2",
            params![source_path.display().to_string(), plugin_id, now_rfc3339()],
        )?;
        Ok(())
    }

    pub fn mark_ready(
        &self,
        source_path: &Path,
        plugin_id: &str,
        plugin_version: &str,
        source_size: i64,
        source_mtime: &str,
        attempts: u32,
    ) -> Result<()> {
        let now = now_rfc3339();
        let text_path = mirror_text_path(&self.index_root, source_path)
            .display()
            .to_string();
        let meta_path = mirror_meta_path(&self.index_root, source_path)
            .display()
            .to_string();
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO indexed_files (
                source_path, plugin_id, source_size, source_mtime, cache_text_path, cache_meta_path,
                status, error_code, error_message, error_hint, attempts, retry_after, plugin_version,
                indexed_at, checked_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, NULL, ?8, NULL, ?9, ?10, ?10, ?10)
            ON CONFLICT(source_path, plugin_id) DO UPDATE SET
                source_size = excluded.source_size,
                source_mtime = excluded.source_mtime,
                cache_text_path = excluded.cache_text_path,
                cache_meta_path = excluded.cache_meta_path,
                status = excluded.status,
                error_code = NULL,
                error_message = NULL,
                error_hint = NULL,
                attempts = excluded.attempts,
                retry_after = NULL,
                plugin_version = excluded.plugin_version,
                indexed_at = excluded.indexed_at,
                checked_at = excluded.checked_at,
                updated_at = excluded.updated_at",
            params![
                source_path.display().to_string(),
                plugin_id,
                source_size,
                source_mtime,
                text_path,
                meta_path,
                STATUS_READY,
                attempts,
                plugin_version,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn mark_processing(
        &self,
        source_path: &Path,
        plugin_id: &str,
        attempts: u32,
    ) -> Result<()> {
        let conn = self.open()?;
        conn.execute(
            "UPDATE indexed_files
             SET status = ?3,
                 attempts = ?4,
                 retry_after = NULL,
                 checked_at = ?5,
                 updated_at = ?5
             WHERE source_path = ?1 AND plugin_id = ?2",
            params![
                source_path.display().to_string(),
                plugin_id,
                STATUS_PROCESSING,
                attempts,
                now_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn mark_failed(
        &self,
        source_path: &Path,
        plugin_id: &str,
        attempts: u32,
        error_code: &str,
        error_message: &str,
        error_hint: Option<&str>,
        retry_after: Option<&str>,
    ) -> Result<()> {
        self.update_status(
            source_path,
            plugin_id,
            STATUS_FAILED,
            Some(error_code),
            Some(error_message),
            error_hint,
            attempts,
            retry_after,
        )
    }

    pub fn mark_missing(&self, source_path: &Path, plugin_id: &str, attempts: u32) -> Result<()> {
        self.update_status(
            source_path,
            plugin_id,
            STATUS_MISSING,
            Some("missing_source"),
            Some("Source file is missing"),
            None,
            attempts,
            None,
        )
    }

    pub fn mark_skipped(
        &self,
        source_path: &Path,
        plugin_id: &str,
        attempts: u32,
        message: &str,
    ) -> Result<()> {
        self.update_status(
            source_path,
            plugin_id,
            STATUS_SKIPPED,
            Some("skipped"),
            Some(message),
            None,
            attempts,
            None,
        )
    }

    pub fn mark_ignored(&self, source_path: &Path, plugin_id: &str, attempts: u32) -> Result<()> {
        let conn = self.open()?;
        conn.execute(
            "UPDATE indexed_files
             SET status = ?3,
                 attempts = ?4,
                 retry_after = NULL,
                 checked_at = ?5,
                 updated_at = ?5
             WHERE source_path = ?1 AND plugin_id = ?2",
            params![
                source_path.display().to_string(),
                plugin_id,
                STATUS_IGNORED,
                attempts,
                now_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn sync_ignored_metadata(
        &self,
        source_path: &Path,
        plugin_id: &str,
        plugin_version: &str,
        source_size: i64,
        source_mtime: &str,
    ) -> Result<()> {
        let text_path = mirror_text_path(&self.index_root, source_path)
            .display()
            .to_string();
        let meta_path = mirror_meta_path(&self.index_root, source_path)
            .display()
            .to_string();
        let conn = self.open()?;
        conn.execute(
            "UPDATE indexed_files
             SET source_size = ?3,
                 source_mtime = ?4,
                 cache_text_path = ?5,
                 cache_meta_path = ?6,
                 plugin_version = ?7,
                 checked_at = ?8
             WHERE source_path = ?1 AND plugin_id = ?2 AND status = ?9",
            params![
                source_path.display().to_string(),
                plugin_id,
                source_size,
                source_mtime,
                text_path,
                meta_path,
                plugin_version,
                now_rfc3339(),
                STATUS_IGNORED,
            ],
        )?;
        Ok(())
    }

    pub fn has_issue_auto_ignore(&self, plugin_id: &str, error_code: &str) -> Result<bool> {
        let conn = self.open()?;
        conn.query_row(
            "SELECT 1
             FROM plugin_issue_preferences
             WHERE plugin_id = ?1 AND error_code = ?2 AND auto_ignore = 1
             LIMIT 1",
            params![plugin_id, error_code],
            |_| Ok(()),
        )
        .optional()
        .map(|value| value.is_some())
        .map_err(Into::into)
    }

    pub fn mark_stale(
        &self,
        source_path: &Path,
        plugin_id: &str,
        attempts: u32,
        error_message: Option<&str>,
    ) -> Result<()> {
        self.update_status(
            source_path,
            plugin_id,
            STATUS_STALE,
            Some("stale_source"),
            error_message,
            None,
            attempts,
            None,
        )
    }

    pub fn list_plugin_counts(
        &self,
        plugin_ids: &[String],
    ) -> Result<HashMap<String, PluginCounts>> {
        let conn = self.open()?;
        let mut map = HashMap::new();

        for plugin_id in plugin_ids {
            let counts = conn.query_row(
                "SELECT
                    SUM(CASE WHEN status = 'ready' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status IN ('failed', 'missing', 'skipped') THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status = 'ignored' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status = 'queued' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status = 'processing' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status IN ('failed', 'missing', 'skipped') AND attempts >= 4 THEN 1 ELSE 0 END)
                 FROM indexed_files WHERE plugin_id = ?1",
                params![plugin_id],
                |row| {
                    Ok(PluginCounts {
                        indexed_count: row.get::<_, Option<i64>>(0)?.unwrap_or(0) as usize,
                        attention_count: row.get::<_, Option<i64>>(1)?.unwrap_or(0) as usize,
                        ignored_count: row.get::<_, Option<i64>>(2)?.unwrap_or(0) as usize,
                        queued_count: row.get::<_, Option<i64>>(3)?.unwrap_or(0) as usize,
                        processing_count: row.get::<_, Option<i64>>(4)?.unwrap_or(0) as usize,
                        blocked_count: row.get::<_, Option<i64>>(5)?.unwrap_or(0) as usize,
                    })
                },
            )?;
            map.insert(plugin_id.clone(), counts);
        }

        Ok(map)
    }

    pub fn list_plugin_issues(&self, plugin_id: &str) -> Result<Vec<PluginIssueRow>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT source_path, plugin_id, status, error_code, error_message, error_hint, attempts, retry_after, updated_at
             FROM indexed_files
             WHERE plugin_id = ?1 AND status IN ('failed', 'missing', 'skipped', 'ignored')
             ORDER BY updated_at ASC, source_path ASC",
        )?;
        let rows = stmt.query_map(params![plugin_id], |row| {
            Ok(PluginIssueRow {
                source_path: row.get(0)?,
                plugin_id: row.get(1)?,
                status: row.get(2)?,
                error_code: row.get(3)?,
                error_message: row.get(4)?,
                error_hint: row.get(5)?,
                attempts: row.get::<_, i64>(6)? as u32,
                retry_after: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn list_plugin_issue_counts(&self, plugin_id: &str) -> Result<Vec<PluginIssueCountRow>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT status, COALESCE(error_code, status), COUNT(*)
             FROM indexed_files
             WHERE plugin_id = ?1 AND status IN ('failed', 'missing', 'skipped', 'ignored')
             GROUP BY status, COALESCE(error_code, status)
             ORDER BY COUNT(*) DESC, COALESCE(error_code, status) ASC",
        )?;
        let rows = stmt.query_map(params![plugin_id], |row| {
            Ok(PluginIssueCountRow {
                status: row.get(0)?,
                error_code: row.get(1)?,
                count: row.get::<_, i64>(2)? as usize,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn list_plugin_issues_page(
        &self,
        plugin_id: &str,
        status: Option<&str>,
        error_code: Option<&str>,
        limit: usize,
    ) -> Result<Vec<PluginIssueRow>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT source_path, plugin_id, status, error_code, error_message, error_hint, attempts, retry_after, updated_at
             FROM indexed_files
             WHERE plugin_id = ?1
               AND status IN ('failed', 'missing', 'skipped', 'ignored')
               AND (?2 IS NULL OR status = ?2)
               AND (?3 IS NULL OR COALESCE(error_code, status) = ?3)
             ORDER BY updated_at ASC, source_path ASC
             LIMIT ?4",
        )?;
        let rows = stmt.query_map(
            params![plugin_id, status, error_code, limit as i64],
            |row| {
                Ok(PluginIssueRow {
                    source_path: row.get(0)?,
                    plugin_id: row.get(1)?,
                    status: row.get(2)?,
                    error_code: row.get(3)?,
                    error_message: row.get(4)?,
                    error_hint: row.get(5)?,
                    attempts: row.get::<_, i64>(6)? as u32,
                    retry_after: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            },
        )?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn list_retry_ready(&self, limit: usize) -> Result<Vec<RetryReadyRow>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT source_path, plugin_id, attempts
             FROM indexed_files
             WHERE status = 'failed'
               AND retry_after IS NOT NULL
               AND attempts < 4
               AND retry_after <= ?1
             ORDER BY retry_after ASC, source_path ASC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![now_rfc3339(), limit as i64], |row| {
            Ok(RetryReadyRow {
                source_path: row.get(0)?,
                plugin_id: row.get(1)?,
                attempts: row.get::<_, i64>(2)? as u32,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn list_recoverable_jobs(&self, limit: usize) -> Result<Vec<RecoverableJobRow>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT source_path, plugin_id, attempts
             FROM indexed_files
             WHERE status IN ('queued', 'processing')
             ORDER BY updated_at ASC, source_path ASC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(RecoverableJobRow {
                source_path: row.get(0)?,
                plugin_id: row.get(1)?,
                attempts: row.get::<_, i64>(2)? as u32,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn upsert_scan_root(&self, root_path: &Path) -> Result<()> {
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO scan_roots (root_path, last_scan_started_at, last_scan_completed_at, last_seen_file_count)
             VALUES (?1, NULL, NULL, 0)
             ON CONFLICT(root_path) DO NOTHING",
            params![root_path.display().to_string()],
        )?;
        Ok(())
    }

    pub fn mark_scan_root_started(&self, root_path: &Path) -> Result<()> {
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO scan_roots (root_path, last_scan_started_at, last_scan_completed_at, last_seen_file_count)
             VALUES (?1, ?2, NULL, 0)
             ON CONFLICT(root_path) DO UPDATE SET last_scan_started_at = excluded.last_scan_started_at",
            params![root_path.display().to_string(), now_rfc3339()],
        )?;
        Ok(())
    }

    pub fn mark_scan_root_completed(&self, root_path: &Path, file_count: usize) -> Result<()> {
        let conn = self.open()?;
        conn.execute(
            "UPDATE scan_roots
             SET last_scan_completed_at = ?2, last_seen_file_count = ?3
             WHERE root_path = ?1",
            params![
                root_path.display().to_string(),
                now_rfc3339(),
                file_count as i64
            ],
        )?;
        Ok(())
    }

    pub fn list_scan_roots(&self) -> Result<Vec<PathBuf>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare("SELECT root_path FROM scan_roots ORDER BY root_path ASC")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| row.map(PathBuf::from))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn start_plugin_run(&self, run: &PluginRunRecord) -> Result<()> {
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO plugin_runs (id, plugin_id, source_path, started_at, finished_at, status, error_code, error_message)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                run.id,
                run.plugin_id,
                run.source_path,
                run.started_at,
                run.finished_at,
                run.status,
                run.error_code,
                run.error_message
            ],
        )?;
        Ok(())
    }

    pub fn finish_plugin_run(
        &self,
        run_id: &str,
        status: &str,
        error_code: Option<&str>,
        error_message: Option<&str>,
    ) -> Result<()> {
        let conn = self.open()?;
        conn.execute(
            "UPDATE plugin_runs
             SET finished_at = ?2, status = ?3, error_code = ?4, error_message = ?5
             WHERE id = ?1",
            params![run_id, now_rfc3339(), status, error_code, error_message],
        )?;
        Ok(())
    }

    fn init(&self) -> Result<()> {
        let conn = self.open()?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS indexed_files (
               source_path TEXT NOT NULL,
               plugin_id TEXT NOT NULL,
               source_size INTEGER NOT NULL,
               source_mtime TEXT NOT NULL,
               cache_text_path TEXT,
               cache_meta_path TEXT,
               status TEXT NOT NULL,
               error_code TEXT,
               error_message TEXT,
               error_hint TEXT,
               attempts INTEGER NOT NULL DEFAULT 0,
               retry_after TEXT,
               plugin_version TEXT NOT NULL,
               indexed_at TEXT,
               checked_at TEXT NOT NULL,
               updated_at TEXT NOT NULL,
               PRIMARY KEY (source_path, plugin_id)
             );
             CREATE TABLE IF NOT EXISTS scan_roots (
               root_path TEXT PRIMARY KEY,
               last_scan_started_at TEXT,
               last_scan_completed_at TEXT,
               last_seen_file_count INTEGER DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS plugin_runs (
               id TEXT PRIMARY KEY,
               plugin_id TEXT NOT NULL,
               source_path TEXT NOT NULL,
               started_at TEXT NOT NULL,
               finished_at TEXT,
               status TEXT NOT NULL,
               error_code TEXT,
               error_message TEXT
             );
             CREATE TABLE IF NOT EXISTS plugin_preferences (
               plugin_id TEXT PRIMARY KEY,
               active_version TEXT NOT NULL,
               enabled INTEGER NOT NULL DEFAULT 1
             );
             CREATE TABLE IF NOT EXISTS plugin_issue_preferences (
               plugin_id TEXT NOT NULL,
               error_code TEXT NOT NULL,
               auto_ignore INTEGER NOT NULL DEFAULT 1,
               PRIMARY KEY (plugin_id, error_code)
             );
             CREATE INDEX IF NOT EXISTS idx_indexed_files_plugin_status ON indexed_files(plugin_id, status);
             CREATE INDEX IF NOT EXISTS idx_indexed_files_root_path ON indexed_files(source_path);
             CREATE INDEX IF NOT EXISTS idx_plugin_runs_plugin ON plugin_runs(plugin_id, started_at);",
        )?;
        let _ = conn.execute(
            "ALTER TABLE plugin_preferences ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1",
            [],
        );
        Ok(())
    }

    fn open(&self) -> Result<Connection> {
        Connection::open(&self.path).with_context(|| {
            format!(
                "failed opening plugin sqlite database {}",
                self.path.display()
            )
        })
    }

    fn update_status(
        &self,
        source_path: &Path,
        plugin_id: &str,
        status: &str,
        error_code: Option<&str>,
        error_message: Option<&str>,
        error_hint: Option<&str>,
        attempts: u32,
        retry_after: Option<&str>,
    ) -> Result<()> {
        let conn = self.open()?;
        let effective_status = if matches!(status, STATUS_FAILED | STATUS_MISSING | STATUS_SKIPPED) {
            match error_code {
                Some(code) if self.has_issue_auto_ignore(plugin_id, code)? => STATUS_IGNORED,
                _ => status,
            }
        } else {
            status
        };
        conn.execute(
            "UPDATE indexed_files
             SET status = ?3,
                 error_code = ?4,
                 error_message = ?5,
                 error_hint = ?6,
                 attempts = ?7,
                 retry_after = ?8,
                 checked_at = ?9,
                 updated_at = ?9
             WHERE source_path = ?1 AND plugin_id = ?2",
            params![
                source_path.display().to_string(),
                plugin_id,
                effective_status,
                error_code,
                error_message,
                error_hint,
                attempts,
                retry_after,
                now_rfc3339(),
            ],
        )?;
        Ok(())
    }
}

fn map_indexed_file_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<IndexedFileRow> {
    Ok(IndexedFileRow {
        source_path: row.get(0)?,
        plugin_id: row.get(1)?,
        source_size: row.get(2)?,
        source_mtime: row.get(3)?,
        cache_text_path: row.get(4)?,
        cache_meta_path: row.get(5)?,
        status: row.get(6)?,
        error_code: row.get(7)?,
        error_message: row.get(8)?,
        error_hint: row.get(9)?,
        attempts: row.get::<_, i64>(10)? as u32,
        retry_after: row.get(11)?,
        plugin_version: row.get(12)?,
        indexed_at: row.get(13)?,
        checked_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}

pub fn now_rfc3339() -> String {
    let datetime = OffsetDateTime::from(SystemTime::now())
        .to_offset(time::UtcOffset::UTC)
        .replace_nanosecond(0)
        .expect("zero nanoseconds should be valid");
    datetime
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

pub fn retry_after_for_attempt(attempts: u32) -> Option<String> {
    let seconds = match attempts {
        1 => Some(600),
        2 => Some(3600),
        3 => Some(86_400),
        _ => None,
    }?;
    let retry_at = OffsetDateTime::from(SystemTime::now()) + time::Duration::seconds(seconds);
    retry_at
        .replace_nanosecond(0)
        .ok()
        .and_then(|value| value.format(&Rfc3339).ok())
}

pub fn is_retry_ready(retry_after: Option<&str>) -> bool {
    let Some(retry_after) = retry_after else {
        return false;
    };
    let Ok(retry_at) = OffsetDateTime::parse(retry_after, &Rfc3339) else {
        return true;
    };
    OffsetDateTime::from(SystemTime::now()) >= retry_at
}

pub fn is_attention_status(status: &str) -> bool {
    matches!(
        status,
        STATUS_FAILED | STATUS_STALE | STATUS_MISSING | STATUS_SKIPPED
    )
}

pub fn ignored_status() -> &'static str {
    STATUS_IGNORED
}

pub fn ready_status() -> &'static str {
    STATUS_READY
}

pub fn queued_status() -> &'static str {
    STATUS_QUEUED
}
