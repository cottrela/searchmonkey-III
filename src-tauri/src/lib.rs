mod search;

use search::{ripgrep::RipgrepSidecarProvider, SearchMatch, SearchProvider, SearchRequest};

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![search_files])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
