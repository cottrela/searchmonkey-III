use crate::app_auth::{MarketplacePluginSummary, PurchaseConnectionSummary};
use crate::plugins::cache::{self, CacheStatus};
use crate::plugins::classifier::{FileClassifier, FileKind};
use crate::plugins::failure_state::{classify_failure, remove_failure_state, FailureDisplay};
use crate::plugins::index_paths::{
    default_index_roots, mirror_meta_path, mirror_meta_tmp_path, mirror_text_path,
    mirror_text_tmp_path,
};
use crate::plugins::indexer::{self, IndexFailure};
use crate::plugins::installer::install_plugin_archive;
use crate::plugins::registry::{
    default_plugin_roots, plugin_version_cmp, plugin_version_satisfies_selected,
    PluginDiscoveryReport, PluginRegistry,
};
use crate::plugins::state_db::{
    is_attention_status, is_retry_ready, now_rfc3339, queued_status, ready_status,
    retry_after_for_attempt, PluginCounts, PluginIssueCountRow, PluginIssueRow, PluginRunRecord,
    StateDb,
};
use anyhow::Result;
use ignore::{DirEntry, WalkBuilder};
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
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
pub struct PluginIndexSummary {
    pub enabled_plugins: Vec<String>,
    pub installed_plugins: Vec<InstalledPluginInfo>,
    pub indexing_state: String,
    pub plugin_state: String,
    pub paused: bool,
    pub search_active: bool,
    pub scanner_running: bool,
    pub worker_running: bool,
    pub plugin_summaries: Vec<PluginHealthSummary>,
    pub auto_ignored_issue_types: Vec<PluginIssuePreferenceSummary>,
    pub purchase_connection: PurchaseConnectionSummary,
    pub marketplace_plugins: Vec<MarketplacePluginSummary>,
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
    pub blocked_count: usize,
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
pub struct PluginIssueCount {
    pub plugin_id: String,
    pub status: String,
    pub error_code: String,
    pub count: usize,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PluginSchedulerPolicy {
    weight: usize,
    max_consecutive_jobs: usize,
}

impl PluginSchedulerPolicy {
    fn burst_limit(self) -> usize {
        self.weight.max(1).min(self.max_consecutive_jobs.max(1))
    }
}

#[derive(Debug, Default)]
struct DefaultPluginQueue {
    jobs: VecDeque<PluginJob>,
    policy: Option<PluginSchedulerPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveDefaultPluginBurst {
    plugin_id: String,
    remaining: usize,
}

#[derive(Debug, Clone)]
struct PluginRefresh {
    root: PathBuf,
    plugin_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueueLane {
    UserImmediate,
    User,
    Default,
}

#[derive(Default)]
struct RuntimeState {
    pending_user_roots: VecDeque<PathBuf>,
    pending_default_roots: VecDeque<PathBuf>,
    queued_user_roots: HashSet<PathBuf>,
    queued_default_roots: HashSet<PathBuf>,
    active_roots: HashMap<PathBuf, QueueLane>,
    pending_refreshes: VecDeque<PluginRefresh>,
    queued_refreshes: HashSet<String>,
    active_refreshes: HashSet<String>,
    user_immediate_jobs: VecDeque<PluginJob>,
    user_jobs_by_plugin: HashMap<String, DefaultPluginQueue>,
    user_plugin_order: VecDeque<String>,
    active_user_plugin_burst: Option<ActiveDefaultPluginBurst>,
    default_jobs_by_plugin: HashMap<String, DefaultPluginQueue>,
    default_plugin_order: VecDeque<String>,
    active_default_plugin_burst: Option<ActiveDefaultPluginBurst>,
    queued_user_jobs: HashSet<String>,
    queued_default_jobs: HashSet<String>,
    processing_jobs: HashSet<String>,
    paused: bool,
    scanner_running: bool,
    active_workers: usize,
}

struct RuntimeInner {
    state: Mutex<RuntimeState>,
    wake: Condvar,
    plugin_roots: Vec<PathBuf>,
    index_roots: Vec<PathBuf>,
    state_db: StateDb,
    search_active: AtomicUsize,
    run_counter: AtomicU64,
    worker_count: usize,
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
        let worker_count = default_plugin_worker_count();
        let inner = Arc::new(RuntimeInner {
            state: Mutex::new(RuntimeState::default()),
            wake: Condvar::new(),
            plugin_roots,
            index_roots,
            state_db,
            search_active: AtomicUsize::new(0),
            run_counter: AtomicU64::new(RUN_COUNTER_START),
            worker_count,
        });

        recover_queued_jobs(inner.clone());
        spawn_scanner_thread(inner.clone());
        spawn_worker_threads(inner.clone());

        Self { inner }
    }

    pub fn request_scan(&self, root: &Path) -> Result<()> {
        self.request_scan_in_lane(root, QueueLane::Default)
    }

    pub fn request_user_scan(&self, root: &Path) -> Result<()> {
        self.request_scan_in_lane(root, QueueLane::User)
    }

    fn request_all_scan_roots_in_lane(&self, lane: QueueLane) -> Result<()> {
        let roots = self.inner.state_db.list_scan_roots()?;
        for root in roots {
            if !root.exists() {
                continue;
            }
            self.request_scan_in_lane(&root, lane)?;
        }
        Ok(())
    }

