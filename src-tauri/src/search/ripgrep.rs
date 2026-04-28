use super::{SearchMatch, SearchProvider, SearchRequest};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use tauri_plugin_shell::ShellExt;

pub struct RipgrepSidecarProvider {
    app_handle: tauri::AppHandle,
}

impl RipgrepSidecarProvider {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self { app_handle }
    }
}

#[async_trait]
impl SearchProvider for RipgrepSidecarProvider {
    async fn search(&self, request: SearchRequest) -> Result<Vec<SearchMatch>> {
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

        let output = self
            .app_handle
            .shell()
            .sidecar("rg")?
            .args(args)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        let mut matches = Vec::new();

        for line in stdout.lines() {
            let json: Value = match serde_json::from_str(line) {
                Ok(value) => value,
                Err(_) => continue,
            };

            if json["type"] != "match" {
                continue;
            }

            let data = &json["data"];

            matches.push(SearchMatch {
                path: data["path"]["text"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                line_number: data["line_number"].as_u64().unwrap_or(0),
                line_text: data["lines"]["text"]
                    .as_str()
                    .unwrap_or_default()
                    .trim_end()
                    .to_string(),
            });
        }

        Ok(matches)
    }
}
