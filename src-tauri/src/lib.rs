pub mod plugins;
pub mod search;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf, MAIN_SEPARATOR};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use plugins::{
    classifier::meta_for_sm_text,
    indexer,
    registry::PluginRegistry,
    runtime::PluginIndexRuntime,
    meta::{SmMeta, SmRangeType},
};
use search::{
    ripgrep::RipgrepSidecarProvider,
    runner::{run_rg_child, SearchRunOptions},
    FilePreview, FilePreviewLine, FilePreviewPageBreak, SearchMatch, SearchProvider, SearchRequest,
    SearchState, SearchStatus,
};
use tauri::{
    menu::{MenuBuilder, SubmenuBuilder},
    Emitter, State,
};

const UI_RESULT_LIMIT: usize = 100_000;
const PREVIEW_MAX_SCAN_LINES: u64 = 250_000;
const DIRECTORY_SUGGESTION_LIMIT: usize = 500;
const IMPROVE_MENU_ID: &str = "improve-searchmonkey";
const ABOUT_SEARCHMONKEY_MENU_ID: &str = "about-searchmonkey-iii";
const REGEX_CHEAT_SHEET_MENU_ID: &str = "regex-cheat-sheet";
const RELEASE_NOTES_MENU_ID: &str = "release-notes";
const WEBSITE_MENU_ID: &str = "searchmonkey-website";
const REPORT_ISSUE_MENU_ID: &str = "report-issue";
const CHECK_FOR_UPDATES_MENU_ID: &str = "check-for-updates";
const MANAGE_PLUGINS_MENU_ID: &str = "manage-plugins";
const INSTALL_PLUGIN_MENU_ID: &str = "install-plugin";
const PAUSE_BACKGROUND_INDEXING_MENU_ID: &str = "pause-background-indexing";
const REBUILD_PLUGIN_CACHE_MENU_ID: &str = "rebuild-plugin-cache";
const OPEN_PLUGIN_FOLDER_MENU_ID: &str = "open-plugin-folder";
const PLUGIN_MENU_ITEM_PREFIX: &str = "plugin-entry:";

#[derive(Default)]
struct SearchSessions {
    next_id: AtomicU64,
    sessions: Mutex<std::collections::HashMap<u64, Arc<SearchSession>>>,
}

struct SearchSession {
    status: Mutex<SearchStatus>,
    results: Mutex<Vec<SearchMatch>>,
    child_pid: Mutex<Option<u32>>,
}

impl SearchSession {
    fn new(search_id: u64) -> Self {
        Self {
            status: Mutex::new(SearchStatus {
                search_id,
                state: SearchState::Starting,
                total_matches: 0,
                error_message: None,
            }),
            results: Mutex::new(Vec::new()),
            child_pid: Mutex::new(None),
        }
    }
}

#[tauri::command]
async fn search_files(
    app: tauri::AppHandle,
    request: SearchRequest,
    plugin_index: State<'_, PluginIndexRuntime>,
) -> Result<Vec<SearchMatch>, String> {
    queue_plugin_scan_for_path(&plugin_index, &request.path);
    let provider = RipgrepSidecarProvider::new(app);
    plugin_index.search_started();

    let result = provider
        .search(request)
        .await
        .map_err(|err| err.to_string());
    plugin_index.search_finished();
    result
}

#[tauri::command]
async fn read_file_preview(
    path: String,
    start_line: u64,
    end_line: u64,
) -> Result<FilePreview, String> {
    if start_line == 0 || end_line == 0 || start_line > end_line {
        return Err("Preview line range is invalid.".to_string());
    }

    tauri::async_runtime::spawn_blocking(move || {
        read_file_preview_range(path, start_line, end_line)
    })
    .await
    .map_err(|err| err.to_string())?
}

