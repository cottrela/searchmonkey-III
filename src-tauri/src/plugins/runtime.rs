use crate::plugins::cache::{self, CacheStatus};
use crate::plugins::classifier::{FileClassifier, FileKind};
use crate::plugins::failure_state::{classify_failure, FailureDisplay};
use crate::plugins::index_paths::default_index_roots;
use crate::plugins::indexer::{self, IndexFailure};
use crate::plugins::installer::install_plugin_archive;
use crate::plugins::registry::{
    default_plugin_roots, plugin_version_cmp, plugin_version_satisfies_selected,
    PluginDiscoveryReport, PluginRegistry,
};
use crate::plugins::state_db::{
    is_attention_status, is_retry_ready, now_rfc3339, queued_status, ready_status,
    retry_after_for_attempt, PluginCounts, PluginIssueRow, PluginRunRecord, StateDb,
};
use anyhow::Result;
use ignore::{DirEntry, WalkBuilder};
use serde::Serialize;
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;
#[cfg(test)]
use std::time::Instant;

const WORKER_DELAY: Duration = Duration::from_millis(250);
const RETRY_SWEEP_INTERVAL: Duration = Duration::from_secs(30);
const ACTIVE_QUEUE_TARGET: usize = 16;
const RUN_COUNTER_START: u64 = 1;