    fn restart_queued_work_from_scan_roots(&self, lane: QueueLane) -> Result<()> {
        let roots = self.inner.state_db.list_scan_roots()?;
        let canonical_roots = roots
            .into_iter()
            .filter(|root| root.exists())
            .filter_map(|root| root.canonicalize().ok())
            .collect::<Vec<_>>();

        {
            let mut state = self
                .inner
                .state
                .lock()
                .expect("plugin runtime lock poisoned");
            clear_pending_runtime_work(&mut state);
            for root in canonical_roots {
                enqueue_root_restart(&mut state, root, lane);
            }
        }

        self.inner.wake.notify_all();
        Ok(())
    }

    fn request_scan_in_lane(&self, root: &Path, lane: QueueLane) -> Result<()> {
        let root = root.canonicalize()?;
        self.inner.state_db.upsert_scan_root(&root)?;
        {
            let mut state = self
                .inner
                .state
                .lock()
                .expect("plugin runtime lock poisoned");
            enqueue_root(&mut state, root, lane);
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
        self.request_retry_in_lane(path, QueueLane::UserImmediate)
    }

    fn request_retry_in_lane(&self, path: &Path, lane: QueueLane) -> Result<()> {
        let path = path.canonicalize()?;
        let discovery = discovery_report(&self.inner)?;
        let classifier = FileClassifier::new(&discovery.registry);
        let FileKind::SupportedByPlugin { plugin_id } = classifier.classify(&path) else {
            self.request_scan_in_lane(&path, lane)?;
            return Ok(());
        };

        let attempts = self
            .inner
            .state_db
            .get_indexed_file(&path, &plugin_id)?
            .map(|row| row.attempts.max(1))
            .unwrap_or(1);
        enqueue_job(&self.inner, &path, &plugin_id, attempts, lane);
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

    pub fn summary(&self) -> PluginIndexSummary {
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
        } else if state.active_workers > 0 || state.scanner_running {
            "running"
        } else if !state.user_immediate_jobs.is_empty()
            || has_default_jobs(state.user_jobs_by_plugin.values())
            || has_default_jobs(state.default_jobs_by_plugin.values())
            || !state.pending_user_roots.is_empty()
            || !state.pending_default_roots.is_empty()
            || !state.pending_refreshes.is_empty()
        {
            "queued"
        } else {
            "idle"
        };
        let plugin_state = if state.paused
            || (state.active_workers == 0
                && !state.scanner_running
                && state.user_immediate_jobs.is_empty()
                && !has_default_jobs(state.user_jobs_by_plugin.values())
                && !has_default_jobs(state.default_jobs_by_plugin.values())
                && state.pending_user_roots.is_empty()
                && state.pending_default_roots.is_empty()
                && state.pending_refreshes.is_empty())
        {
            "idle"
        } else {
            "working"
        };

        PluginIndexSummary {
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
            worker_running: state.active_workers > 0,
            plugin_summaries: installed_plugins
                .iter()
                .map(|plugin| plugin_health_summary(&plugin.id, counts.get(&plugin.id)))
                .collect(),
            auto_ignored_issue_types,
            purchase_connection: PurchaseConnectionSummary {
                state: "not_connected".to_string(),
                email: None,
                pending_email: None,
                pending_expires_at: None,
                last_synced_at: None,
                has_cached_entitlements: false,
                status_message: None,
                storage_warning: None,
            },
            marketplace_plugins: Vec::new(),
        }
    }

    pub fn issue_counts(&self, plugin_id: &str) -> Result<Vec<PluginIssueCount>> {
        let plugin_id = plugin_id.trim();
        if plugin_id.is_empty() {
            anyhow::bail!("plugin id is required");
        }
        Ok(self
            .inner
            .state_db
            .list_plugin_issue_counts(plugin_id)?
            .into_iter()
            .map(|row| map_issue_count_row(plugin_id, row))
            .collect())
    }

    pub fn issues_page(
        &self,
        plugin_id: &str,
        status: Option<&str>,
        error_code: Option<&str>,
        limit: usize,
    ) -> Result<Vec<PluginIssue>> {
        let plugin_id = plugin_id.trim();
        if plugin_id.is_empty() {
            anyhow::bail!("plugin id is required");
        }
        let status = status.map(str::trim).filter(|value| !value.is_empty());
        let error_code = error_code.map(str::trim).filter(|value| !value.is_empty());
        let limit = limit.max(1).min(100);
        Ok(self
            .inner
            .state_db
            .list_plugin_issues_page(plugin_id, status, error_code, limit)?
            .into_iter()
            .map(map_issue_row)
            .collect())
    }

    pub fn set_paused(&self, paused: bool) -> PluginIndexSummary {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("plugin runtime lock poisoned");
        state.paused = paused;
        self.inner.wake.notify_all();
        drop(state);
        self.summary()
    }

    pub fn rebuild(&self) -> PluginIndexSummary {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("plugin runtime lock poisoned");
        state.user_immediate_jobs.clear();
        state.user_jobs_by_plugin.clear();
        state.user_plugin_order.clear();
        state.active_user_plugin_burst = None;
        state.default_jobs_by_plugin.clear();
        state.default_plugin_order.clear();
        state.active_default_plugin_burst = None;
        state.queued_user_jobs.clear();
        state.queued_default_jobs.clear();
        state.processing_jobs.clear();
        state.pending_user_roots.clear();
        state.pending_default_roots.clear();
        state.queued_user_roots.clear();
        state.queued_default_roots.clear();
        state.pending_refreshes.clear();
        state.queued_refreshes.clear();
        state.active_refreshes.clear();
        let _ = self.inner.state_db.clear_all();
        self.inner.wake.notify_all();
        drop(state);
        self.summary()
    }

    pub fn refresh_plugin_supported_files(&self, plugin_id: &str) -> Result<PluginIndexSummary> {
        let plugin_id = plugin_id.trim();
        if plugin_id.is_empty() {
            anyhow::bail!("plugin id is required");
        }

        self.request_plugin_refresh(plugin_id)?;
        Ok(self.summary())
    }

    pub fn reset_plugin_cache(&self, plugin_id: &str) -> Result<PluginIndexSummary> {
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
            state
                .queued_user_jobs
                .retain(|key| !key.ends_with(&format!("\0{plugin_id}")));
            state
                .queued_default_jobs
                .retain(|key| !key.ends_with(&format!("\0{plugin_id}")));
            remove_user_jobs_for_plugin(&mut state, plugin_id);
            remove_default_jobs_for_plugin(&mut state, plugin_id);
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
        Ok(self.summary())
    }

    pub fn ignore_issue(&self, source_path: &Path, plugin_id: &str) -> Result<PluginIndexSummary> {
        let attempts = self
            .inner
            .state_db
            .get_indexed_file(source_path, plugin_id)?
            .map(|row| row.attempts)
            .unwrap_or(0);
        self.inner
            .state_db
            .mark_ignored(source_path, plugin_id, attempts)?;
        Ok(self.summary())
    }

    pub fn unignore_issue(
        &self,
        source_path: &Path,
        plugin_id: &str,
    ) -> Result<PluginIndexSummary> {
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
            self.request_user_scan(source_path)?;
        } else {
            prune_missing_source(&self.inner, source_path, plugin_id);
        }
        Ok(self.summary())
    }

