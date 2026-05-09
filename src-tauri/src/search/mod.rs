pub mod ripgrep;
pub mod runner;

use serde::{Deserialize, Serialize};

pub fn debug_logging_enabled() -> bool {
    matches!(
        std::env::var("SEARCHMONKEY_DEBUG").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub path: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub hidden: bool,
    pub include_patterns: Vec<String>,
    pub exclude_patterns: Vec<String>,
    pub follow_symlinks: bool,
    pub multiline: bool,
    pub context_lines: u64,
    pub min_file_size: String,
    pub max_file_size: String,
    pub modified_after: Option<u64>,
    pub skip_binary: bool,
    pub encoding: String,
    pub max_matches: Option<usize>,
    pub respect_gitignore: bool,
    pub ignore_node_modules: bool,
    pub ignore_build_artifacts: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchMatch {
    pub path: String,
    pub line_number: u64,
    pub line_text: String,
    pub submatches: Vec<SearchSubmatch>,
    pub file_size: Option<u64>,
    pub modified_secs: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SearchSubmatch {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilePreview {
    pub path: String,
    pub start_line: u64,
    pub end_line: u64,
    pub lines: Vec<FilePreviewLine>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilePreviewLine {
    pub number: u64,
    pub text: String,
    pub is_match: bool,
    pub match_ranges: Vec<SearchSubmatch>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum SearchState {
    Starting,
    Running,
    Cancelling,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchStatus {
    pub search_id: u64,
    pub state: SearchState,
    pub total_matches: usize,
    pub error_message: Option<String>,
}

#[async_trait::async_trait]
pub trait SearchProvider: Send + Sync {
    async fn search(&self, request: SearchRequest) -> anyhow::Result<Vec<SearchMatch>>;
}