#[derive(Debug, Clone, Serialize)]
pub struct PluginIndexStatus {
    pub enabled_plugins: Vec<String>,
    pub installed_plugins: Vec<InstalledPluginInfo>,
    pub indexing_state: String,
    pub plugin_state: String,
    pub paused: bool,
    pub search_active: bool,
    pub scanner_running: bool,
    pub worker_running: bool,
    pub plugin_summaries: Vec<PluginHealthSummary>,
    pub issues: Vec<PluginIssue>,
    pub auto_ignored_issue_types: Vec<PluginIssuePreferenceSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstalledPluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub is_active: bool,
    pub enabled: bool,
    pub requires_entitlement: bool,
    pub handles: Vec<String>,
    pub root_path: String,
    pub capabilities: PluginCapabilitySummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginCapabilitySummary {
    pub text: bool,
    pub layout: bool,
    pub ocr: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginHealthSummary {
    pub plugin_id: String,
    pub indexed_count: usize,
    pub attention_count: usize,
    pub ignored_count: usize,
    pub queued_count: usize,
    pub processing_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginIssue {
    pub source_path: String,
    pub file_name: String,
    pub plugin_id: String,
    pub status: String,
    pub error_code: String,
    pub message: String,
    pub details: String,
    pub attempts: u32,
    pub retry_after: Option<String>,
    pub last_reported_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginIssuePreferenceSummary {
    pub plugin_id: String,
    pub error_code: String,
}

#[derive(Debug, Clone)]
struct PluginJob {
    source_path: PathBuf,
    plugin_id: String,
    attempts: u32,
    run_id: String,
}

#[derive(Debug, Clone)]
struct PluginRefresh {
    root: PathBuf,
    plugin_id: String,
}

#[derive(Default)]
struct RuntimeState {
    pending_roots: VecDeque<PathBuf>,
    queued_roots: HashSet<PathBuf>,
    active_roots: HashSet<PathBuf>,
    pending_refreshes: VecDeque<PluginRefresh>,
    queued_refreshes: HashSet<String>,
    active_refreshes: HashSet<String>,
    pending_jobs: VecDeque<PluginJob>,
    pending_job_keys: HashSet<String>,
    jobs: VecDeque<PluginJob>,
    queued_jobs: HashSet<String>,
    processing_jobs: HashSet<String>,
    paused: bool,
    scanner_running: bool,
    worker_running: bool,
}

struct RuntimeInner {
    state: Mutex<RuntimeState>,
    wake: Condvar,
    plugin_roots: Vec<PathBuf>,
    index_roots: Vec<PathBuf>,
    state_db: StateDb,
    search_active: AtomicUsize,
    run_counter: AtomicU64,
}

#[derive(Clone)]
pub struct PluginIndexRuntime {
    inner: Arc<RuntimeInner>,
}

impl Default for PluginIndexRuntime {
    fn default() -> Self {
        Self::new(default_plugin_roots(), default_index_roots())
    }
}

impl PluginIndexRuntime {
    pub fn new(plugin_roots: Vec<PathBuf>, index_roots: Vec<PathBuf>) -> Self {
        let state_db =
            StateDb::new(&index_roots).expect("plugin sqlite state database should initialize");
        let inner = Arc::new(RuntimeInner {
            state: Mutex::new(RuntimeState::default()),
            wake: Condvar::new(),
            plugin_roots,
            index_roots,
            state_db,
            search_active: AtomicUsize::new(0),
            run_counter: AtomicU64::new(RUN_COUNTER_START),
        });

        recover_queued_jobs(inner.clone());
        spawn_scanner_thread(inner.clone());
        spawn_worker_thread(inner.clone());

        Self { inner }
    }

    pub fn request_scan(&self, root: &Path) -> Result<()> {
        let root = root.canonicalize()?;
        self.inner.state_db.upsert_scan_root(&root)?;
        {
            let mut state = self
                .inner
                .state
                .lock()
                .expect("plugin runtime lock poisoned");
            if state.queued_roots.contains(&root) || state.active_roots.contains(&root) {
                return Ok(());
            }
            state.queued_roots.insert(root.clone());
            state.pending_roots.push_front(root);
        }
        self.inner.wake.notify_all();
        Ok(())
    }

    pub fn request_plugin_refresh(&self, plugin_id: &str) -> Result<()> {
        let plugin_id = plugin_id.trim();
        if plugin_id.is_empty() {
            anyhow::bail!("plugin id is required");
        }

        for root in self.inner.state_db.list_scan_roots()? {
            if !root.exists() {
                continue;
            }

            let key = refresh_key(&root, plugin_id);
            let mut state = self
                .inner
                .state
                .lock()
                .expect("plugin runtime lock poisoned");
            if state.queued_refreshes.contains(&key) || state.active_refreshes.contains(&key) {
                continue;
            }

            state.queued_refreshes.insert(key);
            state.pending_refreshes.push_back(PluginRefresh {
                root,
                plugin_id: plugin_id.to_string(),
            });
        }

        self.inner.wake.notify_all();
        Ok(())
    }

    pub fn request_retry(&self, path: &Path) -> Result<()> {
        let path = path.canonicalize()?;
        let discovery = discovery_report(&self.inner)?;
        let classifier = FileClassifier::new(&discovery.registry);
        let FileKind::SupportedByPlugin { plugin_id } = classifier.classify(&path) else {
            self.request_scan(&path)?;
            return Ok(());
        };

        let attempts = self
            .inner
            .state_db
            .get_indexed_file(&path, &plugin_id)?
            .map(|row| row.attempts.max(1))
            .unwrap_or(1);
        enqueue_job(&self.inner, &path, &plugin_id, attempts, true);
        Ok(())
    }

    pub fn search_started(&self) {
        self.inner.search_active.fetch_add(1, Ordering::SeqCst);
        self.inner.wake.notify_all();
    }

    pub fn search_finished(&self) {
        self.inner.search_active.fetch_sub(1, Ordering::SeqCst);
        self.inner.wake.notify_all();
    }

    pub fn status(&self) -> PluginIndexStatus {
        let installed_plugins = discovered_plugins(self);
        let plugin_ids = installed_plugins
            .iter()
            .map(|plugin| plugin.id.clone())
            .collect::<Vec<_>>();
        let counts = self
            .inner
            .state_db
            .list_plugin_counts(&plugin_ids)
            .unwrap_or_default();
        let issues = plugin_ids
            .iter()
            .flat_map(|plugin_id| {
                self.inner
                    .state_db
                    .list_plugin_issues(plugin_id)
                    .unwrap_or_default()
            })
            .map(map_issue_row)
            .collect::<Vec<_>>();
        let auto_ignored_issue_types = self
            .inner
            .state_db
            .list_issue_preferences()
            .unwrap_or_default()
            .into_iter()
            .map(|preference| PluginIssuePreferenceSummary {
                plugin_id: preference.plugin_id,
                error_code: preference.error_code,
            })
            .collect::<Vec<_>>();

        let state = self
            .inner
            .state
            .lock()
            .expect("plugin runtime lock poisoned");
        let indexing_state = if state.paused {
            "paused"
        } else if state.worker_running || state.scanner_running {
            "running"
        } else if !state.jobs.is_empty()
            || !state.pending_jobs.is_empty()
            || !state.pending_refreshes.is_empty()
        {
            "queued"
        } else {
            "idle"
        };
        let plugin_state = if state.paused
            || (!state.worker_running
                && !state.scanner_running
                && state.jobs.is_empty()
                && state.pending_jobs.is_empty()
                && state.pending_refreshes.is_empty())
        {
            "idle"
        } else {
            "working"
        };

        PluginIndexStatus {
            enabled_plugins: installed_plugins
                .iter()
                .filter(|plugin| plugin.enabled)
                .map(|plugin| plugin.id.clone())
                .collect(),
            installed_plugins: installed_plugins.clone(),
            indexing_state: indexing_state.to_string(),
            plugin_state: plugin_state.to_string(),
            paused: state.paused,
            search_active: self.inner.search_active.load(Ordering::SeqCst) > 0,
            scanner_running: state.scanner_running,
            worker_running: state.worker_running,
            plugin_summaries: installed_plugins
                .iter()
                .map(|plugin| plugin_health_summary(&plugin.id, counts.get(&plugin.id)))
                .collect(),
            issues,
            auto_ignored_issue_types,
        }
    }

    pub fn set_paused(&self, paused: bool) -> PluginIndexStatus {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("plugin runtime lock poisoned");
        state.paused = paused;
        self.inner.wake.notify_all();
        drop(state);
        self.status()
    }

    pub fn rebuild(&self) -> PluginIndexStatus {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("plugin runtime lock poisoned");
        state.pending_jobs.clear();
        state.pending_job_keys.clear();
        state.jobs.clear();
        state.queued_jobs.clear();
        state.processing_jobs.clear();
        state.pending_roots.clear();
        state.queued_roots.clear();
        state.pending_refreshes.clear();
        state.queued_refreshes.clear();
        state.active_refreshes.clear();
        let _ = self.inner.state_db.clear_all();
        self.inner.wake.notify_all();
        drop(state);
        self.status()
    }

    pub fn refresh_plugin_supported_files(&self, plugin_id: &str) -> Result<PluginIndexStatus> {
        let plugin_id = plugin_id.trim();
        if plugin_id.is_empty() {
            anyhow::bail!("plugin id is required");
        }

        self.request_plugin_refresh(plugin_id)?;
        Ok(self.status())
    }

    pub fn reset_plugin_cache(&self, plugin_id: &str) -> Result<PluginIndexStatus> {
        let plugin_id = plugin_id.trim();
        if plugin_id.is_empty() {
            anyhow::bail!("plugin id is required");
        }

        let rows = self.inner.state_db.list_plugin_rows(plugin_id)?;
        {
            let mut state = self
                .inner
                .state
                .lock()
                .expect("plugin runtime lock poisoned");
            state.pending_jobs.retain(|job| job.plugin_id != plugin_id);
            state
                .pending_job_keys
                .retain(|key| !key.ends_with(&format!("\0{plugin_id}")));
            state.jobs.retain(|job| job.plugin_id != plugin_id);
            state
                .queued_jobs
                .retain(|key| !key.ends_with(&format!("\0{plugin_id}")));
        }

        for row in &rows {
            if let Some(text_path) = &row.cache_text_path {
                let _ = fs::remove_file(text_path);
            }
            if let Some(meta_path) = &row.cache_meta_path {
                let _ = fs::remove_file(meta_path);
            }
        }
        self.inner.state_db.clear_plugin(plugin_id)?;
        self.inner.wake.notify_all();
        Ok(self.status())
    }

    pub fn ignore_issue(&self, source_path: &Path, plugin_id: &str) -> Result<PluginIndexStatus> {
        let attempts = self
            .inner
            .state_db
            .get_indexed_file(source_path, plugin_id)?
            .map(|row| row.attempts)
            .unwrap_or(0);
        self.inner
            .state_db
            .mark_ignored(source_path, plugin_id, attempts)?;
        Ok(self.status())
    }

    pub fn unignore_issue(&self, source_path: &Path, plugin_id: &str) -> Result<PluginIndexStatus> {
        let attempts = self
            .inner
            .state_db
            .get_indexed_file(source_path, plugin_id)?
            .map(|row| row.attempts)
            .unwrap_or(0)
            .max(1);
        if source_path.exists() {
            self.inner
                .state_db
                .mark_stale(source_path, plugin_id, attempts, Some("Re-enabled"))?;
            self.request_scan(source_path)?;
        } else {
            self.inner
                .state_db
                .mark_missing(source_path, plugin_id, attempts)?;
        }
        Ok(self.status())
    }

    pub fn retry_issue_type(
        &self,
        plugin_id: &str,
        error_code: &str,
    ) -> Result<PluginIndexStatus> {
        let plugin_id = plugin_id.trim();
        let error_code = error_code.trim();
        if plugin_id.is_empty() || error_code.is_empty() {
            anyhow::bail!("plugin id and error code are required");
        }

        for issue in self
            .inner
            .state_db
            .list_plugin_issues(plugin_id)?
            .into_iter()
            .filter(|row| {
                row.status != "ignored"
                    && row
                        .error_code
                        .as_deref()
                        .unwrap_or(row.status.as_str())
                        == error_code
            })
        {
            let source_path = PathBuf::from(&issue.source_path);
            if !source_path.exists() {
                continue;
            }
            let _ = self.request_retry(&source_path);
        }

        Ok(self.status())
    }

    pub fn ignore_issue_type(
        &self,
        plugin_id: &str,
        error_code: &str,
    ) -> Result<PluginIndexStatus> {
        let plugin_id = plugin_id.trim();
        let error_code = error_code.trim();
        if plugin_id.is_empty() || error_code.is_empty() {
            anyhow::bail!("plugin id and error code are required");
        }

        self.inner
            .state_db
            .ignore_issue_type(plugin_id, error_code)?;
        Ok(self.status())
    }

    pub fn set_issue_type_auto_ignore(
        &self,
        plugin_id: &str,
        error_code: &str,
        enabled: bool,
    ) -> Result<PluginIndexStatus> {
        let plugin_id = plugin_id.trim();
        let error_code = error_code.trim();
        if plugin_id.is_empty() || error_code.is_empty() {
            anyhow::bail!("plugin id and error code are required");
        }

        self.inner
            .state_db
            .set_issue_auto_ignore(plugin_id, error_code, enabled)?;
        if enabled {
            self.inner
                .state_db
                .ignore_issue_type(plugin_id, error_code)?;
        }
        Ok(self.status())
    }

    pub fn default_plugin_folder(&self) -> Option<PathBuf> {
        self.inner.plugin_roots.first().cloned()
    }

    pub fn install_plugin_archive(
        &self,
        archive_path: &Path,
    ) -> Result<(String, String, PluginIndexStatus)> {
        let plugin_root = self
            .default_plugin_folder()
            .ok_or_else(|| anyhow::anyhow!("Could not resolve the plugin folder."))?;
        let installed = install_plugin_archive(archive_path, &plugin_root)?;
        self.inner
            .state_db
            .set_preferred_plugin_version(&installed.plugin_id, &installed.version)?;
        self.request_plugin_refresh(&installed.plugin_id)?;
        Ok((installed.plugin_id, installed.version, self.status()))
    }

    pub fn set_plugin_enabled(
        &self,
        plugin_id: &str,
        enabled: bool,
    ) -> Result<PluginIndexStatus> {
        let plugin_id = plugin_id.trim();
        if plugin_id.is_empty() {
            anyhow::bail!("plugin id is required");
        }

        let discovery = discovery_report(&self.inner)?;
        if !discovery.registry.versions_by_id.contains_key(plugin_id) {
            anyhow::bail!("plugin {plugin_id} is not installed");
        }

        self.inner.state_db.set_plugin_enabled(plugin_id, enabled)?;
        if enabled {
            self.request_plugin_refresh(plugin_id)?;
        } else {
            drop_runtime_jobs_for_plugin(&self.inner, plugin_id);
            self.reset_plugin_cache(plugin_id)?;
        }
        Ok(self.status())
    }

    pub fn set_active_plugin_version(
        &self,
        plugin_id: &str,
        version: &str,
    ) -> Result<PluginIndexStatus> {
        let discovery = discovery_report(&self.inner)?;
        let Some(versions) = discovery.registry.versions_by_id.get(plugin_id) else {
            anyhow::bail!("plugin {plugin_id} is not installed");
        };
        if !versions.iter().any(|plugin| plugin.version == version) {
            anyhow::bail!("plugin {plugin_id} version {version} is not installed");
        }
        self.inner
            .state_db
            .set_preferred_plugin_version(plugin_id, version)?;
        Ok(self.status())
    }

    pub fn uninstall_plugin_version(
        &self,
        plugin_id: &str,
        version: &str,
    ) -> Result<PluginIndexStatus> {
        let discovery = discovery_report(&self.inner)?;
        let Some(versions) = discovery.registry.versions_by_id.get(plugin_id) else {
            anyhow::bail!("plugin {plugin_id} is not installed");
        };
        let plugin = versions
            .iter()
            .find(|plugin| plugin.version == version)
            .ok_or_else(|| {
                anyhow::anyhow!("plugin {plugin_id} version {version} is not installed")
            })?;
        fs::remove_dir_all(&plugin.root_dir)?;
        drop_runtime_jobs_for_plugin(&self.inner, plugin_id);

        let remaining = discovery_report(&self.inner)?;
        if let Some(active) = remaining.registry.by_id.get(plugin_id) {
            self.inner
                .state_db
                .set_preferred_plugin_version(plugin_id, &active.version)?;
        } else {
            self.inner
                .state_db
                .clear_preferred_plugin_version(plugin_id)?;
            self.inner.state_db.clear_plugin(plugin_id)?;
        }
        Ok(self.status())
    }

    #[cfg(test)]
    pub fn wait_for_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut state = self
            .inner
            .state
            .lock()
            .expect("plugin runtime lock poisoned");

        loop {
            let search_active = self.inner.search_active.load(Ordering::SeqCst) > 0;
            let idle = state.pending_roots.is_empty()
                && state.active_roots.is_empty()
                && state.pending_refreshes.is_empty()
                && state.active_refreshes.is_empty()
                && state.pending_jobs.is_empty()
                && state.jobs.is_empty()
                && state.processing_jobs.is_empty()
                && !state.scanner_running
                && !state.worker_running
                && !search_active;
            if idle {
                return true;
            }

            let now = Instant::now();
            if now >= deadline {
                return false;
            }

            let wait_for = deadline.saturating_duration_since(now);
            let (next_state, _) = self
                .inner
                .wake
                .wait_timeout(state, wait_for)
                .expect("plugin runtime condvar poisoned");
            state = next_state;
        }
    }
}

fn spawn_scanner_thread(inner: Arc<RuntimeInner>) {
    thread::spawn(move || loop {
        let task = {
            let mut state = inner.state.lock().expect("plugin runtime lock poisoned");
            while (state.pending_roots.is_empty() && state.pending_refreshes.is_empty())
                || state.paused
            {
                state.scanner_running = false;
                state = inner
                    .wake
                    .wait(state)
                    .expect("plugin runtime condvar poisoned");
            }

            let task = if let Some(refresh) = state.pending_refreshes.pop_front() {
                let key = refresh_key(&refresh.root, &refresh.plugin_id);
                state.queued_refreshes.remove(&key);
                state.active_refreshes.insert(key);
                ScanTask::PluginRefresh(refresh)
            } else {
                let root = state
                    .pending_roots
                    .pop_front()
                    .expect("pending root disappeared");
                state.queued_roots.remove(&root);
                state.active_roots.insert(root.clone());
                ScanTask::Root(root)
            };
            state.scanner_running = true;
            task
        };

        match &task {
            ScanTask::Root(root) => {
                let _ = inner.state_db.mark_scan_root_started(root);
                let scanned_count = scan_root(&inner, root).unwrap_or(0);
                let _ = inner.state_db.mark_scan_root_completed(root, scanned_count);
            }
            ScanTask::PluginRefresh(refresh) => {
                let _ =
                    scan_root_for_plugin(&inner, &refresh.root, &refresh.plugin_id).unwrap_or(0);
            }
        }

        let mut state = inner.state.lock().expect("plugin runtime lock poisoned");
        match task {
            ScanTask::Root(root) => {
                state.active_roots.remove(&root);
            }
            ScanTask::PluginRefresh(refresh) => {
                state
                    .active_refreshes
                    .remove(&refresh_key(&refresh.root, &refresh.plugin_id));
            }
        }
        state.scanner_running =
            !(state.pending_roots.is_empty() && state.pending_refreshes.is_empty());
        inner.wake.notify_all();
    });
}

enum ScanTask {
    Root(PathBuf),
    PluginRefresh(PluginRefresh),
}

fn recover_queued_jobs(inner: Arc<RuntimeInner>) {
    let Ok(discovery) = discovery_report(&inner) else {
        return;
    };
    let registered_plugin_ids = discovery
        .registry
        .by_id
        .keys()
        .cloned()
        .collect::<HashSet<_>>();
    let Ok(rows) = inner
        .state_db
        .list_recoverable_jobs(ACTIVE_QUEUE_TARGET.saturating_mul(64))
    else {
        return;
    };

    let mut state = inner.state.lock().expect("plugin runtime lock poisoned");
    for row in rows {
        if !registered_plugin_ids.contains(&row.plugin_id) {
            continue;
        }
        enqueue_pending_job(
            &inner,
            &mut state,
            PluginJob {
                source_path: PathBuf::from(row.source_path),
                plugin_id: row.plugin_id,
                attempts: row.attempts.max(1),
                run_id: next_run_id(&inner),
            },
            false,
        );
    }
    inner.wake.notify_all();
}

fn spawn_worker_thread(inner: Arc<RuntimeInner>) {
    thread::spawn(move || loop {
        let job = {
            let mut state = inner.state.lock().expect("plugin runtime lock poisoned");
            loop {
                enqueue_due_retries(&inner, &mut state);
                promote_pending_jobs(&mut state);
                if !state.jobs.is_empty()
                    && !state.paused
                    && inner.search_active.load(Ordering::SeqCst) == 0
                {
                    break;
                }
                state.worker_running = false;
                let (next_state, _) = inner
                    .wake
                    .wait_timeout(state, RETRY_SWEEP_INTERVAL)
                    .expect("plugin runtime condvar poisoned");
                state = next_state;
            }

            let job = state.jobs.pop_front().expect("queued job disappeared");
            let key = job_key(&job.source_path, &job.plugin_id);
            state.queued_jobs.remove(&key);
            state.processing_jobs.insert(key);
            state.worker_running = true;
            job
        };

        let job_key_value = job_key(&job.source_path, &job.plugin_id);
        let Some(plugin) = registered_plugin(&inner, &job.plugin_id) else {
            let _ = inner.state_db.mark_skipped(
                &job.source_path,
                &job.plugin_id,
                job.attempts.max(1),
                "Plugin is no longer installed",
            );
            let _ = inner.state_db.finish_plugin_run(
                &job.run_id,
                "skipped",
                Some("plugin_removed"),
                Some("Plugin is no longer installed"),
            );

            let mut state = inner.state.lock().expect("plugin runtime lock poisoned");
            state.processing_jobs.remove(&job_key_value);
            state.worker_running = false;
            inner.wake.notify_all();
            drop(state);
            thread::sleep(WORKER_DELAY);
            continue;
        };

        let _ = inner
            .state_db
            .mark_processing(&job.source_path, &job.plugin_id, job.attempts);
        let _ = inner.state_db.start_plugin_run(&PluginRunRecord {
            id: job.run_id.clone(),
            plugin_id: job.plugin_id.clone(),
            source_path: job.source_path.display().to_string(),
            started_at: now_rfc3339(),
            finished_at: None,
            status: "processing".to_string(),
            error_code: None,
            error_message: None,
        });

        let result = indexer::index_file_with_plugin_paths(
            &job.source_path,
            &inner.plugin_roots,
            &inner.index_roots,
        );

        let mut state = inner.state.lock().expect("plugin runtime lock poisoned");
        state.processing_jobs.remove(&job_key_value);

        match result {
            Ok(_) => {
                if let Ok(metadata) = fs::metadata(&job.source_path) {
                    let source_mtime = system_time_rfc3339(
                        metadata
                            .modified()
                            .unwrap_or_else(|_| std::time::SystemTime::now()),
                    );
                    let _ = inner.state_db.mark_ready(
                        &job.source_path,
                        &job.plugin_id,
                        &plugin.version,
                        metadata.len() as i64,
                        &source_mtime,
                        job.attempts,
                    );
                }
                let _ = inner
                    .state_db
                    .finish_plugin_run(&job.run_id, ready_status(), None, None);
            }
            Err(err) => {
                if !job.source_path.exists() {
                    let _ =
                        inner
                            .state_db
                            .mark_missing(&job.source_path, &job.plugin_id, job.attempts);
                    let _ = inner.state_db.finish_plugin_run(
                        &job.run_id,
                        "missing",
                        Some("missing_source"),
                        Some("Source file is missing"),
                    );
                } else {
                    let display = classify_index_error(&err);
                    let retry_after = retry_after_for_attempt(job.attempts);
                    let _ = inner.state_db.mark_failed(
                        &job.source_path,
                        &job.plugin_id,
                        job.attempts,
                        &display.code,
                        &display.message,
                        if display.details.is_empty() {
                            None
                        } else {
                            Some(display.details.as_str())
                        },
                        retry_after.as_deref(),
                    );
                    let _ = inner.state_db.finish_plugin_run(
                        &job.run_id,
                        "failed",
                        Some(&display.code),
                        Some(&display.message),
                    );
                }
            }
        }

        state.worker_running = false;
        inner.wake.notify_all();
        drop(state);
        thread::sleep(WORKER_DELAY);
    });
}

fn enqueue_due_retries(inner: &Arc<RuntimeInner>, state: &mut RuntimeState) {
    let Ok(rows) = inner.state_db.list_retry_ready(ACTIVE_QUEUE_TARGET) else {
        return;
    };

    for row in rows {
        enqueue_pending_job(
            inner,
            state,
            PluginJob {
                source_path: PathBuf::from(row.source_path),
                plugin_id: row.plugin_id,
                attempts: row.attempts + 1,
                run_id: next_run_id(inner),
            },
            false,
        );
    }
}

fn promote_pending_jobs(state: &mut RuntimeState) {
    while state.jobs.len() < ACTIVE_QUEUE_TARGET {
        let Some(job) = state.pending_jobs.pop_front() else {
            break;
        };
        let key = job_key(&job.source_path, &job.plugin_id);
        state.pending_job_keys.remove(&key);
        state.queued_jobs.insert(key);
        state.jobs.push_back(job);
    }
}

fn enqueue_pending_job(
    inner: &Arc<RuntimeInner>,
    state: &mut RuntimeState,
    job: PluginJob,
    priority_front: bool,
) {
    let key = job_key(&job.source_path, &job.plugin_id);
    if state.pending_job_keys.contains(&key)
        || state.queued_jobs.contains(&key)
        || state.processing_jobs.contains(&key)
    {
        return;
    }
    state.pending_job_keys.insert(key);
    let Some(plugin) = registered_plugin(inner, &job.plugin_id) else {
        state
            .pending_job_keys
            .remove(&job_key(&job.source_path, &job.plugin_id));
        let _ = inner.state_db.mark_skipped(
            &job.source_path,
            &job.plugin_id,
            job.attempts.max(1),
            "Plugin is no longer installed",
        );
        return;
    };
    let plugin_version = plugin.version;
    let source_size = fs::metadata(&job.source_path)
        .map(|value| value.len() as i64)
        .unwrap_or(0);
    let source_mtime = fs::metadata(&job.source_path)
        .and_then(|value| value.modified())
        .map(system_time_rfc3339)
        .unwrap_or_else(|_| now_rfc3339());
    let existing = inner
        .state_db
        .get_indexed_file(&job.source_path, &job.plugin_id)
        .ok()
        .flatten();
    let _ = if existing.is_some() {
        inner.state_db.mark_queued(
            &job.source_path,
            &job.plugin_id,
            &plugin_version,
            source_size,
            &source_mtime,
            job.attempts.saturating_sub(1),
        )
    } else {
        inner.state_db.upsert_discovered_file(
            &job.source_path,
            &job.plugin_id,
            &plugin_version,
            source_size,
            &source_mtime,
            queued_status(),
            job.attempts.saturating_sub(1),
        )
    };
    if priority_front {
        state.pending_jobs.push_front(job);
    } else {
        state.pending_jobs.push_back(job);
    }
}

fn scan_root(inner: &Arc<RuntimeInner>, root: &Path) -> Result<usize> {
    scan_root_internal(inner, root, None)
}

fn scan_root_for_plugin(inner: &Arc<RuntimeInner>, root: &Path, plugin_id: &str) -> Result<usize> {
    scan_root_internal(inner, root, Some(plugin_id))
}

fn scan_root_internal(
    inner: &Arc<RuntimeInner>,
    root: &Path,
    plugin_filter: Option<&str>,
) -> Result<usize> {
    let discovery = discovery_report(inner)?;
    let classifier = FileClassifier::new(&discovery.registry);
    let mut seen = HashSet::new();
    let mut supported_file_count = 0usize;

    if root.is_file() {
        if let Some(key) = scan_file(
            inner,
            root,
            &classifier,
            &discovery.registry,
            plugin_filter,
            true,
        ) {
            seen.insert(key);
            supported_file_count += 1;
        }
        if plugin_filter.is_none() {
            mark_missing_for_root(inner, root, &seen);
        }
        return Ok(supported_file_count);
    }

    let plugin_roots = discovery
        .registry
        .ignored_paths
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let index_roots = inner.index_roots.clone();
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .filter_entry(move |entry| scan_entry_allowed(entry, &plugin_roots, &index_roots))
        .build();

    for entry in walker {
        let Ok(entry) = entry else {
            continue;
        };
        if !entry
            .file_type()
            .is_some_and(|file_type| file_type.is_file())
        {
            continue;
        }
        if let Some(key) = scan_file(
            inner,
            entry.path(),
            &classifier,
            &discovery.registry,
            plugin_filter,
            true,
        ) {
            seen.insert(key);
            supported_file_count += 1;
        }
    }

    if plugin_filter.is_none() {
        mark_missing_for_root(inner, root, &seen);
    }
    Ok(supported_file_count)
}

fn mark_missing_for_root(inner: &Arc<RuntimeInner>, root: &Path, seen: &HashSet<String>) {
    let Ok(rows) = inner.state_db.list_root_rows(root) else {
        return;
    };

    for row in rows {
        let key = format!("{}\0{}", row.source_path, row.plugin_id);
        if seen.contains(&key) {
            continue;
        }
        let source_path = PathBuf::from(&row.source_path);
        if source_path.exists() {
            continue;
        }
        let _ = inner
            .state_db
            .mark_missing(&source_path, &row.plugin_id, row.attempts);
    }
}

fn scan_entry_allowed(entry: &DirEntry, plugin_roots: &[PathBuf], index_roots: &[PathBuf]) -> bool {
    let path = entry.path();

    if plugin_roots.iter().any(|root| path.starts_with(root)) {
        return false;
    }
    if index_roots.iter().any(|root| path.starts_with(root)) {
        return false;
    }

    if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
        if matches!(name, ".git" | "node_modules" | "target" | "dist") {
            return false;
        }
    }

    true
}

fn scan_file(
    inner: &Arc<RuntimeInner>,
    path: &Path,
    classifier: &FileClassifier,
    registry: &PluginRegistry,
    plugin_filter: Option<&str>,
    priority_front: bool,
) -> Option<String> {
    let FileKind::SupportedByPlugin { plugin_id } = classifier.classify(path) else {
        return None;
    };
    if plugin_filter.is_some_and(|value| value != plugin_id) {
        return None;
    }
    let plugin = registry.by_id.get(&plugin_id)?;
    let metadata = fs::metadata(path).ok()?;
    let source_size = metadata.len() as i64;
    let source_mtime = system_time_rfc3339(metadata.modified().ok()?);
    let key = job_key(path, &plugin.id);
    let existing = inner
        .state_db
        .get_indexed_file(path, &plugin.id)
        .ok()
        .flatten();
    let validation = cache::validate_cache(path, plugin);

    match existing {
        None => {
            enqueue_job(inner, path, &plugin.id, 1, priority_front);
        }
        Some(row) => {
            if row.status == "ignored" {
                let _ = inner.state_db.sync_ignored_metadata(
                    path,
                    &plugin.id,
                    &plugin.version,
                    source_size,
                    &source_mtime,
                );
                return Some(key);
            }

            let changed = row.source_size != source_size
                || row.source_mtime != source_mtime
                || !plugin_version_satisfies_selected(&plugin.version, &row.plugin_version);
            if changed {
                let _ = inner.state_db.mark_stale(
                    path,
                    &plugin.id,
                    row.attempts,
                    Some("Source file or plugin version changed"),
                );
                enqueue_job(inner, path, &plugin.id, 1, priority_front);
                return Some(key);
            }

            if validation.status == CacheStatus::Ready {
                let _ = inner.state_db.mark_ready(
                    path,
                    &plugin.id,
                    &plugin.version,
                    source_size,
                    &source_mtime,
                    row.attempts,
                );
                return Some(key);
            }

            if row.status == "failed" {
                if row.attempts >= 4 || !is_retry_ready(row.retry_after.as_deref()) {
                    let _ = inner.state_db.touch_checked_at(path, &plugin.id);
                    return Some(key);
                }
                enqueue_job(inner, path, &plugin.id, row.attempts + 1, priority_front);
                return Some(key);
            }

            if matches!(
                row.status.as_str(),
                "stale" | "missing" | "queued" | "processing"
            ) {
                enqueue_job(inner, path, &plugin.id, row.attempts.max(1), priority_front);
                return Some(key);
            }

            if row.status == "skipped" {
                let _ = inner.state_db.touch_checked_at(path, &plugin.id);
                return Some(key);
            }

            let _ =
                inner
                    .state_db
                    .mark_stale(path, &plugin.id, row.attempts, Some("Cache missing"));
            enqueue_job(inner, path, &plugin.id, row.attempts.max(1), priority_front);
        }
    }

    Some(key)
}

fn enqueue_job(
    inner: &Arc<RuntimeInner>,
    source_path: &Path,
    plugin_id: &str,
    attempts: u32,
    priority_front: bool,
) {
    let mut state = inner.state.lock().expect("plugin runtime lock poisoned");
    enqueue_pending_job(
        inner,
        &mut state,
        PluginJob {
            source_path: source_path.to_path_buf(),
            plugin_id: plugin_id.to_string(),
            attempts,
            run_id: next_run_id(inner),
        },
        priority_front,
    );
    inner.wake.notify_all();
}

fn next_run_id(inner: &RuntimeInner) -> String {
    let next = inner.run_counter.fetch_add(1, Ordering::SeqCst);
    format!("plugin-run-{next}")
}

fn job_key(source_path: &Path, plugin_id: &str) -> String {
    format!("{}\0{plugin_id}", source_path.display())
}

fn refresh_key(root: &Path, plugin_id: &str) -> String {
    format!("{}\0{plugin_id}", root.display())
}

fn classify_index_error(err: &anyhow::Error) -> FailureDisplay {
    err.downcast_ref::<IndexFailure>()
        .map(|failure| failure.display.clone())
        .unwrap_or_else(|| classify_failure(&err.to_string()))
}

fn registered_plugin(
    inner: &Arc<RuntimeInner>,
    plugin_id: &str,
) -> Option<crate::plugins::registry::RegisteredPlugin> {
    discovery_report(inner)
        .ok()
        .and_then(|report| report.registry.by_id.get(plugin_id).cloned())
}

fn drop_runtime_jobs_for_plugin(inner: &Arc<RuntimeInner>, plugin_id: &str) {
    let suffix = format!("\0{plugin_id}");
    let mut state = inner.state.lock().expect("plugin runtime lock poisoned");
    state.pending_jobs.retain(|job| job.plugin_id != plugin_id);
    state.pending_job_keys.retain(|key| !key.ends_with(&suffix));
    state.jobs.retain(|job| job.plugin_id != plugin_id);
    state.queued_jobs.retain(|key| !key.ends_with(&suffix));
    state.processing_jobs.retain(|key| !key.ends_with(&suffix));
    inner.wake.notify_all();
}

fn discovery_report(inner: &Arc<RuntimeInner>) -> Result<PluginDiscoveryReport> {
    let preferences = inner
        .state_db
        .preferred_plugin_versions()
        .unwrap_or_default();
    let disabled_plugin_ids = inner
        .state_db
        .disabled_plugin_ids()
        .unwrap_or_default()
        .into_iter()
        .collect::<HashSet<_>>();
    PluginRegistry::discover_for_platform_with_preferences(
        &inner.plugin_roots,
        crate::plugins::manifest::current_platform()?,
        &preferences,
        &disabled_plugin_ids,
    )
}

fn discovered_plugins(runtime: &PluginIndexRuntime) -> Vec<InstalledPluginInfo> {
    let disabled_plugin_ids = runtime
        .inner
        .state_db
        .disabled_plugin_ids()
        .unwrap_or_default()
        .into_iter()
        .collect::<HashSet<_>>();
    let Ok(discovery) = discovery_report(&runtime.inner) else {
        return Vec::new();
    };

    let mut plugins = discovery
        .registry
        .versions_by_id
        .values()
        .flat_map(|versions| versions.iter())
        .map(|plugin| InstalledPluginInfo {
            id: plugin.id.clone(),
            name: plugin.name.clone(),
            version: plugin.version.clone(),
            is_active: discovery
                .registry
                .by_id
                .get(&plugin.id)
                .map(|active| active.version == plugin.version)
                .unwrap_or(false),
            enabled: !disabled_plugin_ids.contains(&plugin.id),
            requires_entitlement: plugin.requires_entitlement,
            handles: plugin.handles.clone(),
            root_path: plugin.root_dir.display().to_string(),
            capabilities: PluginCapabilitySummary {
                text: plugin.capabilities.text,
                layout: plugin.capabilities.layout,
                ocr: plugin.capabilities.ocr,
            },
        })
        .collect::<Vec<_>>();
    plugins.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(right.is_active.cmp(&left.is_active))
            .then_with(|| plugin_version_cmp(&right.version, &left.version))
    });
    plugins
}