    pub fn retry_issue_type(
        &self,
        plugin_id: &str,
        error_code: &str,
    ) -> Result<PluginIndexSummary> {
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
                    && row.error_code.as_deref().unwrap_or(row.status.as_str()) == error_code
            })
        {
            let source_path = PathBuf::from(&issue.source_path);
            if !source_path.exists() {
                continue;
            }
            let _ = self.request_retry(&source_path);
        }

        Ok(self.summary())
    }

    pub fn ignore_issue_type(
        &self,
        plugin_id: &str,
        error_code: &str,
    ) -> Result<PluginIndexSummary> {
        let plugin_id = plugin_id.trim();
        let error_code = error_code.trim();
        if plugin_id.is_empty() || error_code.is_empty() {
            anyhow::bail!("plugin id and error code are required");
        }

        self.inner
            .state_db
            .ignore_issue_type(plugin_id, error_code)?;
        Ok(self.summary())
    }

    pub fn set_issue_type_auto_ignore(
        &self,
        plugin_id: &str,
        error_code: &str,
        enabled: bool,
    ) -> Result<PluginIndexSummary> {
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
        Ok(self.summary())
    }

    pub fn default_plugin_folder(&self) -> Option<PathBuf> {
        self.inner.plugin_roots.first().cloned()
    }

    pub fn install_plugin_archive(
        &self,
        archive_path: &Path,
    ) -> Result<(String, String, PluginIndexSummary)> {
        let plugin_root = self
            .default_plugin_folder()
            .ok_or_else(|| anyhow::anyhow!("Could not resolve the plugin folder."))?;
        let installed = install_plugin_archive(archive_path, &plugin_root)?;
        self.inner
            .state_db
            .set_preferred_plugin_version(&installed.plugin_id, &installed.version)?;
        self.restart_queued_work_from_scan_roots(QueueLane::Default)?;
        Ok((installed.plugin_id, installed.version, self.summary()))
    }

    pub fn set_plugin_enabled(&self, plugin_id: &str, enabled: bool) -> Result<PluginIndexSummary> {
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
            self.request_all_scan_roots_in_lane(QueueLane::Default)?;
        } else {
            drop_runtime_jobs_for_plugin(&self.inner, plugin_id);
            self.reset_plugin_cache(plugin_id)?;
        }
        Ok(self.summary())
    }

    pub fn set_active_plugin_version(
        &self,
        plugin_id: &str,
        version: &str,
    ) -> Result<PluginIndexSummary> {
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
        self.restart_queued_work_from_scan_roots(QueueLane::Default)?;
        Ok(self.summary())
    }

    pub fn uninstall_plugin_version(
        &self,
        plugin_id: &str,
        version: &str,
    ) -> Result<PluginIndexSummary> {
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
        Ok(self.summary())
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
            let idle = state.pending_user_roots.is_empty()
                && state.pending_default_roots.is_empty()
                && state.active_roots.is_empty()
                && state.pending_refreshes.is_empty()
                && state.active_refreshes.is_empty()
                && state.user_immediate_jobs.is_empty()
                && !has_default_jobs(state.user_jobs_by_plugin.values())
                && !has_default_jobs(state.default_jobs_by_plugin.values())
                && state.processing_jobs.is_empty()
                && !state.scanner_running
                && state.active_workers == 0
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

            while (state.pending_user_roots.is_empty()
                && state.pending_default_roots.is_empty()
                && state.pending_refreshes.is_empty())
                || state.paused
            {
                state.scanner_running = false;
                state = inner
                    .wake
                    .wait(state)
                    .expect("plugin runtime condvar poisoned");
            }

            let task = if let Some(root) = state.pending_user_roots.pop_front() {
                state.queued_user_roots.remove(&root);
                state.active_roots.insert(root.clone(), QueueLane::User);

                ScanTask::Root {
                    root,
                    lane: QueueLane::User,
                }
            } else if let Some(refresh) = state.pending_refreshes.pop_front() {
                let key = refresh_key(&refresh.root, &refresh.plugin_id);
                state.queued_refreshes.remove(&key);
                state.active_refreshes.insert(key);

                ScanTask::PluginRefresh(refresh)
            } else {
                let root = state
                    .pending_default_roots
                    .pop_front()
                    .expect("pending root disappeared");

                state.queued_default_roots.remove(&root);
                state.active_roots.insert(root.clone(), QueueLane::Default);

                ScanTask::Root {
                    root,
                    lane: QueueLane::Default,
                }
            };

            state.scanner_running = true;
            task
        };

