use crate::plugins::cache::{self, CacheStatus};
use crate::plugins::classifier::{FileClassifier, FileKind};
use crate::plugins::failure_state::{
    classify_failure, load_failure_state, remove_failure_state, retry_allowed, save_failure_state,
};
use crate::plugins::index_paths::default_index_roots;
use crate::plugins::indexer::{self, IndexFailure};
use crate::plugins::registry::{default_plugin_roots, PluginRegistry};
use anyhow::Result;
use ignore::{DirEntry, WalkBuilder};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;
#[cfg(test)]
use std::time::Instant;

const WORKER_DELAY: Duration = Duration::from_millis(250);
const FAILURE_HISTORY_LIMIT: usize = 20;
const RETRY_LIMIT: u32 = 3;
const ACTIVE_QUEUE_TARGET: usize = 16;
const RUNTIME_STATE_SCHEMA: &str = "sm.plugin-runtime.v1";

#[derive(Debug, Clone, Serialize)]
pub struct PluginIndexStatus {
    pub enabled_plugins: Vec<String>,
    pub installed_plugins: Vec<InstalledPluginInfo>,
    pub indexing_state: String,
    pub total_known: usize,
    pub ready_count: usize,
    pub processing_count: usize,
    pub queued_count: usize,
    pub pending_count: usize,
    pub failed_count: usize,
    pub skipped_count: usize,
    pub paused: bool,
    pub search_active: bool,
    pub scanner_running: bool,
    pub worker_running: bool,
    pub failures: Vec<PluginIndexFailure>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstalledPluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
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
pub struct PluginIndexFailure {
    pub source_path: String,
    pub plugin_id: String,
    pub attempts: u32,
    pub code: String,
    pub message: String,
    pub details: String,
    pub next_retry_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexedFileState {
    Ready,
    Pending,
    Queued,
    Processing,
    Failed,
    Skipped,
}

#[derive(Debug, Clone)]
struct FileStatus {
    plugin_id: String,
    state: IndexedFileState,
    attempts: u32,
    last_error: Option<String>,
}

#[derive(Debug, Clone)]
struct PluginJob {
    source_path: PathBuf,
    plugin_id: String,
    attempts: u32,
}

#[derive(Default)]
struct RuntimeState {
    pending_roots: VecDeque<PathBuf>,
    queued_roots: HashSet<PathBuf>,
    active_roots: HashSet<PathBuf>,
    pending_jobs: VecDeque<PluginJob>,
    pending_job_keys: HashSet<String>,
    jobs: VecDeque<PluginJob>,
    queued_jobs: HashSet<String>,
    processing_jobs: HashSet<String>,
    statuses: HashMap<PathBuf, FileStatus>,
    failures: VecDeque<PluginIndexFailure>,
    known_roots: HashSet<PathBuf>,
    loaded_roots: HashSet<PathBuf>,
    paused: bool,
    scanner_running: bool,
    worker_running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedRuntimeState {
    schema: String,
    roots: Vec<PersistedRootState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedRootState {
    root_path: String,
    files: Vec<PersistedFileState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedFileState {
    source_path: String,
    plugin_id: String,
    state: PersistedFileKind,
    attempts: u32,
    code: Option<String>,
    message: Option<String>,
    details: Option<String>,
    next_retry_at: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PersistedFileKind {
    Ready,
    Pending,
    Failed,
    Skipped,
}

struct RuntimeInner {
    state: Mutex<RuntimeState>,
    wake: Condvar,
    plugin_roots: Vec<PathBuf>,
    index_roots: Vec<PathBuf>,
    search_active: AtomicUsize,
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
        let inner = Arc::new(RuntimeInner {
            state: Mutex::new(RuntimeState::default()),
            wake: Condvar::new(),
            plugin_roots,
            index_roots,
            search_active: AtomicUsize::new(0),
        });

        spawn_scanner_thread(inner.clone());
        spawn_worker_thread(inner.clone());

        Self { inner }
    }

    pub fn request_scan(&self, root: &Path) -> Result<()> {
        let root = root.canonicalize()?;
        {
            let mut state = self.inner.state.lock().expect("plugin runtime lock poisoned");
            if !state.loaded_roots.contains(&root) {
                load_persisted_root(&self.inner, &mut state, &root);
                state.loaded_roots.insert(root.clone());
            }
            if state.queued_roots.contains(&root) || state.active_roots.contains(&root) {
                return Ok(());
            }

            state.known_roots.insert(root.clone());
            state.queued_roots.insert(root.clone());
            state.pending_roots.push_back(root);
        }
        self.inner.wake.notify_all();
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
        let state = self.inner.state.lock().expect("plugin runtime lock poisoned");
        let installed_plugins = discovered_plugins(&self.inner.plugin_roots);
        let mut ready_count = 0;
        let mut processing_count = 0;
        let mut queued_count = 0;
        let mut pending_count = 0;
        let mut failed_count = 0;
        let mut skipped_count = 0;

        for status in state.statuses.values() {
            match status.state {
                IndexedFileState::Ready => ready_count += 1,
                IndexedFileState::Pending => pending_count += 1,
                IndexedFileState::Processing => processing_count += 1,
                IndexedFileState::Queued => queued_count += 1,
                IndexedFileState::Failed => failed_count += 1,
                IndexedFileState::Skipped => skipped_count += 1,
            }
        }

        let indexing_state = if state.worker_running || state.scanner_running {
            "running"
        } else if queued_count > 0 || processing_count > 0 || pending_count > 0 {
            "queued"
        } else {
            "idle"
        };

        PluginIndexStatus {
            enabled_plugins: installed_plugins.iter().map(|plugin| plugin.id.clone()).collect(),
            installed_plugins,
            indexing_state: indexing_state.to_string(),
            total_known: state.statuses.len(),
            ready_count,
            processing_count,
            queued_count,
            pending_count,
            failed_count,
            skipped_count,
            paused: state.paused,
            search_active: self.inner.search_active.load(Ordering::SeqCst) > 0,
            scanner_running: state.scanner_running,
            worker_running: state.worker_running,
            failures: state.failures.iter().cloned().collect(),
        }
    }

    pub fn set_paused(&self, paused: bool) -> PluginIndexStatus {
        let mut state = self.inner.state.lock().expect("plugin runtime lock poisoned");
        state.paused = paused;
        self.inner.wake.notify_all();
        drop(state);
        self.status()
    }

    pub fn rebuild(&self) -> PluginIndexStatus {
        let mut state = self.inner.state.lock().expect("plugin runtime lock poisoned");
        state.pending_jobs.clear();
        state.pending_job_keys.clear();
        state.jobs.clear();
        state.queued_jobs.clear();
        state.processing_jobs.clear();
        state.statuses.clear();
        state.failures.clear();
        state.pending_roots.clear();
        state.queued_roots.clear();
        state.loaded_roots.clear();
        let mut roots = state.known_roots.iter().cloned().collect::<Vec<_>>();
        roots.sort();
        for root in roots {
            state.queued_roots.insert(root.clone());
            state.pending_roots.push_back(root);
        }
        self.inner.wake.notify_all();
        drop(state);
        self.status()
    }

    pub fn default_plugin_folder(&self) -> Option<PathBuf> {
        self.inner.plugin_roots.first().cloned()
    }

    #[cfg(test)]
    pub fn wait_for_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut state = self.inner.state.lock().expect("plugin runtime lock poisoned");

        loop {
            let search_active = self.inner.search_active.load(Ordering::SeqCst) > 0;
            let idle = state.pending_roots.is_empty()
                && state.active_roots.is_empty()
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

fn persisted_state_path(index_roots: &[PathBuf]) -> Option<PathBuf> {
    index_roots.first().map(|root| root.join(".sm-plugin-runtime.json"))
}

fn load_persisted_root(inner: &Arc<RuntimeInner>, state: &mut RuntimeState, root: &Path) {
    let Some(path) = persisted_state_path(&inner.index_roots) else {
        return;
    };
    let Ok(contents) = fs::read_to_string(&path) else {
        return;
    };
    let Ok(snapshot) = serde_json::from_str::<PersistedRuntimeState>(&contents) else {
        return;
    };
    if snapshot.schema != RUNTIME_STATE_SCHEMA {
        return;
    }
    let Some(root_state) = snapshot
        .roots
        .into_iter()
        .find(|entry| entry.root_path == root.display().to_string())
    else {
        return;
    };

    for file in root_state.files {
        let source_path = PathBuf::from(&file.source_path);
        let indexed_state = match file.state {
            PersistedFileKind::Ready => IndexedFileState::Ready,
            PersistedFileKind::Pending => IndexedFileState::Pending,
            PersistedFileKind::Failed => IndexedFileState::Failed,
            PersistedFileKind::Skipped => IndexedFileState::Skipped,
        };
        state.statuses.insert(
            source_path.clone(),
            FileStatus {
                plugin_id: file.plugin_id.clone(),
                state: indexed_state,
                attempts: file.attempts,
                last_error: file.message.clone(),
            },
        );
        if indexed_state == IndexedFileState::Failed {
            upsert_failure(
                &mut state.failures,
                PluginIndexFailure {
                    source_path: file.source_path,
                    plugin_id: file.plugin_id,
                    attempts: file.attempts,
                    code: file.code.unwrap_or_else(|| "plugin_failed".to_string()),
                    message: file.message.unwrap_or_else(|| "Plugin failed".to_string()),
                    details: file.details.unwrap_or_default(),
                    next_retry_at: file.next_retry_at,
                },
            );
        }
    }
}

fn persist_runtime_state(inner: &Arc<RuntimeInner>) {
    let Some(path) = persisted_state_path(&inner.index_roots) else {
        return;
    };

    let snapshot = {
        let state = inner.state.lock().expect("plugin runtime lock poisoned");
        let mut roots = state.known_roots.iter().cloned().collect::<Vec<_>>();
        roots.sort();
        let persisted_roots = roots
            .into_iter()
            .map(|root| {
                let mut files = state
                    .statuses
                    .iter()
                    .filter(|(source_path, _)| source_path.starts_with(&root))
                    .map(|(source_path, file_status)| {
                        let failure = state
                            .failures
                            .iter()
                            .find(|failure| failure.source_path == source_path.display().to_string());
                        PersistedFileState {
                            source_path: source_path.display().to_string(),
                            plugin_id: file_status.plugin_id.clone(),
                            state: match file_status.state {
                                IndexedFileState::Ready => PersistedFileKind::Ready,
                                IndexedFileState::Pending
                                | IndexedFileState::Queued
                                | IndexedFileState::Processing => PersistedFileKind::Pending,
                                IndexedFileState::Failed => PersistedFileKind::Failed,
                                IndexedFileState::Skipped => PersistedFileKind::Skipped,
                            },
                            attempts: file_status.attempts,
                            code: failure.map(|failure| failure.code.clone()),
                            message: file_status.last_error.clone(),
                            details: failure.map(|failure| failure.details.clone()),
                            next_retry_at: failure.and_then(|failure| failure.next_retry_at.clone()),
                        }
                    })
                    .collect::<Vec<_>>();
                files.sort_by(|left, right| left.source_path.cmp(&right.source_path));
                PersistedRootState {
                    root_path: root.display().to_string(),
                    files,
                }
            })
            .collect::<Vec<_>>();

        PersistedRuntimeState {
            schema: RUNTIME_STATE_SCHEMA.to_string(),
            roots: persisted_roots,
        }
    };

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&snapshot) {
        let _ = fs::write(path, bytes);
    }
}

fn spawn_scanner_thread(inner: Arc<RuntimeInner>) {
    thread::spawn(move || loop {
        let root = {
            let mut state = inner.state.lock().expect("plugin runtime lock poisoned");
            while state.pending_roots.is_empty() || state.paused {
                state.scanner_running = false;
                state = inner
                    .wake
                    .wait(state)
                    .expect("plugin runtime condvar poisoned");
            }

            let root = state.pending_roots.pop_front().expect("pending root disappeared");
            state.queued_roots.remove(&root);
            state.active_roots.insert(root.clone());
            state.scanner_running = true;
            root
        };

        let _ = scan_root(&inner, &root);

        let mut state = inner.state.lock().expect("plugin runtime lock poisoned");
        state.active_roots.remove(&root);
        state.scanner_running = !state.pending_roots.is_empty();
        inner.wake.notify_all();
        drop(state);
        persist_runtime_state(&inner);
    });
}

fn spawn_worker_thread(inner: Arc<RuntimeInner>) {
    thread::spawn(move || loop {
        let job = {
            let mut state = inner.state.lock().expect("plugin runtime lock poisoned");
            loop {
                promote_pending_jobs(&mut state);
                if !state.jobs.is_empty()
                    && !state.paused
                    && inner.search_active.load(Ordering::SeqCst) == 0
                {
                    break;
                }
                state.worker_running = false;
                state = inner
                    .wake
                    .wait(state)
                    .expect("plugin runtime condvar poisoned");
            }

            let job = state.jobs.pop_front().expect("queued job disappeared");
            let key = job_key(&job.source_path, &job.plugin_id);
            state.queued_jobs.remove(&key);
            state.processing_jobs.insert(key);
            if let Some(status) = state.statuses.get_mut(&job.source_path) {
                status.state = IndexedFileState::Processing;
                status.attempts = job.attempts;
                status.last_error = None;
            }
            state.worker_running = true;
            job
        };

        let job_key_value = job_key(&job.source_path, &job.plugin_id);
        let plugin = registered_plugin(&inner.plugin_roots, &job.plugin_id);
        let result = indexer::index_file_with_plugin_paths(
            &job.source_path,
            &inner.plugin_roots,
            &inner.index_roots,
        );

        let mut state = inner.state.lock().expect("plugin runtime lock poisoned");
        state.processing_jobs.remove(&job_key_value);

        match result {
            Ok(_) => {
                state.statuses.insert(
                    job.source_path.clone(),
                    FileStatus {
                        plugin_id: job.plugin_id.clone(),
                        state: IndexedFileState::Ready,
                        attempts: job.attempts,
                        last_error: None,
                    },
                );
                state.failures.retain(|failure| {
                    !(failure.source_path == job.source_path.display().to_string()
                        && failure.plugin_id == job.plugin_id)
                });
            }
            Err(err) => {
                let message = err.to_string();
                if !job.source_path.exists() {
                    state.statuses.insert(
                        job.source_path.clone(),
                        FileStatus {
                            plugin_id: job.plugin_id.clone(),
                            state: IndexedFileState::Skipped,
                            attempts: job.attempts,
                            last_error: Some(message),
                        },
                    );
                } else {
                    let display = classify_index_error(&err);
                    let failure_state = save_failure_state(
                        &inner.index_roots[0],
                        &job.source_path,
                        &plugin,
                        job.attempts,
                        display.clone(),
                    )
                    .ok();
                    let failure = PluginIndexFailure {
                        source_path: job.source_path.display().to_string(),
                        plugin_id: job.plugin_id.clone(),
                        attempts: job.attempts,
                        code: failure_state
                            .as_ref()
                            .map(|state| state.code.clone())
                            .unwrap_or_else(|| display.code.clone()),
                        message: failure_state
                            .as_ref()
                            .map(|state| state.message.clone())
                            .unwrap_or_else(|| display.message.clone()),
                        details: failure_state
                            .as_ref()
                            .map(|state| state.details.clone())
                            .unwrap_or_else(|| display.details.clone()),
                        next_retry_at: failure_state
                            .as_ref()
                            .map(|state| state.next_retry_at.clone()),
                    };
                    upsert_failure(&mut state.failures, failure);
                    state.statuses.insert(
                        job.source_path.clone(),
                        FileStatus {
                            plugin_id: job.plugin_id.clone(),
                            state: IndexedFileState::Failed,
                            attempts: job.attempts,
                            last_error: Some(display.message),
                        },
                    );
                    if job.attempts < RETRY_LIMIT
                        && failure_state
                            .as_ref()
                            .map(|saved| {
                                retry_allowed(
                                    saved,
                                    &job.source_path,
                                    &plugin,
                                    std::time::SystemTime::now(),
                                )
                            })
                            .unwrap_or(false)
                    {
                        enqueue_pending_job(
                            &mut state,
                            PluginJob {
                                source_path: job.source_path.clone(),
                                plugin_id: job.plugin_id.clone(),
                                attempts: job.attempts + 1,
                            },
                        );
                    }
                }
            }
        }

        state.worker_running = false;
        inner.wake.notify_all();
        drop(state);
        persist_runtime_state(&inner);
        thread::sleep(WORKER_DELAY);
    });
}

fn promote_pending_jobs(state: &mut RuntimeState) {
    while state.jobs.len() < ACTIVE_QUEUE_TARGET {
        let Some(job) = state.pending_jobs.pop_front() else {
            break;
        };
        let key = job_key(&job.source_path, &job.plugin_id);
        state.pending_job_keys.remove(&key);
        state.queued_jobs.insert(key);
        if let Some(status) = state.statuses.get_mut(&job.source_path) {
            status.state = IndexedFileState::Queued;
            status.attempts = job.attempts;
        }
        state.jobs.push_back(job);
    }
}

fn enqueue_pending_job(state: &mut RuntimeState, job: PluginJob) {
    let key = job_key(&job.source_path, &job.plugin_id);
    if state.pending_job_keys.contains(&key)
        || state.queued_jobs.contains(&key)
        || state.processing_jobs.contains(&key)
    {
        return;
    }
    state.pending_job_keys.insert(key);
    state.statuses.insert(
        job.source_path.clone(),
        FileStatus {
            plugin_id: job.plugin_id.clone(),
            state: IndexedFileState::Pending,
            attempts: job.attempts.saturating_sub(1),
            last_error: None,
        },
    );
    state.pending_jobs.push_back(job);
}

fn scan_root(inner: &Arc<RuntimeInner>, root: &Path) -> Result<()> {
        let discovery = PluginRegistry::discover(&inner.plugin_roots)?;

        let classifier = FileClassifier::new(&discovery.registry);

    if root.is_file() {
        scan_file(inner, root, &classifier, &discovery.registry);
        return Ok(());
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
        if !entry.file_type().is_some_and(|file_type| file_type.is_file()) {
            continue;
        }
        scan_file(inner, entry.path(), &classifier, &discovery.registry);
    }

    Ok(())
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
) {
    let kind = classifier.classify(path);
    let FileKind::SupportedByPlugin { plugin_id } = kind else {
        return;
    };

    let Some(plugin) = registry.by_id.get(&plugin_id) else {
        return;
    };

    let validation = cache::validate_cache(path, plugin);
    if validation.status == CacheStatus::Ready {
        if let Some(index_root) = inner.index_roots.first() {
            let _ = remove_failure_state(index_root, path);
        }
        let mut state = inner.state.lock().expect("plugin runtime lock poisoned");
        state.statuses.insert(
            path.to_path_buf(),
            FileStatus {
                plugin_id: plugin.id.clone(),
                state: IndexedFileState::Ready,
                attempts: 0,
                last_error: None,
            },
        );
        return;
    }

    if let Some(index_root) = inner.index_roots.first() {
        if let Some(failure_state) = load_failure_state(index_root, path) {
            if !retry_allowed(&failure_state, path, plugin, std::time::SystemTime::now()) {
                let mut state = inner.state.lock().expect("plugin runtime lock poisoned");
                state.statuses.insert(
                    path.to_path_buf(),
                    FileStatus {
                        plugin_id: plugin.id.clone(),
                        state: IndexedFileState::Failed,
                        attempts: failure_state.attempts,
                        last_error: Some(failure_state.message.clone()),
                    },
                );
                upsert_failure(
                    &mut state.failures,
                    PluginIndexFailure {
                        source_path: path.display().to_string(),
                        plugin_id: plugin.id.clone(),
                        attempts: failure_state.attempts,
                        code: failure_state.code,
                        message: failure_state.message,
                        details: failure_state.details,
                        next_retry_at: Some(failure_state.next_retry_at),
                    },
                );
                return;
            }
        }
    }

    enqueue_job(inner, path, &plugin.id);
}

fn enqueue_job(inner: &Arc<RuntimeInner>, source_path: &Path, plugin_id: &str) {
    let mut state = inner.state.lock().expect("plugin runtime lock poisoned");
    enqueue_pending_job(
        &mut state,
        PluginJob {
            source_path: source_path.to_path_buf(),
            plugin_id: plugin_id.to_string(),
            attempts: 1,
        },
    );
    inner.wake.notify_all();
}

fn job_key(source_path: &Path, plugin_id: &str) -> String {
    format!("{}\0{plugin_id}", source_path.display())
}

fn classify_index_error(err: &anyhow::Error) -> crate::plugins::failure_state::FailureDisplay {
    err.downcast_ref::<IndexFailure>()
        .map(|failure| failure.display.clone())
        .unwrap_or_else(|| classify_failure(&err.to_string()))
}

fn upsert_failure(failures: &mut VecDeque<PluginIndexFailure>, failure: PluginIndexFailure) {
    if let Some(index) = failures
        .iter()
        .position(|existing| existing.source_path == failure.source_path && existing.plugin_id == failure.plugin_id)
    {
        failures.remove(index);
    }
    if failures.len() >= FAILURE_HISTORY_LIMIT {
        failures.pop_front();
    }
    failures.push_back(failure);
}

fn registered_plugin(plugin_roots: &[PathBuf], plugin_id: &str) -> crate::plugins::registry::RegisteredPlugin {
    PluginRegistry::discover(plugin_roots)
        .ok()
        .and_then(|report| report.registry.by_id.get(plugin_id).cloned())
        .unwrap_or_else(|| panic!("plugin {plugin_id} should be registered"))
}

fn discovered_plugins(plugin_roots: &[PathBuf]) -> Vec<InstalledPluginInfo> {
    let Ok(discovery) = PluginRegistry::discover(plugin_roots) else {
        return Vec::new();
    };

    let mut plugins = discovery
        .registry
        .by_id
        .values()
        .map(|plugin| InstalledPluginInfo {
            id: plugin.id.clone(),
            name: plugin.name.clone(),
            version: plugin.version.clone(),
            enabled: true,
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
    plugins.sort_by(|left, right| left.name.cmp(&right.name).then(left.version.cmp(&right.version)));
    plugins
}

#[cfg(test)]
mod tests {
    use super::PluginIndexRuntime;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::Duration;
    use tempfile::tempdir;

    #[cfg(unix)]
    fn write_plugin(root: &PathBuf, sleep_seconds: u64) {
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(
            root.join("plugin.toml"),
            r#"
schema = "sm.plugin.v1"
id = "sm.plugin.pdf"
name = "PDF Plugin"
version = "0.1.0"
handles = [".pdf"]
platforms = ["macos-arm64", "macos-x64", "linux-x64"]
timeout_seconds = 5

[entry]
kind = "process"
command = "sm-plugin-pdf"
args = ["--job"]
"#,
        )
        .unwrap();

        let script = format!(
            r#"#!/bin/sh
JOB_PATH="$2"
sleep {sleep_seconds}
python3 - "$JOB_PATH" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

job = json.load(open(sys.argv[1], "r", encoding="utf-8"))
source = Path(job["source_path"])
text = Path(job["output_text_path"])
meta = Path(job["output_meta_path"])
text.parent.mkdir(parents=True, exist_ok=True)
content = "plugin indexed text\n"
text.write_text(content, encoding="utf-8")
source_stat = source.stat()
text_stat = text.stat()
def mtime(stat):
    return datetime.fromtimestamp(stat.st_mtime, timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")
meta.write_text(json.dumps({{
    "schema": "sm.meta.v1",
    "source": {{
        "path": str(source),
        "size": source_stat.st_size,
        "mtime": mtime(source_stat)
    }},
    "generator": {{
        "plugin_id": job["plugin_id"],
        "plugin_version": "0.1.0"
    }},
    "text": {{
        "path": str(text),
        "encoding": "utf-8",
        "length_bytes": text_stat.st_size,
        "mtime": mtime(text_stat),
        "offsets": "utf8-bytes"
    }},
    "ranges": [
        {{"type": "document", "start": 0, "end": text_stat.st_size, "index": 1}},
        {{"type": "page", "start": 0, "end": text_stat.st_size, "page": 1, "index": 1}}
    ]
}}, indent=2), encoding="utf-8")
PY
"#
        );
        let script_path = root.join("bin/sm-plugin-pdf");
        fs::write(&script_path, script).unwrap();
        let mut perms = fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn scanner_indexes_supported_files_in_background() {
        let temp = tempdir().unwrap();
        let plugin_root = temp.path().join("plugins/sm.plugin.pdf/0.1.0");
        let index_root = temp.path().join("index");
        let source_root = temp.path().join("docs");
        fs::create_dir_all(&source_root).unwrap();
        fs::write(source_root.join("report.pdf"), b"%PDF-test").unwrap();
        write_plugin(&plugin_root, 0);

        let runtime = PluginIndexRuntime::new(vec![temp.path().join("plugins")], vec![index_root.clone()]);
        runtime.request_scan(&source_root).unwrap();
        assert!(runtime.wait_for_idle(Duration::from_secs(10)));

        let status = runtime.status();
        assert_eq!(status.ready_count, 1);
        assert_eq!(status.failed_count, 0);
        assert!(index_root.join("private").exists() || index_root.join("var").exists() || index_root.join("Users").exists());
    }

    #[cfg(unix)]
    #[test]
    fn worker_waits_until_search_is_idle() {
        let temp = tempdir().unwrap();
        let plugin_root = temp.path().join("plugins/sm.plugin.pdf/0.1.0");
        let index_root = temp.path().join("index");
        let source_root = temp.path().join("docs");
        fs::create_dir_all(&source_root).unwrap();
        let source_path = source_root.join("report.pdf");
        fs::write(&source_path, b"%PDF-test").unwrap();
        write_plugin(&plugin_root, 0);

        let runtime = PluginIndexRuntime::new(vec![temp.path().join("plugins")], vec![index_root.clone()]);
        runtime.search_started();
        runtime.request_scan(&source_root).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let status = runtime.status();
        assert_eq!(status.processing_count, 0);
        assert_eq!(status.queued_count, 1);

        runtime.search_finished();
        assert!(runtime.wait_for_idle(Duration::from_secs(10)));
        let final_status = runtime.status();
        assert_eq!(final_status.ready_count, 1);
        assert_eq!(final_status.queued_count, 0);
        let text_path = index_root
            .join(crate::plugins::index_paths::mirror_relative_path(
                &source_path.canonicalize().unwrap(),
            ))
            .with_file_name("report.pdf.sm.txt");
        assert!(text_path.is_file());
    }
}