fn read_file_preview_range(
    path: String,
    start_line: u64,
    end_line: u64,
) -> Result<FilePreview, String> {
    let file = std::fs::File::open(&path).map_err(|err| err.to_string())?;
    let mut reader = BufReader::new(file);
    let preview_meta = load_preview_meta(Path::new(&path));
    let mut lines = Vec::new();
    let mut saw_after_window = false;
    let mut buffer = Vec::new();
    let mut offset = 0u64;
    let mut number = 0u64;

    loop {
        buffer.clear();
        let bytes_read = reader
            .read_until(b'\n', &mut buffer)
            .map_err(|err| err.to_string())?;
        if bytes_read == 0 {
            break;
        }

        number += 1;
        let line_start = offset;
        offset += bytes_read as u64;

        if number > PREVIEW_MAX_SCAN_LINES {
            return Err(
                "Preview skipped because the match is too deep in a large file.".to_string(),
            );
        }

        if number < start_line {
            continue;
        }

        if number > end_line {
            saw_after_window = true;
            break;
        }

        let trimmed = trim_line_ending(&buffer);
        let text = String::from_utf8_lossy(trimmed).to_string();

        lines.push(FilePreviewLine {
            number,
            text,
            is_match: false,
            match_ranges: Vec::new(),
            page_breaks: preview_meta
                .as_ref()
                .map(|meta| page_breaks_for_line(meta, line_start, trimmed.len() as u64))
                .unwrap_or_default(),
        });
    }

    let actual_end_line = lines.last().map(|line| line.number).unwrap_or(start_line);

    Ok(FilePreview {
        path,
        start_line,
        end_line: actual_end_line,
        lines,
        truncated: start_line > 1 || saw_after_window,
    })
}

fn trim_line_ending(bytes: &[u8]) -> &[u8] {
    if bytes.ends_with(b"\r\n") {
        &bytes[..bytes.len() - 2]
    } else if bytes.ends_with(b"\n") {
        &bytes[..bytes.len() - 1]
    } else {
        bytes
    }
}

fn load_preview_meta(path: &Path) -> Option<SmMeta> {
    let meta_path = meta_for_sm_text(path)?;
    SmMeta::load(meta_path).ok()
}

fn page_breaks_for_line(
    meta: &SmMeta,
    line_start: u64,
    line_length: u64,
) -> Vec<FilePreviewPageBreak> {
    let line_end = line_start + line_length;
    let mut page_breaks = meta
        .ranges
        .iter()
        .filter(|range| range.kind == SmRangeType::PageBreak)
        .filter(|range| line_start <= range.start && range.start < line_end)
        .map(|range| FilePreviewPageBreak {
            page: range.page,
            label: range.label.clone(),
        })
        .collect::<Vec<_>>();
    page_breaks.sort_by_key(|page_break| page_break.page.unwrap_or(0));
    page_breaks
}

#[tauri::command]
fn home_dir() -> Result<String, String> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| "Could not resolve the current user's home directory".to_string())
}

#[tauri::command]
async fn list_directory(path: String, include_hidden: bool) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || list_directory_entries(path, include_hidden))
        .await
        .map_err(|err| err.to_string())?
}

fn list_directory_entries(path: String, include_hidden: bool) -> Result<Vec<String>, String> {
    let path = expand_home_path(&path)?;
    let entries = std::fs::read_dir(path).map_err(|err| err.to_string())?;
    let mut suggestions = Vec::new();

    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };

        let name = entry.file_name().to_string_lossy().to_string();

        if name.is_empty() {
            continue;
        }

        if !include_hidden && name.starts_with('.') {
            continue;
        }

        if entry.path().is_dir() {
            suggestions.push(name);
        }
    }

    suggestions.sort_by_key(|name| name.to_lowercase());

    Ok(suggestions
        .into_iter()
        .take(DIRECTORY_SUGGESTION_LIMIT)
        .map(|name| format!("{name}{MAIN_SEPARATOR}"))
        .collect())
}

fn expand_home_path(path: &str) -> Result<PathBuf, String> {
    if path == "~" {
        return home_dir().map(PathBuf::from);
    }

    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir().map(|home| Path::new(&home).join(rest));
    }

    Ok(PathBuf::from(path))
}

#[tauri::command]
async fn open_file_path(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || open_path_native(path))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn index_file_with_plugin(source_path: String) -> Result<indexer::IndexResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let source_path = expand_home_path(source_path.trim())?;
        indexer::index_file_with_plugin(&source_path).map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
