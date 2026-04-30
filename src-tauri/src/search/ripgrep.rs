use super::{SearchMatch, SearchProvider, SearchRequest, SearchSubmatch};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::UNIX_EPOCH;

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
            "--no-messages".to_string(),
        ];

        if !request.min_file_size.trim().is_empty() {
            args.push("--min-filesize".to_string());
            args.push(request.min_file_size.trim().to_string());
        }

        if !request.max_file_size.trim().is_empty() {
            args.push("--max-filesize".to_string());
            args.push(request.max_file_size.trim().to_string());
        }

        if request.max_matches > Some(0) {
            args.push("--max-count".to_string());
            args.push(request.max_matches.unwrap().to_string());
        }

        if request.context_lines > 0 {
            args.push("--context".to_string());
            args.push(request.context_lines.min(20).to_string());
        }

        let SearchRequest {
            query,
            path,
            regex,
            case_sensitive,
            hidden,
            include_patterns,
            exclude_patterns,
            follow_symlinks,
            multiline,
            skip_binary,
            encoding,
            respect_gitignore,
            ignore_node_modules,
            ignore_build_artifacts,
            ..
        } = request;

        for pattern in include_patterns {
            args.push("--glob".to_string());
            args.push(pattern);
        }

        for pattern in exclude_patterns {
            args.push("--glob".to_string());
            args.push(format!("!{pattern}"));
        }

        if ignore_node_modules {
            args.push("--glob".to_string());
            args.push("!**/node_modules/**".to_string());
        }

        if ignore_build_artifacts {
            for pattern in [
                "!**/dist/**",
                "!**/build/**",
                "!**/target/**",
                "!**/.svelte-kit/**",
                "!**/.next/**",
                "!**/coverage/**",
            ] {
                args.push("--glob".to_string());
                args.push(pattern.to_string());
            }
        }

        if !regex {
            args.push("--fixed-strings".to_string());
        }

        if !case_sensitive {
            args.push("--ignore-case".to_string());
        }

        if hidden {
            args.push("--hidden".to_string());
        }

        if follow_symlinks {
            args.push("--follow".to_string());
        }

        if multiline {
            args.push("--multiline".to_string());
        }

        if !skip_binary {
            args.push("--text".to_string());
        }

        if encoding == "utf-8" || encoding == "ascii" {
            args.push("--encoding".to_string());
            args.push(encoding);
        }

        if !respect_gitignore {
            args.push("--no-ignore".to_string());
        }

        args.push(query);
        args.push(path);

        args
    }

    pub fn spawn(&self, request: SearchRequest) -> Result<Child> {
        let program = sidecar_path("rg")?;
        let args = Self::args(request);

        eprintln!("searchmonkey rg command: {}", debug_command_line(&program, &args));

        let mut command = Command::new(program);
        command
            .args(args)
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
            file_size: None,
            modified_secs: None,
        })
    }
}

pub fn add_file_metadata(result: &mut SearchMatch) {
    let Ok(metadata) = std::fs::metadata(&result.path) else {
        return;
    };

    result.file_size = Some(metadata.len());
    result.modified_secs = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs());
}

pub fn matches_modified_filter(result: &SearchMatch, modified_after: Option<u64>) -> bool {
    match modified_after {
        Some(after) => result.modified_secs.is_some_and(|modified| modified >= after),
        None => true,
    }
}

fn debug_command_line(program: &Path, args: &[String]) -> String {
    std::iter::once(shell_quote(&program.to_string_lossy()))
        .chain(args.iter().map(|arg| shell_quote(arg)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-_./:=,@%+".contains(character))
    {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
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
        let modified_after = request.modified_after;
        let mut child = self.spawn(request)?;
        let mut matches = Vec::new();

        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);

            for line in reader.split(b'\n') {
                let line = line?;
                if let Some(mut result) = Self::parse_match(&line) {
                    add_file_metadata(&mut result);
                    if !matches_modified_filter(&result, modified_after) {
                        continue;
                    }
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
