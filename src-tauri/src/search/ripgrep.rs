use super::{SearchMatch, SearchProvider, SearchRequest, SearchSubmatch};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

pub struct RipgrepSidecarProvider {
    _app_handle: tauri::AppHandle,
}

impl RipgrepSidecarProvider {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self {
            _app_handle: app_handle,
        }
    }

    pub fn args(request: SearchRequest) -> Vec<String> {
        let mut args = vec![
            "--json".to_string(),
            "--line-number".to_string(),
            "--max-filesize".to_string(),
            "1M".to_string(),
            "--max-count".to_string(),
            "100000".to_string(),
            "--no-messages".to_string(),
        ];

        for pattern in request.include_patterns {
            args.push("--glob".to_string());
            args.push(pattern);
        }

        for pattern in request.exclude_patterns {
            args.push("--glob".to_string());
            args.push(format!("!{pattern}"));
        }

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

    pub fn spawn(&self, request: SearchRequest) -> Result<Child> {
        let mut command = Command::new(sidecar_path("rg")?);
        command
            .args(Self::args(request))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;

            unsafe {
                command.pre_exec(|| {
                    if libc::setpgid(0, 0) == 0 {
                        Ok(())
                    } else {
                        Err(std::io::Error::last_os_error())
                    }
                });
            }
        }

        Ok(command.spawn()?)
    }

    pub fn parse_match(line: &[u8]) -> Option<SearchMatch> {
        let json: Value = serde_json::from_slice(line).ok()?;

        if json["type"] != "match" {
            return None;
        }

        let data = &json["data"];

        let line_text = data["lines"]["text"]
            .as_str()
            .unwrap_or_default()
            .trim_end()
            .to_string();
        let submatches = data["submatches"]
            .as_array()
            .map(|items| parse_submatches(items, &line_text))
            .unwrap_or_default();

        Some(SearchMatch {
            path: data["path"]["text"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            line_number: data["line_number"].as_u64().unwrap_or(0),
            line_text,
            submatches,
        })
    }
}

fn parse_submatches(items: &[Value], line_text: &str) -> Vec<SearchSubmatch> {
    let mut submatches = items
        .iter()
        .filter_map(|item| {
            let start = item["start"].as_u64()? as usize;
            let end = item["end"].as_u64()? as usize;

            if start >= end || end > line_text.len() {
                return None;
            }

            Some(SearchSubmatch {
                start: byte_to_utf16_offset(line_text, start),
                end: byte_to_utf16_offset(line_text, end),
            })
        })
        .collect::<Vec<_>>();

    submatches.sort_by_key(|submatch| (submatch.start, submatch.end));
    submatches
}

fn byte_to_utf16_offset(text: &str, byte_offset: usize) -> usize {
    text.char_indices()
        .take_while(|(index, _)| *index < byte_offset)
        .map(|(_, character)| character.len_utf16())
        .sum()
}

#[async_trait]
impl SearchProvider for RipgrepSidecarProvider {
    async fn search(&self, request: SearchRequest) -> Result<Vec<SearchMatch>> {
        let mut child = self.spawn(request)?;
        let mut matches = Vec::new();

        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);

            for line in reader.split(b'\n') {
                let line = line?;
                if let Some(result) = Self::parse_match(&line) {
                    matches.push(result);
                }
            }
        }

        let _ = child.wait();
        Ok(matches)
    }
}

pub fn sidecar_path(program: &str) -> Result<PathBuf> {
    let exe_path = std::env::current_exe()?;
    let exe_dir = exe_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Current executable has no parent directory"))?;
    let base_dir = if exe_dir.ends_with("deps") {
        exe_dir.parent().unwrap_or(exe_dir)
    } else {
        exe_dir
    };

    let mut command_path = base_dir.join(Path::new(program));

    #[cfg(windows)]
    {
        if command_path.extension().is_none() {
            command_path.as_mut_os_string().push(".exe");
        }
    }

    #[cfg(not(windows))]
    {
        if command_path.extension().is_some_and(|ext| ext == "exe") {
            command_path.set_extension("");
        }
    }

    Ok(command_path)
}
