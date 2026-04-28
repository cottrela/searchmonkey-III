use super::{SearchMatch, SearchProvider, SearchRequest};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

pub struct RipgrepSidecarProvider {
    app_handle: tauri::AppHandle,
}

impl RipgrepSidecarProvider {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self { app_handle }
    }

    fn args(request: SearchRequest) -> Vec<String> {
        let mut args = vec!["--json".to_string(), "--line-number".to_string()];

        if !request.regex {
            args.push("--fixed-strings".to_string());
        }

        if !request.case_sensitive {
            args.push("--ignore-case".to_string());
        }

        if request.hidden {
            args.push("--hidden".to_string());
        }

        args.push(request.query);
        args.push(request.path);

        args
    }

    pub fn spawn(
        &self,
        request: SearchRequest,
    ) -> Result<(tauri::async_runtime::Receiver<CommandEvent>, CommandChild)> {
        Ok(self
            .app_handle
            .shell()
            .sidecar("rg")?
            .args(Self::args(request))
            .spawn()?)
    }

    pub fn parse_match(line: &[u8]) -> Option<SearchMatch> {
        let json: Value = serde_json::from_slice(line).ok()?;

        if json["type"] != "match" {
            return None;
        }

        let data = &json["data"];

        Some(SearchMatch {
            path: data["path"]["text"].as_str().unwrap_or_default().to_string(),
            line_number: data["line_number"].as_u64().unwrap_or(0),
            line_text: data["lines"]["text"]
                .as_str()
                .unwrap_or_default()
                .trim_end()
                .to_string(),
        })
    }
}

#[async_trait]
impl SearchProvider for RipgrepSidecarProvider {
    async fn search(&self, request: SearchRequest) -> Result<Vec<SearchMatch>> {
        let (mut rx, _child) = self.spawn(request)?;
        let mut matches = Vec::new();

        while let Some(event) = rx.recv().await {
            if let CommandEvent::Stdout(line) = event {
                if let Some(result) = Self::parse_match(&line) {
                    matches.push(result);
                }
            }
        }

        Ok(matches)
    }
}