        match &task {
            ScanTask::Root { root, lane } => {
                let _ = inner.state_db.mark_scan_root_started(root);
                let scanned_count = scan_root(&inner, root, *lane).unwrap_or(0);
                let _ = inner.state_db.mark_scan_root_completed(root, scanned_count);
            }
            ScanTask::PluginRefresh(refresh) => {
                let _ = scan_root_for_plugin(
                    &inner,
                    &refresh.root,
                    &refresh.plugin_id,
                    QueueLane::Default,
                )
                .unwrap_or(0);
            }
        }

        let mut state = inner.state.lock().expect("plugin runtime lock poisoned");

        match task {
            ScanTask::Root { root, .. } => {
                state.active_roots.remove(&root);
            }
            ScanTask::PluginRefresh(refresh) => {
                state
                    .active_refreshes
                    .remove(&refresh_key(&refresh.root, &refresh.plugin_id));
            }
        }

        state.scanner_running = !(state.pending_user_roots.is_empty()
            && state.pending_default_roots.is_empty()
            && state.pending_refreshes.is_empty());

        inner.wake.notify_all();
    });
}

enum ScanTask {
    Root { root: PathBuf, lane: QueueLane },
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
            QueueLane::Default,
        );
    }
    inner.wake.notify_all();
}

fn spawn_worker_threads(inner: Arc<RuntimeInner>) {
    for _ in 0..inner.worker_count {
        let worker_inner = inner.clone();
        thread::spawn(move || worker_loop(worker_inner));
    }
}

fn worker_loop(inner: Arc<RuntimeInner>) {
    loop {
        let job = {
            let mut state = inner.state.lock().expect("plugin runtime lock poisoned");
            loop {
                enqueue_due_retries(&inner, &mut state);
                if (!state.user_immediate_jobs.is_empty()
                    || has_default_jobs(state.user_jobs_by_plugin.values())
                    || has_default_jobs(state.default_jobs_by_plugin.values()))
                    && !state.paused
                    && inner.search_active.load(Ordering::SeqCst) == 0
                {
                    break;
                }
                let (next_state, _) = inner
                    .wake
                    .wait_timeout(state, RETRY_SWEEP_INTERVAL)
                    .expect("plugin runtime condvar poisoned");
                state = next_state;
            }

            let (job, key) = if let Some(job) = state.user_immediate_jobs.pop_front() {
                let key = job_key(&job.source_path, &job.plugin_id);
                state.queued_user_jobs.remove(&key);
                (job, key)
            } else if let Some(job) = pop_weighted_user_job(&mut state) {
                let key = job_key(&job.source_path, &job.plugin_id);
                state.queued_user_jobs.remove(&key);
                (job, key)
            } else {
                let job = pop_weighted_default_job(&mut state).expect("queued job disappeared");
                let key = job_key(&job.source_path, &job.plugin_id);
                state.queued_default_jobs.remove(&key);
                (job, key)
            };
            state.processing_jobs.insert(key);
            state.active_workers += 1;
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
            state.active_workers = state.active_workers.saturating_sub(1);
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
                    prune_missing_source(&inner, &job.source_path, &job.plugin_id);
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

        state.active_workers = state.active_workers.saturating_sub(1);
        inner.wake.notify_all();
        drop(state);
        thread::sleep(WORKER_DELAY);
    }
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
            QueueLane::Default,
        );
    }
}

fn enqueue_pending_job(
    inner: &Arc<RuntimeInner>,
    state: &mut RuntimeState,
    job: PluginJob,
    lane: QueueLane,
) {
    let key = job_key(&job.source_path, &job.plugin_id);
    if state.processing_jobs.contains(&key) {
        return;
    }
    let Some(plugin) = registered_plugin(inner, &job.plugin_id) else {
        let _ = inner.state_db.mark_skipped(
            &job.source_path,
            &job.plugin_id,
            job.attempts.max(1),
            "Plugin is no longer installed",
        );
        return;
    };
    let plugin_version = plugin.version.clone();
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
    enqueue_job_in_lane(
        state,
        job,
        key,
        lane,
        Some(scheduler_policy_for_plugin(&plugin)),
    );
}

fn scan_root(inner: &Arc<RuntimeInner>, root: &Path, lane: QueueLane) -> Result<usize> {
    scan_root_internal(inner, root, None, lane)
}

fn scan_root_for_plugin(
    inner: &Arc<RuntimeInner>,
    root: &Path,
    plugin_id: &str,
    lane: QueueLane,
) -> Result<usize> {
    scan_root_internal(inner, root, Some(plugin_id), lane)
}

fn scan_root_internal(
    inner: &Arc<RuntimeInner>,
    root: &Path,
    plugin_filter: Option<&str>,
    lane: QueueLane,
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
            lane,
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
        if lane == QueueLane::Default && has_user_work(inner) {
            break;
        }
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
            lane,
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
        prune_missing_source(inner, &source_path, &row.plugin_id);
    }
}

