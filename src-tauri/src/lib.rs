pub mod search;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf, MAIN_SEPARATOR};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use search::{
    ripgrep::RipgrepSidecarProvider,
    runner::{run_rg_child, SearchRunOptions},
    FilePreview, FilePreviewLine, SearchMatch, SearchProvider, SearchRequest, SearchState,
    SearchStatus,
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
const RELEASE_NOTES_MENU_ID: &str = "release-notes";
const WEBSITE_MENU_ID: &str = "searchmonkey-website";
const REPORT_ISSUE_MENU_ID: &str = "report-issue";
const CHECK_FOR_UPDATES_MENU_ID: &str = "check-for-updates";

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
) -> Result<Vec<SearchMatch>, String> {
    let provider = RipgrepSidecarProvider::new(app);

    provider
        .search(request)
        .await
        .map_err(|err| err.to_string())
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
    let reader = BufReader::new(file);
    let mut lines = Vec::new();
    let mut saw_after_window = false;

    for (index, line) in reader.lines().enumerate() {
        let number = index as u64 + 1;

        if number > PREVIEW_MAX_SCAN_LINES {
            return Err(
                "Preview skipped because the match is too deep in a large file.".to_string(),
            );
        }

        if number < start_line {
            line.map_err(|err| err.to_string())?;
            continue;
        }

        if number > end_line {
            saw_after_window = true;
            break;
        }

        let text = line.map_err(|err| err.to_string())?;

        lines.push(FilePreviewLine {
            number,
            text,
            is_match: false,
            match_ranges: Vec::new(),
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
) -> Result<u64, String> {
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

    thread::spawn(move || {
        let summary = run_rg_child(
            child,
            stdout,
            SearchRunOptions {
                search_id,
                result_limit,
                modified_after,
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .menu(|app| {
            let app_menu = SubmenuBuilder::new(app, "Searchmonkey III")
                .text(ABOUT_SEARCHMONKEY_MENU_ID, "About Searchmonkey III")
                .separator()
                .quit()
                .build()?;
            let help_menu = SubmenuBuilder::new(app, "Help")
                .text(CHECK_FOR_UPDATES_MENU_ID, "Check for Updates...")
                .separator()
                .text(RELEASE_NOTES_MENU_ID, "Release Notes")
                .text(WEBSITE_MENU_ID, "Searchmonkey Website")
                .text(REPORT_ISSUE_MENU_ID, "Report an Issue")
                .separator()
                .text(IMPROVE_MENU_ID, "Improve Searchmonkey")
                .build()?;
            let edit_menu = SubmenuBuilder::new(app, "Edit")
                .cut()
                .copy()
                .paste()
                .select_all()
                .build()?;

            MenuBuilder::new(app)
                .item(&app_menu)
                .item(&edit_menu)
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
        })
        .manage(SearchSessions::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            cancel_search,
            clear_search,
            copy_text,
            get_results,
            get_search_status,
            home_dir,
            list_directory,
            open_file_path,
            read_file_preview,
            reveal_file_path,
            search_files,
            start_search
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