fn get_plugin_index_status(
    plugin_index: State<'_, PluginIndexRuntime>,
) -> Result<plugins::runtime::PluginIndexStatus, String> {
    Ok(plugin_index.status())
}

#[tauri::command]
fn queue_plugin_scan(
    plugin_index: State<'_, PluginIndexRuntime>,
    path: String,
) -> Result<plugins::runtime::PluginIndexStatus, String> {
    let path = expand_home_path(path.trim())?;
    if path.exists() {
        if path.is_file() {
            plugin_index
                .request_retry(&path)
                .map_err(|err| err.to_string())?;
        } else {
            plugin_index
                .request_scan(&path)
                .map_err(|err| err.to_string())?;
        }
    }
    Ok(plugin_index.status())
}

#[tauri::command]
fn ignore_plugin_issue(
    plugin_index: State<'_, PluginIndexRuntime>,
    path: String,
    plugin_id: String,
) -> Result<plugins::runtime::PluginIndexStatus, String> {
    let path = expand_home_path(path.trim())?;
    plugin_index
        .ignore_issue(&path, plugin_id.trim())
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn unignore_plugin_issue(
    plugin_index: State<'_, PluginIndexRuntime>,
    path: String,
    plugin_id: String,
) -> Result<plugins::runtime::PluginIndexStatus, String> {
    let path = expand_home_path(path.trim())?;
    plugin_index
        .unignore_issue(&path, plugin_id.trim())
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn set_plugin_index_paused(
    plugin_index: State<'_, PluginIndexRuntime>,
    paused: bool,
) -> Result<plugins::runtime::PluginIndexStatus, String> {
    Ok(plugin_index.set_paused(paused))
}

#[tauri::command]
fn rebuild_plugin_index(
    plugin_index: State<'_, PluginIndexRuntime>,
) -> Result<plugins::runtime::PluginIndexStatus, String> {
    Ok(plugin_index.rebuild())
}

#[tauri::command]
fn plugin_folder_path(plugin_index: State<'_, PluginIndexRuntime>) -> Result<String, String> {
    plugin_index
        .default_plugin_folder()
        .map(|path| path.display().to_string())
        .ok_or_else(|| "Could not resolve the plugin folder.".to_string())
}

#[tauri::command]
fn set_active_plugin_version(
    plugin_index: State<'_, PluginIndexRuntime>,
    plugin_id: String,
    version: String,
) -> Result<plugins::runtime::PluginIndexStatus, String> {
    plugin_index
        .set_active_plugin_version(plugin_id.trim(), version.trim())
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn uninstall_plugin_version(
    plugin_index: State<'_, PluginIndexRuntime>,
    plugin_id: String,
    version: String,
) -> Result<plugins::runtime::PluginIndexStatus, String> {
    plugin_index
        .uninstall_plugin_version(plugin_id.trim(), version.trim())
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn reveal_file_path(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || reveal_path_native(path))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn copy_text(text: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || copy_text_native(&text))
        .await
        .map_err(|err| err.to_string())?
}

fn existing_path(path: String) -> Result<PathBuf, String> {
    let path = expand_home_path(path.trim())?;

    if path.exists() {
        Ok(path)
    } else {
        Err(format!("Path does not exist: {}", path.display()))
    }
}

#[cfg(target_os = "macos")]
fn open_path_native(path: String) -> Result<(), String> {
    run_native_command(Command::new("open").arg(existing_path(path)?))
}

#[cfg(target_os = "macos")]
fn reveal_path_native(path: String) -> Result<(), String> {
    run_native_command(Command::new("open").arg("-R").arg(existing_path(path)?))
}

#[cfg(target_os = "windows")]
fn open_path_native(path: String) -> Result<(), String> {
    let path = existing_path(path)?.to_string_lossy().to_string();
    run_native_command(Command::new("cmd").args(["/C", "start", "", &path]))
}

#[cfg(target_os = "windows")]
fn reveal_path_native(path: String) -> Result<(), String> {
    let path = existing_path(path)?.to_string_lossy().to_string();
    run_native_command(Command::new("explorer").arg(format!("/select,{path}")))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_path_native(path: String) -> Result<(), String> {
    run_native_command(Command::new("xdg-open").arg(existing_path(path)?))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn reveal_path_native(path: String) -> Result<(), String> {
    let path = existing_path(path)?;
    let directory = if path.is_dir() {
        path
    } else {
        path.parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "Could not resolve containing directory.".to_string())?
    };

    run_native_command(Command::new("xdg-open").arg(directory))
}

fn run_native_command(command: &mut Command) -> Result<(), String> {
    let status = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| err.to_string())?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("Command failed with status {status}"))
    }
}

#[cfg(target_os = "macos")]
fn copy_text_native(text: &str) -> Result<(), String> {
    write_to_clipboard_command("pbcopy", &[], text)
}

#[cfg(target_os = "windows")]
fn copy_text_native(text: &str) -> Result<(), String> {
    write_to_clipboard_command("clip", &[], text)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn copy_text_native(text: &str) -> Result<(), String> {
    write_to_clipboard_command("wl-copy", &[], text)
        .or_else(|_| write_to_clipboard_command("xclip", &["-selection", "clipboard"], text))
        .or_else(|_| write_to_clipboard_command("xsel", &["--clipboard", "--input"], text))
}

fn write_to_clipboard_command(program: &str, args: &[&str], text: &str) -> Result<(), String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| err.to_string())?;

    child
        .stdin
        .as_mut()
        .ok_or_else(|| "Clipboard command stdin was unavailable.".to_string())?
        .write_all(text.as_bytes())
        .map_err(|err| err.to_string())?;

    let status = child.wait().map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Clipboard command failed with status {status}"))
    }
}