fn prune_missing_source(inner: &Arc<RuntimeInner>, source_path: &Path, plugin_id: &str) {
    let _ = inner.state_db.remove_indexed_file(source_path, plugin_id);

    for index_root in &inner.index_roots {
        let _ = fs::remove_file(mirror_text_path(index_root, source_path));
        let _ = fs::remove_file(mirror_meta_path(index_root, source_path));
        let _ = fs::remove_file(mirror_text_tmp_path(index_root, source_path));
        let _ = fs::remove_file(mirror_meta_tmp_path(index_root, source_path));
        let _ = remove_failure_state(index_root, source_path);
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
    lane: QueueLane,
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
            enqueue_job(inner, path, &plugin.id, 1, lane);
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
                enqueue_job(inner, path, &plugin.id, 1, lane);
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
                enqueue_job(inner, path, &plugin.id, row.attempts + 1, lane);
                return Some(key);
            }

            if matches!(
                row.status.as_str(),
                "stale" | "missing" | "queued" | "processing"
            ) {
                enqueue_job(inner, path, &plugin.id, row.attempts.max(1), lane);
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
            enqueue_job(inner, path, &plugin.id, row.attempts.max(1), lane);
        }
    }

    Some(key)
}

fn enqueue_job(
    inner: &Arc<RuntimeInner>,
    source_path: &Path,
    plugin_id: &str,
    attempts: u32,
    lane: QueueLane,
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
        lane,
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
    state
        .user_immediate_jobs
        .retain(|job| job.plugin_id != plugin_id);
    remove_user_jobs_for_plugin(&mut state, plugin_id);
    remove_default_jobs_for_plugin(&mut state, plugin_id);
    state.queued_user_jobs.retain(|key| !key.ends_with(&suffix));
    state
        .queued_default_jobs
        .retain(|key| !key.ends_with(&suffix));
    state.processing_jobs.retain(|key| !key.ends_with(&suffix));
    inner.wake.notify_all();
}

fn clear_pending_runtime_work(state: &mut RuntimeState) {
    state.user_immediate_jobs.clear();
    state.user_jobs_by_plugin.clear();
    state.user_plugin_order.clear();
    state.active_user_plugin_burst = None;
    state.default_jobs_by_plugin.clear();
    state.default_plugin_order.clear();
    state.active_default_plugin_burst = None;
    state.queued_user_jobs.clear();
    state.queued_default_jobs.clear();
    state.pending_user_roots.clear();
    state.pending_default_roots.clear();
    state.queued_user_roots.clear();
    state.queued_default_roots.clear();
    state.pending_refreshes.clear();
    state.queued_refreshes.clear();
}

fn enqueue_root_restart(state: &mut RuntimeState, root: PathBuf, lane: QueueLane) {
    if state.active_roots.contains_key(&root) {
        match lane {
            QueueLane::UserImmediate | QueueLane::User => {
                if state.queued_user_roots.insert(root.clone()) {
                    state.pending_user_roots.push_back(root);
                }
            }
            QueueLane::Default => {
                if !state.queued_user_roots.contains(&root)
                    && state.queued_default_roots.insert(root.clone())
                {
                    state.pending_default_roots.push_back(root);
                }
            }
        }
        return;
    }

    enqueue_root(state, root, lane);
}

fn enqueue_root(state: &mut RuntimeState, root: PathBuf, lane: QueueLane) {
    if let Some(active_lane) = state.active_roots.get(&root).copied() {
        if active_lane == QueueLane::Default
            && matches!(lane, QueueLane::User | QueueLane::UserImmediate)
        {
            if state.queued_user_roots.insert(root.clone()) {
                state.pending_user_roots.push_front(root);
            }
        }

        return;
    }

    match lane {
        QueueLane::UserImmediate | QueueLane::User => {
            if state.queued_user_roots.insert(root.clone()) {
                state.pending_user_roots.push_front(root.clone());
            }

            state.queued_default_roots.remove(&root);
            remove_root_from_queue(&mut state.pending_default_roots, &root);
        }

        QueueLane::Default => {
            if !state.queued_user_roots.contains(&root)
                && state.queued_default_roots.insert(root.clone())
            {
                state.pending_default_roots.push_back(root);
            }
        }
    }
}

fn remove_root_from_queue(queue: &mut VecDeque<PathBuf>, root: &Path) {
    if let Some(index) = queue.iter().position(|entry| entry == root) {
        queue.remove(index);
    }
}

