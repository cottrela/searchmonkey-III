mod search;

use std::io::{BufRead, BufReader};
use std::process::Child;
use std::sync::Mutex;
use std::thread;

use search::{
    ripgrep::RipgrepSidecarProvider, SearchMatch, SearchProvider, SearchRequest, SearchStreamEvent,
};
use tauri::{ipc::Channel, Manager, State};

const SEARCH_BATCH_SIZE: usize = 100;
const UI_RESULT_LIMIT: usize = 1_000;

#[derive(Default)]
struct SearchRuntime {
    current: Mutex<Option<RunningSearch>>,
}

struct RunningSearch {
    id: u64,
    child: Option<Child>,
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
    let mut child = provider.spawn(request).map_err(|err| err.to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "ripgrep stdout was not available".to_string())?;
    let stderr = child.stderr.take();

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

    let app_for_stdout = app.clone();
    let events_for_stdout = events.clone();
    thread::spawn(move || {
        let mut total_matches = 0usize;
        let mut batch = Vec::with_capacity(SEARCH_BATCH_SIZE);
        let reader = BufReader::new(stdout);

        for line in reader.split(b'\n') {
            if !is_active_search(&app_for_stdout, search_id) {
                break;
            }

            let Ok(line) = line else {
                continue;
            };

            let Some(result) = RipgrepSidecarProvider::parse_match(&line) else {
                continue;
            };

            total_matches += 1;
            if total_matches <= UI_RESULT_LIMIT {
                batch.push(result);
            }

            if batch.len() >= SEARCH_BATCH_SIZE {
                emit_batch(&events_for_stdout, search_id, &mut batch);
            }

            if total_matches >= UI_RESULT_LIMIT {
                break;
            }
        }

        emit_batch(&events_for_stdout, search_id, &mut batch);

        if let Some(mut child) = take_active_child(&app_for_stdout, search_id) {
            let _ = child.kill();
            let _ = child.wait();
        }

        if is_active_search(&app_for_stdout, search_id) {
            clear_active_search(&app_for_stdout, search_id);
        }

        let _ = events_for_stdout.send(SearchStreamEvent::Finished {
            search_id,
            total_matches,
        });
    });

    if let Some(stderr) = stderr {
        let app_for_stderr = app.clone();
        let events_for_stderr = events.clone();
        thread::spawn(move || {
            let reader = BufReader::new(stderr);

            for line in reader.lines() {
                if !is_active_search(&app_for_stderr, search_id) {
                    break;
                }

                let Ok(message) = line else {
                    continue;
                };

                if !message.trim().is_empty() {
                    let _ = events_for_stderr.send(SearchStreamEvent::Error { search_id, message });
                }
            }
        });
    }

    Ok(search_id)
}

fn emit_batch(
    events: &Channel<SearchStreamEvent>,
    search_id: u64,
    batch: &mut Vec<SearchMatch>,
) {
    if batch.is_empty() {
        return;
    }

    let results = std::mem::take(batch);
    let _ = events.send(SearchStreamEvent::Batch { search_id, results });
}

fn take_active_child(app: &tauri::AppHandle, search_id: u64) -> Option<Child> {
    app.state::<SearchRuntime>()
        .current
        .lock()
        .ok()
        .and_then(|mut current| {
            if current.as_ref().is_some_and(|running| running.id == search_id) {
                current.as_mut().and_then(|running| running.child.take())
            } else {
                None
            }
        })
}

fn clear_active_search(app: &tauri::AppHandle, search_id: u64) {
    if let Ok(mut current) = app.state::<SearchRuntime>().current.lock() {
        if current.as_ref().is_some_and(|running| running.id == search_id) {
            current.take();
        }
    }
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

fn kill_child(mut child: Child) -> Result<(), String> {
    let pid = child.id();
    let kill_result = child.kill().map_err(|err| err.to_string());

    #[cfg(unix)]
    if kill_result.is_ok() {
        let _ = std::process::Command::new("kill")
            .arg("-KILL")
            .arg(pid.to_string())
            .status();
    }

    let _ = child.wait();
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