#[tauri::command]
async fn start_search(
    app: tauri::AppHandle,
    request: SearchRequest,
    sessions: State<'_, SearchSessions>,
    plugin_index: State<'_, PluginIndexRuntime>,
) -> Result<u64, String> {
    queue_plugin_scan_for_path(&plugin_index, &request.path);
    let search_id = sessions.next_id.fetch_add(1, Ordering::Relaxed) + 1;
    let session = Arc::new(SearchSession::new(search_id));
    let provider = RipgrepSidecarProvider::new(app.clone());
    let result_limit = request.max_matches.unwrap_or(UI_RESULT_LIMIT).max(1);
    let modified_after = request.modified_after;
    let mut child = provider.spawn(request).map_err(|err| err.to_string())?;
    let child_pid = child.id();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "ripgrep stdout was not available".to_string())?;
    *session
        .child_pid
        .lock()
        .map_err(|_| "search process handle is unavailable".to_string())? = Some(child_pid);
    sessions
        .sessions
        .lock()
        .map_err(|_| "search session store is unavailable".to_string())?
        .insert(search_id, session.clone());

    set_search_state(&session, SearchState::Running, None);
    plugin_index.search_started();
    let plugin_index = plugin_index.inner().clone();

    thread::spawn(move || {
        let summary = run_rg_child(
            child,
            stdout,
            SearchRunOptions {
                search_id,
                result_limit,
                modified_after,
                plugin_registry: provider.plugin_registry(),
            },
            |result, total_matches| {
                if let Ok(mut results) = session.results.lock() {
                    results.push(result);
                }
                if let Ok(mut status) = session.status.lock() {
                    status.total_matches = total_matches;
                }
            },
        );

        if let Ok(mut status) = session.status.lock() {
            status.total_matches = summary.total_matches;
        }
        let current_state = session
            .status
            .lock()
            .ok()
            .map(|status| status.state.clone())
            .unwrap_or(SearchState::Failed);
        let final_state = if current_state == SearchState::Cancelling {
            SearchState::Cancelled
        } else {
            summary.final_state
        };
        set_search_state(&session, final_state, summary.error_message);
        if let Ok(mut session_child_pid) = session.child_pid.lock() {
            *session_child_pid = None;
        }
        plugin_index.search_finished();
    });

    Ok(search_id)
}