fn enqueue_job_in_lane(
    state: &mut RuntimeState,
    job: PluginJob,
    key: String,
    lane: QueueLane,
    policy: Option<PluginSchedulerPolicy>,
) {
    match lane {
        QueueLane::UserImmediate => {
            if state.queued_user_jobs.contains(&key) {
                return;
            }

            if state.queued_default_jobs.remove(&key) {
                remove_default_job_from_queues(state, &key);
            }

            state.queued_user_jobs.insert(key);
            state.user_immediate_jobs.push_front(job);
        }

        QueueLane::User => {
            if state.queued_user_jobs.contains(&key) {
                return;
            }

            if state.queued_default_jobs.remove(&key) {
                remove_default_job_from_queues(state, &key);
            }

            state.queued_user_jobs.insert(key);
            let plugin_id = job.plugin_id.clone();
            let queue = state
                .user_jobs_by_plugin
                .entry(plugin_id.clone())
                .or_default();
            if queue.policy.is_none() {
                queue.policy = policy.or_else(|| Some(default_scheduler_policy()));
            }
            queue.jobs.push_back(job);
            if !state.user_plugin_order.iter().any(|entry| entry == &plugin_id) {
                state.user_plugin_order.push_back(plugin_id);
            }
        }

        QueueLane::Default => {
            if state.queued_user_jobs.contains(&key) || state.queued_default_jobs.contains(&key) {
                return;
            }

            state.queued_default_jobs.insert(key);
            let plugin_id = job.plugin_id.clone();
            let queue = state
                .default_jobs_by_plugin
                .entry(plugin_id.clone())
                .or_default();
            if queue.policy.is_none() {
                queue.policy = policy.or_else(|| Some(default_scheduler_policy()));
            }
            queue.jobs.push_back(job);
            if !state.default_plugin_order.iter().any(|entry| entry == &plugin_id) {
                state.default_plugin_order.push_back(plugin_id);
            }
        }
    }
}

fn has_user_work(inner: &Arc<RuntimeInner>) -> bool {
    let state = inner.state.lock().expect("plugin runtime lock poisoned");

    !state.pending_user_roots.is_empty()
        || !state.user_immediate_jobs.is_empty()
        || has_default_jobs(state.user_jobs_by_plugin.values())
        || state
            .active_roots
            .values()
            .any(|lane| matches!(lane, QueueLane::User | QueueLane::UserImmediate))
}

fn has_default_jobs<'a>(queues: impl IntoIterator<Item = &'a DefaultPluginQueue>) -> bool {
    queues.into_iter().any(|queue| !queue.jobs.is_empty())
}

fn default_scheduler_policy() -> PluginSchedulerPolicy {
    PluginSchedulerPolicy {
        weight: 4,
        max_consecutive_jobs: 8,
    }
}

fn scheduler_policy_for_plugin(
    plugin: &crate::plugins::registry::RegisteredPlugin,
) -> PluginSchedulerPolicy {
    if plugin.capabilities.ocr {
        PluginSchedulerPolicy {
            weight: 1,
            max_consecutive_jobs: 1,
        }
    } else {
        default_scheduler_policy()
    }
}

fn remove_user_jobs_for_plugin(state: &mut RuntimeState, plugin_id: &str) {
    remove_plugin_queue(
        &mut state.user_jobs_by_plugin,
        &mut state.user_plugin_order,
        &mut state.active_user_plugin_burst,
        plugin_id,
    );
}

fn remove_default_job_from_queues(state: &mut RuntimeState, key: &str) {
    remove_job_from_plugin_queues(&mut state.default_jobs_by_plugin, key);
    prune_empty_plugin_queues(
        &mut state.default_jobs_by_plugin,
        &mut state.default_plugin_order,
        &mut state.active_default_plugin_burst,
    );
}

fn remove_default_jobs_for_plugin(state: &mut RuntimeState, plugin_id: &str) {
    remove_plugin_queue(
        &mut state.default_jobs_by_plugin,
        &mut state.default_plugin_order,
        &mut state.active_default_plugin_burst,
        plugin_id,
    );
}

fn remove_job_from_plugin_queues(
    queues: &mut HashMap<String, DefaultPluginQueue>,
    key: &str,
) {
    for queue in queues.values_mut() {
        if let Some(index) = queue
            .jobs
            .iter()
            .position(|job| job_key(&job.source_path, &job.plugin_id) == key)
        {
            queue.jobs.remove(index);
            break;
        }
    }
}

fn remove_plugin_queue(
    queues: &mut HashMap<String, DefaultPluginQueue>,
    order: &mut VecDeque<String>,
    active_burst: &mut Option<ActiveDefaultPluginBurst>,
    plugin_id: &str,
) {
    queues.remove(plugin_id);
    order.retain(|entry| entry != plugin_id);
    if active_burst
        .as_ref()
        .is_some_and(|burst| burst.plugin_id == plugin_id)
    {
        *active_burst = None;
    }
}