fn plugin_health_summary(plugin_id: &str, counts: Option<&PluginCounts>) -> PluginHealthSummary {
    let counts = counts.cloned().unwrap_or_default();
    PluginHealthSummary {
        plugin_id: plugin_id.to_string(),
        indexed_count: counts.indexed_count,
        attention_count: counts.attention_count,
        ignored_count: counts.ignored_count,
        queued_count: counts.queued_count,
        processing_count: counts.processing_count,
    }
}

fn map_issue_row(row: PluginIssueRow) -> PluginIssue {
    let error_code = row.error_code.clone().unwrap_or_else(|| row.status.clone());
    let message = issue_message(&row);
    let details = row
        .error_hint
        .or(row.error_message.clone())
        .unwrap_or_else(|| message.clone());
    PluginIssue {
        file_name: Path::new(&row.source_path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(&row.source_path)
            .to_string(),
        source_path: row.source_path,
        plugin_id: row.plugin_id,
        status: row.status,
        error_code,
        message,
        details,
        attempts: row.attempts,
        retry_after: row.retry_after,
        last_reported_at: row.updated_at,
    }
}

fn issue_message(row: &PluginIssueRow) -> String {
    if is_attention_status(&row.status) {
        match row.status.as_str() {
            "stale" => return "Needs reprocessing".to_string(),
            "missing" => return "Source file missing".to_string(),
            "skipped" => {
                return row
                    .error_message
                    .clone()
                    .unwrap_or_else(|| "Skipped".to_string())
            }
            "ignored" => return "Ignored".to_string(),
            _ => {}
        }
    }
    row.error_message
        .clone()
        .unwrap_or_else(|| "Plugin issue".to_string())
}

fn system_time_rfc3339(value: std::time::SystemTime) -> String {
    let datetime = time::OffsetDateTime::from(value)
        .to_offset(time::UtcOffset::UTC)
        .replace_nanosecond(0)
        .expect("zero nanoseconds should be valid");
    datetime
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}