#[tauri::command]
fn get_search_status(
    sessions: State<'_, SearchSessions>,
    search_id: u64,
) -> Result<SearchStatus, String> {
    let session = find_session(&sessions, search_id)?;
    session
        .status
        .lock()
        .map(|status| status.clone())
        .map_err(|_| "search status is unavailable".to_string())
}

#[tauri::command]
fn get_results(
    sessions: State<'_, SearchSessions>,
    search_id: u64,
    offset: usize,
    limit: usize,
) -> Result<Vec<SearchMatch>, String> {
    let session = find_session(&sessions, search_id)?;
    let results = session
        .results
        .lock()
        .map_err(|_| "search results are unavailable".to_string())?;
    if offset >= results.len() {
        return Ok(Vec::new());
    }

    let end = offset.saturating_add(limit).min(results.len());
    Ok(results[offset..end].to_vec())
}

#[tauri::command]
fn cancel_search(sessions: State<'_, SearchSessions>, search_id: u64) -> Result<(), String> {
    let session = find_session(&sessions, search_id)?;
    set_search_state(&session, SearchState::Cancelling, None);

    let child_pid = session
        .child_pid
        .lock()
        .map_err(|_| "search process handle is unavailable".to_string())?
        .to_owned();
    if let Some(child_pid) = child_pid {
        kill_search_process(child_pid).map_err(|err| err.to_string())?;
    }

    Ok(())
}

#[tauri::command]
fn clear_search(sessions: State<'_, SearchSessions>, search_id: u64) -> Result<(), String> {
    let session = sessions
        .sessions
        .lock()
        .map_err(|_| "search session store is unavailable".to_string())?
        .remove(&search_id);
    if let Some(session) = session {
        let child_pid = session
            .child_pid
            .lock()
            .map_err(|_| "search process handle is unavailable".to_string())?
            .to_owned();
        if let Some(child_pid) = child_pid {
            let _ = kill_search_process(child_pid);
        }
    }

    Ok(())
}

fn find_session(
    sessions: &State<'_, SearchSessions>,
    search_id: u64,
) -> Result<Arc<SearchSession>, String> {
    sessions
        .sessions
        .lock()
        .map_err(|_| "search session store is unavailable".to_string())?
        .get(&search_id)
        .cloned()
        .ok_or_else(|| "search session was not found".to_string())
}

fn set_search_state(session: &SearchSession, state: SearchState, error_message: Option<String>) {
    if let Ok(mut status) = session.status.lock() {
        status.state = state;
        status.error_message = error_message;
    }
}

fn kill_search_process(pid: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let pid = pid as i32;
        unsafe {
            if libc::kill(-pid, libc::SIGTERM) == 0 {
                return Ok(());
            }
        }

        unsafe {
            if libc::kill(pid, libc::SIGTERM) == 0 {
                return Ok(());
            }
        }

        return Err(std::io::Error::last_os_error());
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "cancel by process id is not supported on this platform",
        ))
    }
}

