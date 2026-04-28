mod search;

use std::sync::Mutex;

use search::{
    ripgrep::RipgrepSidecarProvider, SearchMatch, SearchProvider, SearchRequest, SearchStreamEvent,
};
use tauri::{ipc::Channel, Manager, State};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};

#[derive(Default)]
struct SearchRuntime {
    current: Mutex<Option<RunningSearch>>,
}

struct RunningSearch {
    id: u64,
    child: Option<CommandChild>,
    events: Channel<SearchStreamEvent>,
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
async fn start_search(
    app: tauri::AppHandle,
    runtime: State<'_, SearchRuntime>,
    request: SearchRequest,
    search_id: u64,
    events: Channel<SearchStreamEvent>,
) -> Result<u64, String> {
    {
        let current = runtime
            .current
            .lock()
            .map_err(|_| "Search state is unavailable".to_string())?;

        if current.is_some() {
            return Err("A search is already running. Stop it before starting another.".to_string());
        }
    }

    let provider = RipgrepSidecarProvider::new(app.clone());
    let (mut rx, child) = provider.spawn(request).map_err(|err| err.to_string())?;

    {
        let mut current = runtime
            .current
            .lock()
            .map_err(|_| "Search state is unavailable".to_string())?;

        *current = Some(RunningSearch {
            id: search_id,
            child: Some(child),
            events: events.clone(),
        });
    }

    events
        .send(SearchStreamEvent::Started { search_id })
        .map_err(|err| err.to_string())?;

    let app_for_task = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut total_matches = 0usize;
        let mut stderr_messages = Vec::new();

        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(line) => {
                    if let Some(result) = RipgrepSidecarProvider::parse_match(&line) {
                        if !is_active_search(&app_for_task, search_id) {
                            continue;
                        }

                        total_matches += 1;
                        let _ = events.send(SearchStreamEvent::Match { search_id, result });
                    }
                }
                CommandEvent::Stderr(line) => {
                    let message = String::from_utf8_lossy(&line).trim().to_string();
                    if !message.is_empty() {
                        stderr_messages.push(message.clone());
                        if is_active_search(&app_for_task, search_id) {
                            let _ = events.send(SearchStreamEvent::Error { search_id, message });
                        }
                    }
                }
                CommandEvent::Error(message) => {
                    if is_active_search(&app_for_task, search_id) {
                        let _ = events.send(SearchStreamEvent::Error { search_id, message });
                    }
                }
                CommandEvent::Terminated(_) => break,
                _ => {}
            }
        }

        let should_finish = app_for_task
            .state::<SearchRuntime>()
            .current
            .lock()
            .ok()
            .and_then(|mut current| {
                if current.as_ref().is_some_and(|running| running.id == search_id) {
                    current.take();
                    Some(())
                } else {
                    None
                }
            })
            .is_some();

        if !should_finish {
            return;
        }

        if !stderr_messages.is_empty() && total_matches == 0 {
            let _ = events.send(SearchStreamEvent::Error {
                search_id,
                message: stderr_messages.join("\n"),
            });
        }

        let _ = events.send(SearchStreamEvent::Finished {
            search_id,
            total_matches,
        });
    });

    Ok(search_id)
}

#[tauri::command]
async fn stop_search(
    runtime: State<'_, SearchRuntime>,
    search_id: u64,
) -> Result<(), String> {
    let running = {
        let mut current = runtime
            .current
            .lock()
            .map_err(|_| "Search state is unavailable".to_string())?;

        match current.as_ref().map(|running| running.id) {
            Some(active_id) if active_id == search_id => current.take(),
            Some(_) => return Err("The active search does not match the stop request.".to_string()),
            None => None,
        }
    };

    if let Some(mut running) = running {
        if let Some(child) = running.child.take() {
            kill_child(child)?;
        }

        running
            .events
            .send(SearchStreamEvent::Cancelled {
                search_id,
                total_matches: 0,
            })
            .map_err(|err| err.to_string())?;
    }

    Ok(())
}

fn is_active_search(app: &tauri::AppHandle, search_id: u64) -> bool {
    app.state::<SearchRuntime>()
        .current
        .lock()
        .is_ok_and(|current| current.as_ref().is_some_and(|running| running.id == search_id))
}

fn kill_child(child: CommandChild) -> Result<(), String> {
    let pid = child.pid();
    let kill_result = child.kill().map_err(|err| err.to_string());

    #[cfg(unix)]
    if kill_result.is_ok() {
        let _ = std::process::Command::new("kill")
            .arg("-KILL")
            .arg(pid.to_string())
            .status();
    }

    kill_result
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(SearchRuntime::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            search_files,
            start_search,
            stop_search
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