fn prune_empty_plugin_queues(
    queues: &mut HashMap<String, DefaultPluginQueue>,
    order: &mut VecDeque<String>,
    active_burst: &mut Option<ActiveDefaultPluginBurst>,
) {
    let empty_plugins = queues
        .iter()
        .filter_map(|(plugin_id, queue)| {
            if queue.jobs.is_empty() {
                Some(plugin_id.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    for plugin_id in empty_plugins {
        remove_plugin_queue(queues, order, active_burst, &plugin_id);
    }
}

fn pop_weighted_plugin_job(
    queues: &mut HashMap<String, DefaultPluginQueue>,
    order: &mut VecDeque<String>,
    active_burst: &mut Option<ActiveDefaultPluginBurst>,
) -> Option<PluginJob> {
    loop {
        if let Some(active) = active_burst.clone() {
            let plugin_id = active.plugin_id.clone();
            let remaining = active.remaining.saturating_sub(1);
            let (job, queue_empty) = if let Some(queue) = queues.get_mut(&plugin_id) {
                if let Some(job) = queue.jobs.pop_front() {
                    (Some(job), queue.jobs.is_empty())
                } else {
                    (None, true)
                }
            } else {
                (None, true)
            };

            if let Some(job) = job {
                if queue_empty {
                    remove_plugin_queue(queues, order, active_burst, &plugin_id);
                } else if remaining > 0 {
                    *active_burst = Some(ActiveDefaultPluginBurst {
                        plugin_id,
                        remaining,
                    });
                } else {
                    *active_burst = None;
                    order.retain(|entry| entry != &plugin_id);
                    order.push_back(plugin_id);
                }

                return Some(job);
            }

            *active_burst = None;
            remove_plugin_queue(queues, order, active_burst, &plugin_id);
        }

        let plugin_id = order.pop_front()?;
        let Some((queue_empty, burst_limit)) = queues
            .get(&plugin_id)
            .map(|queue| {
                (
                    queue.jobs.is_empty(),
                    queue.policy
                        .unwrap_or_else(default_scheduler_policy)
                        .burst_limit(),
                )
            })
        else {
            continue;
        };
        if queue_empty {
            remove_plugin_queue(queues, order, active_burst, &plugin_id);
            continue;
        }

        *active_burst = Some(ActiveDefaultPluginBurst {
            plugin_id,
            remaining: burst_limit,
        });
    }
}

fn pop_weighted_default_job(state: &mut RuntimeState) -> Option<PluginJob> {
    pop_weighted_plugin_job(
        &mut state.default_jobs_by_plugin,
        &mut state.default_plugin_order,
        &mut state.active_default_plugin_burst,
    )
}

fn pop_weighted_user_job(state: &mut RuntimeState) -> Option<PluginJob> {
    pop_weighted_plugin_job(
        &mut state.user_jobs_by_plugin,
        &mut state.user_plugin_order,
        &mut state.active_user_plugin_burst,
    )
}

fn default_plugin_worker_count() -> usize {
    #[cfg(target_os = "macos")]
    if let Some(performance_cores) = macos_sysctl_usize("hw.perflevel0.physicalcpu") {
        if performance_cores > 0 {
            return performance_cores;
        }
    }

    thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .max(1)
}

#[cfg(target_os = "macos")]
fn macos_sysctl_usize(name: &str) -> Option<usize> {
    use std::ffi::CString;
    use std::mem::size_of;
    use std::ptr::null_mut;

    let name = CString::new(name).ok()?;
    let mut value: u32 = 0;
    let mut size = size_of::<u32>();
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut u32).cast(),
            &mut size,
            null_mut(),
            0,
        )
    };
    if rc == 0 && size == size_of::<u32>() {
        Some(value as usize)
    } else {
        None
    }
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
        blocked_count: counts.blocked_count,
    }
}

