pub mod ripgrep;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub path: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub hidden: bool,
}

#[derive(Debug, Serialize)]
pub struct SearchMatch {
    pub path: String,
    pub line_number: u64,
    pub line_text: String,
}

#[async_trait::async_trait]
pub trait SearchProvider: Send + Sync {
    async fn search(&self, request: SearchRequest) -> anyhow::Result<Vec<SearchMatch>>;
}