fn queue_plugin_scan_for_path(plugin_index: &PluginIndexRuntime, path: &str) {
    let Ok(path) = expand_home_path(path.trim()) else {
        return;
    };
    if path.exists() {
        let _ = plugin_index.request_scan(&path);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let plugin_discovery = PluginRegistry::discover_default().unwrap_or_default();
    let mut installed_plugin_labels = plugin_discovery
        .registry
        .by_id
        .values()
        .map(|plugin| {
            (
                format!("{PLUGIN_MENU_ITEM_PREFIX}{}", plugin.id),
                format!("{}        ✓ Enabled", plugin.name),
            )
        })
        .collect::<Vec<_>>();
    installed_plugin_labels.sort_by(|left, right| left.1.cmp(&right.1));

    tauri::Builder::default()
        .menu(move |app| {
            let app_menu = SubmenuBuilder::new(app, "Searchmonkey III")
                .text(ABOUT_SEARCHMONKEY_MENU_ID, "About Searchmonkey III")
                .separator()
                .quit()
                .build()?;
            let help_menu = SubmenuBuilder::new(app, "Help")
                .text(REGEX_CHEAT_SHEET_MENU_ID, "Regex Cheat Sheet")
                .separator()
                .text(CHECK_FOR_UPDATES_MENU_ID, "Check for Updates...")
                .separator()
                .text(RELEASE_NOTES_MENU_ID, "Release Notes")
                .text(WEBSITE_MENU_ID, "Searchmonkey Website")
                .text(REPORT_ISSUE_MENU_ID, "Report an Issue")
                .separator()
                .text(IMPROVE_MENU_ID, "Improve Searchmonkey")
                .build()?;
            let edit_menu = SubmenuBuilder::new(app, "Edit")
                .undo()
                .redo()
                .separator()
                .cut()
                .copy()
                .paste()
                .select_all()
                .build()?;
            let mut plugins_menu = SubmenuBuilder::new(app, "Plugins")
                .text(MANAGE_PLUGINS_MENU_ID, "Manage Plugins…")
                .text(INSTALL_PLUGIN_MENU_ID, "Install Plugin…")
                .separator();
            for (id, label) in &installed_plugin_labels {
                plugins_menu = plugins_menu.text(id, label);
            }
            let plugins_menu = plugins_menu
                .separator()
                .text(PAUSE_BACKGROUND_INDEXING_MENU_ID, "Pause Background Processing")
                .text(REBUILD_PLUGIN_CACHE_MENU_ID, "Reset Processing Cache")
                .text(OPEN_PLUGIN_FOLDER_MENU_ID, "Open Plugin Folder")
                .build()?;

            MenuBuilder::new(app)
                .item(&app_menu)
                .item(&edit_menu)
                .item(&plugins_menu)
                .item(&help_menu)
                .build()
        })
        .on_menu_event(|app, event| {
            if event.id() == IMPROVE_MENU_ID {
                let _ = app.emit("open-improve-searchmonkey", ());
            }

            if event.id() == ABOUT_SEARCHMONKEY_MENU_ID {
                let _ = app.emit("open-about-searchmonkey", ());
            }

            if event.id() == REGEX_CHEAT_SHEET_MENU_ID {
                let _ = app.emit("open-regex-cheat-sheet", ());
            }

            if event.id() == RELEASE_NOTES_MENU_ID {
                let _ = app.emit("open-release-notes", ());
            }

            if event.id() == WEBSITE_MENU_ID {
                let _ = app.emit("open-searchmonkey-website", ());
            }

            if event.id() == REPORT_ISSUE_MENU_ID {
                let _ = app.emit("open-report-issue", ());
            }

            if event.id() == CHECK_FOR_UPDATES_MENU_ID {
                let _ = app.emit("check-for-updates", ());
            }

            if event.id() == MANAGE_PLUGINS_MENU_ID {
                let _ = app.emit("open-manage-plugins", Option::<String>::None);
            }

            if event.id() == INSTALL_PLUGIN_MENU_ID {
                let _ = app.emit("open-install-plugin", ());
            }

            if event.id() == PAUSE_BACKGROUND_INDEXING_MENU_ID {
                let _ = app.emit("toggle-plugin-indexing", ());
            }

            if event.id() == REBUILD_PLUGIN_CACHE_MENU_ID {
                let _ = app.emit("rebuild-plugin-index", ());
            }

            if event.id() == OPEN_PLUGIN_FOLDER_MENU_ID {
                let _ = app.emit("open-plugin-folder", ());
            }

            if let Some(plugin_id) = event.id().0.strip_prefix(PLUGIN_MENU_ITEM_PREFIX) {
                let _ = app.emit("open-manage-plugins", Some(plugin_id.to_string()));
            }
        })
        .manage(SearchSessions::default())
        .manage(PluginIndexRuntime::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            cancel_search,
            clear_search,
            copy_text,
            get_results,
            get_plugin_index_status,
            get_search_status,
            home_dir,
            ignore_plugin_issue,
            index_file_with_plugin,
            list_directory,
            open_file_path,
            plugin_folder_path,
            queue_plugin_scan,
            read_file_preview,
            rebuild_plugin_index,
            reveal_file_path,
            search_files,
            set_active_plugin_version,
            set_plugin_index_paused,
            start_search,
            uninstall_plugin_version,
            unignore_plugin_issue
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