fn map_issue_count_row(plugin_id: &str, row: PluginIssueCountRow) -> PluginIssueCount {
    PluginIssueCount {
        plugin_id: plugin_id.to_string(),
        status: row.status,
        error_code: row.error_code,
        count: row.count,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::index_paths::{
        mirror_failure_state_path, mirror_meta_path, mirror_text_path,
    };
    use std::fs;
    use tempfile::tempdir;

    fn test_job(path: &str, plugin_id: &str) -> PluginJob {
        PluginJob {
            source_path: PathBuf::from(path),
            plugin_id: plugin_id.to_string(),
            attempts: 1,
            run_id: format!("run-{path}"),
        }
    }

    #[test]
    fn user_jobs_are_popped_before_default_jobs() {
        let mut state = RuntimeState::default();
        let default = test_job("/tmp/default.pdf", "ocr");
        let default_key = job_key(&default.source_path, &default.plugin_id);
        enqueue_job_in_lane(
            &mut state,
            default,
            default_key.clone(),
            QueueLane::Default,
            Some(PluginSchedulerPolicy {
                weight: 1,
                max_consecutive_jobs: 1,
            }),
        );

        let user = test_job("/tmp/user.pdf", "ocr");
        let user_key = job_key(&user.source_path, &user.plugin_id);
        enqueue_job_in_lane(&mut state, user.clone(), user_key.clone(), QueueLane::User, None);

        assert_eq!(
            pop_weighted_user_job(&mut state).map(|job| job.source_path),
            Some(user.source_path)
        );
        assert!(state.queued_user_jobs.contains(&user_key));
        assert!(state.queued_default_jobs.contains(&default_key));
    }

    #[test]
    fn user_request_promotes_existing_default_job() {
        let mut state = RuntimeState::default();
        let path = "/tmp/promoted.pdf";
        let default = test_job(path, "ocr");
        let key = job_key(&default.source_path, &default.plugin_id);
        enqueue_job_in_lane(
            &mut state,
            default,
            key.clone(),
            QueueLane::Default,
            Some(PluginSchedulerPolicy {
                weight: 1,
                max_consecutive_jobs: 1,
            }),
        );

        let user = test_job(path, "ocr");
        enqueue_job_in_lane(&mut state, user.clone(), key.clone(), QueueLane::User, None);

        assert!(!has_default_jobs(state.default_jobs_by_plugin.values()));
        assert!(state.queued_default_jobs.is_empty());
        assert_eq!(
            pop_weighted_user_job(&mut state).map(|job| job.source_path),
            Some(user.source_path)
        );
        assert!(state.queued_user_jobs.contains(&key));
    }

    #[test]
    fn user_scan_promotes_existing_default_root() {
        let mut state = RuntimeState::default();
        let root = PathBuf::from("/tmp/ocr-test");
        enqueue_root(&mut state, root.clone(), QueueLane::Default);
        enqueue_root(&mut state, root.clone(), QueueLane::User);

        assert!(state.pending_default_roots.is_empty());
        assert!(state.queued_default_roots.is_empty());
        assert_eq!(state.pending_user_roots.front(), Some(&root));
        assert!(state.queued_user_roots.contains(&root));
    }

    #[test]
    fn restart_scan_requeues_active_root_for_followup_pass() {
        let mut state = RuntimeState::default();
        let root = PathBuf::from("/tmp/restart-root");
        state.active_roots.insert(root.clone(), QueueLane::Default);

        enqueue_root_restart(&mut state, root.clone(), QueueLane::Default);

        assert_eq!(state.pending_default_roots.front(), Some(&root));
        assert!(state.queued_default_roots.contains(&root));
    }

    #[test]
    fn default_jobs_are_scheduled_in_weighted_plugin_bursts() {
        let mut state = RuntimeState::default();
        for index in 0..6 {
            let job = test_job(&format!("/tmp/pdf-{index}.pdf"), "sm.plugin.pdf");
            let key = job_key(&job.source_path, &job.plugin_id);
            enqueue_job_in_lane(
                &mut state,
                job,
                key,
                QueueLane::Default,
                Some(PluginSchedulerPolicy {
                    weight: 4,
                    max_consecutive_jobs: 8,
                }),
            );
        }
        for index in 0..2 {
            let job = test_job(&format!("/tmp/ocr-{index}.png"), "sm.plugin.ocr");
            let key = job_key(&job.source_path, &job.plugin_id);
            enqueue_job_in_lane(
                &mut state,
                job,
                key,
                QueueLane::Default,
                Some(PluginSchedulerPolicy {
                    weight: 1,
                    max_consecutive_jobs: 1,
                }),
            );
        }

        let actual = (0..7)
            .map(|_| {
                pop_weighted_default_job(&mut state)
                    .expect("job should be queued")
                    .plugin_id
            })
            .collect::<Vec<_>>();

        assert_eq!(
            actual,
            vec![
                "sm.plugin.pdf",
                "sm.plugin.pdf",
                "sm.plugin.pdf",
                "sm.plugin.pdf",
                "sm.plugin.ocr",
                "sm.plugin.pdf",
                "sm.plugin.pdf",
            ]
        );
    }

    #[test]
    fn user_jobs_are_scheduled_in_weighted_plugin_bursts() {
        let mut state = RuntimeState::default();
        for index in 0..6 {
            let job = test_job(&format!("/tmp/user-pdf-{index}.pdf"), "sm.plugin.pdf");
            let key = job_key(&job.source_path, &job.plugin_id);
            enqueue_job_in_lane(
                &mut state,
                job,
                key,
                QueueLane::User,
                Some(PluginSchedulerPolicy {
                    weight: 4,
                    max_consecutive_jobs: 8,
                }),
            );
        }
        for index in 0..2 {
            let job = test_job(&format!("/tmp/user-ocr-{index}.png"), "sm.plugin.ocr");
            let key = job_key(&job.source_path, &job.plugin_id);
            enqueue_job_in_lane(
                &mut state,
                job,
                key,
                QueueLane::User,
                Some(PluginSchedulerPolicy {
                    weight: 1,
                    max_consecutive_jobs: 1,
                }),
            );
        }

        let actual = (0..7)
            .map(|_| {
                pop_weighted_user_job(&mut state)
                    .expect("job should be queued")
                    .plugin_id
            })
            .collect::<Vec<_>>();

        assert_eq!(
            actual,
            vec![
                "sm.plugin.pdf",
                "sm.plugin.pdf",
                "sm.plugin.pdf",
                "sm.plugin.pdf",
                "sm.plugin.ocr",
                "sm.plugin.pdf",
                "sm.plugin.pdf",
            ]
        );
    }

    #[test]
    fn prune_missing_source_removes_db_row_and_cached_artifacts() {
        let temp = tempdir().unwrap();
        let index_root = temp.path().join("index");
        let plugin_root = temp.path().join("plugins");
        let state_db_path = temp.path().join("searchmonkey.sqlite");
        fs::create_dir_all(&index_root).unwrap();
        fs::create_dir_all(&plugin_root).unwrap();

        let inner = Arc::new(RuntimeInner {
            state: Mutex::new(RuntimeState::default()),
            wake: Condvar::new(),
            plugin_roots: vec![plugin_root],
            index_roots: vec![index_root.clone()],
            state_db: StateDb::new_with_path(&[index_root.clone()], state_db_path).unwrap(),
            search_active: AtomicUsize::new(0),
            run_counter: AtomicU64::new(RUN_COUNTER_START),
            worker_count: 1,
        });
        let source_path = temp.path().join("missing.pdf");

        inner
            .state_db
            .upsert_discovered_file(
                &source_path,
                "sm.plugin.pdf",
                "0.1.0",
                123,
                "2026-05-17T00:00:00Z",
                queued_status(),
                1,
            )
            .unwrap();

        let text_path = mirror_text_path(&index_root, &source_path);
        let meta_path = mirror_meta_path(&index_root, &source_path);
        let failure_path = mirror_failure_state_path(&index_root, &source_path);
        fs::create_dir_all(text_path.parent().unwrap()).unwrap();
        fs::write(&text_path, "cached text").unwrap();
        fs::write(&meta_path, "{}").unwrap();
        fs::write(&failure_path, "{}").unwrap();

        prune_missing_source(&inner, &source_path, "sm.plugin.pdf");

        assert!(inner
            .state_db
            .get_indexed_file(&source_path, "sm.plugin.pdf")
            .unwrap()
            .is_none());
        assert!(!text_path.exists());
        assert!(!meta_path.exists());
        assert!(!failure_path.exists());
    }
}
