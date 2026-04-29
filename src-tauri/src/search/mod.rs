pub mod ripgrep;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub path: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub hidden: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchMatch {
    pub path: String,
    pub line_number: u64,
    pub line_text: String,
    pub submatches: Vec<SearchSubmatch>,
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

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SearchStreamEvent {
    Started {
        search_id: u64,
    },
    Batch {
        search_id: u64,
        results: Vec<SearchMatch>,
    },
    Error {
        search_id: u64,
        message: String,
    },
    Finished {
        search_id: u64,
        total_matches: usize,
    },
    Cancelled {
        search_id: u64,
        total_matches: usize,
    },
}

#[async_trait::async_trait]
pub trait SearchProvider: Send + Sync {
    async fn search(&self, request: SearchRequest) -> anyhow::Result<Vec<SearchMatch>>;
}
