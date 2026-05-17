use super::{debug_logging_enabled, SearchMatch, SearchProvider, SearchRequest, SearchSubmatch};
use crate::plugins::{
    index_paths::{default_index_roots, mirror_search_path},
    registry::PluginRegistry,
    result_mapper,
    search_filter::SearchFilter,
};
use anyhow::Result;
use async_trait::async_trait;
use globset::{Glob, GlobMatcher};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub struct RipgrepSidecarProvider {
    _app_handle: tauri::AppHandle,
    plugin_registry: Arc<PluginRegistry>,
}

#[derive(Clone, Default)]
pub struct ResultPathFilter {
    search_root: PathBuf,
    include_patterns: Vec<CompiledGlobPattern>,
    exclude_patterns: Vec<CompiledGlobPattern>,
}

#[derive(Clone)]
struct CompiledGlobPattern {
    matcher: GlobMatcher,
    basename_only: bool,
}

impl RipgrepSidecarProvider {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        let discovery = PluginRegistry::discover_default().unwrap_or_default();
        if debug_logging_enabled() {
            for issue in &discovery.issues {
                eprintln!(
                    "searchmonkey plugin discovery issue: {}: {}",
                    issue.manifest_path.display(),
                    issue.message
                );
            }
        }
        Self {
            _app_handle: app_handle,
            plugin_registry: Arc::new(discovery.registry),
        }
    }

    pub fn plugin_registry(&self) -> Arc<PluginRegistry> {
        self.plugin_registry.clone()
    }

    pub fn args(request: SearchRequest, plugin_registry: &PluginRegistry) -> Vec<String> {
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
        let path = expand_search_path(&path);

        let plugin_filter = SearchFilter::for_search_root(Path::new(&path), plugin_registry);
        let _ = include_patterns;

        for pattern in exclude_patterns {
            args.push("--glob".to_string());
            args.push(format!("!{}", normalize_glob_pattern(pattern)));
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

        plugin_filter.apply_to_args(&mut args);
        args.push(query);
        args.push(path.clone());
        for index_root in default_index_roots() {
            let mirror_path = mirror_search_path(&index_root, Path::new(&path));
            if mirror_path.exists() {
                args.push(mirror_path.to_string_lossy().to_string());
            }
        }

        args
    }

    pub fn spawn(&self, request: SearchRequest) -> Result<Child> {
        let program = sidecar_path("rg")?;
        let args = Self::args(request, &self.plugin_registry);

        if debug_logging_enabled() {
            eprintln!(
                "searchmonkey rg command: {}",
                debug_command_line(&program, &args)
            );
        }

        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);

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
        let first_byte_offset = data["submatches"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item["start"].as_u64());
        let absolute_offset = data["absolute_offset"]
            .as_u64()
            .map(|offset| offset + first_byte_offset.unwrap_or(0));

        Some(SearchMatch {
            path: data["path"]["text"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            preview_path: None,
            display_context: None,
            plugin_id: None,
            meta_outdated: None,
            line_number: data["line_number"].as_u64().unwrap_or(0),
            line_text,
            submatches,
            absolute_offset,
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
        Some(after) => result
            .modified_secs
            .is_some_and(|modified| modified >= after),
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
    let mut ranges = items
        .iter()
        .filter_map(|item| {
            let start = item["start"].as_u64()? as usize;
            let end = item["end"].as_u64()? as usize;

            if start >= end || end > line_text.len() {
                return None;
            }

            Some((start, end))
        })
        .collect::<Vec<_>>();

    ranges.sort_by_key(|range| (range.0, range.1));

    if line_text.is_ascii() {
        return ranges
            .into_iter()
            .map(|(start, end)| SearchSubmatch { start, end })
            .collect();
    }

    let offsets = byte_to_utf16_offsets(
        line_text,
        ranges.iter().flat_map(|(start, end)| [*start, *end]),
    );
    let mut submatches = ranges
        .into_iter()
        .filter_map(|(start, end)| {
            Some(SearchSubmatch {
                start: *offsets.get(&start)?,
                end: *offsets.get(&end)?,
            })
        })
        .collect::<Vec<_>>();

    submatches.sort_by_key(|submatch| (submatch.start, submatch.end));
    submatches
}

fn byte_to_utf16_offsets<I>(text: &str, byte_offsets: I) -> std::collections::HashMap<usize, usize>
where
    I: IntoIterator<Item = usize>,
{
    let mut requested = byte_offsets.into_iter().collect::<Vec<_>>();
    requested.sort_unstable();
    requested.dedup();

    let mut resolved = std::collections::HashMap::with_capacity(requested.len());
    let mut requested_index = 0usize;
    let mut utf16_offset = 0usize;

    for (byte_index, character) in text.char_indices() {
        while requested_index < requested.len() && requested[requested_index] <= byte_index {
            resolved.insert(requested[requested_index], utf16_offset);
            requested_index += 1;
        }

        utf16_offset += character.len_utf16();
    }

    while requested_index < requested.len() {
        resolved.insert(requested[requested_index], utf16_offset);
        requested_index += 1;
    }

    resolved
}

#[async_trait]
impl SearchProvider for RipgrepSidecarProvider {
    async fn search(&self, request: SearchRequest) -> Result<Vec<SearchMatch>> {
        let modified_after = request.modified_after;
        let result_path_filter = ResultPathFilter::from_request(&request);
        let mut child = self.spawn(request)?;
        let mut matches = Vec::new();

        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);

            for line in reader.split(b'\n') {
                let line = line?;
                if let Some(result) = Self::parse_match(&line) {
                    let Some(mut result) =
                        result_mapper::map_search_match(result, &self.plugin_registry)
                    else {
                        continue;
                    };
                    if !result_path_filter.matches_path(Path::new(&result.path)) {
                        continue;
                    }
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

fn normalize_glob_pattern(pattern: String) -> String {
    pattern.replace('\\', "/")
}

impl ResultPathFilter {
    pub fn from_request(request: &SearchRequest) -> Self {
        let expanded_path = expand_search_path(&request.path);
        let search_root = filter_root(Path::new(&expanded_path)).to_path_buf();

        Self {
            search_root,
            include_patterns: compile_glob_patterns(&request.include_patterns),
            exclude_patterns: compile_glob_patterns(&request.exclude_patterns),
        }
    }

    pub fn matches_path(&self, path: &Path) -> bool {
        let include_matches = self.include_patterns.is_empty()
            || self
                .include_patterns
                .iter()
                .any(|pattern| pattern.matches(path, &self.search_root));
        if !include_matches {
            return false;
        }

        !self
            .exclude_patterns
            .iter()
            .any(|pattern| pattern.matches(path, &self.search_root))
    }
}

impl CompiledGlobPattern {
    fn matches(&self, path: &Path, search_root: &Path) -> bool {
        let absolute = normalize_path_for_matching(path);
        let basename = path
            .file_name()
            .and_then(|value| value.to_str())
            .map(|value| value.replace('\\', "/"));
        let relative = path
            .strip_prefix(search_root)
            .ok()
            .map(normalize_path_for_matching);

        if self.basename_only {
            return basename
                .as_deref()
                .is_some_and(|candidate| self.matcher.is_match(candidate));
        }

        relative
            .as_deref()
            .is_some_and(|candidate| self.matcher.is_match(candidate))
            || self.matcher.is_match(&absolute)
    }
}

fn compile_glob_patterns(patterns: &[String]) -> Vec<CompiledGlobPattern> {
    patterns
        .iter()
        .filter_map(|pattern| {
            let normalized = normalize_glob_pattern(pattern.clone());
            let matcher = Glob::new(&normalized).ok()?.compile_matcher();
            Some(CompiledGlobPattern {
                matcher,
                basename_only: !normalized.contains('/'),
            })
        })
        .collect()
}

fn filter_root(path: &Path) -> &Path {
    if path.is_file() {
        path.parent().unwrap_or(path)
    } else {
        path
    }
}

fn normalize_path_for_matching(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn expand_search_path(path: &str) -> String {
    if path == "~" {
        return home_dir().unwrap_or_else(|| path.to_string());
    }

    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir()
            .map(|home| Path::new(&home).join(rest).to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
    }

    path.to_string()
}

fn home_dir() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
}

#[cfg(test)]
mod tests {
    use super::{expand_search_path, ResultPathFilter};
    use crate::search::SearchRequest;
    use std::path::Path;

    #[test]
    fn expands_tilde_search_path() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        assert_eq!(expand_search_path("~"), home);
        assert!(expand_search_path("~/sm-test").ends_with("/sm-test"));
    }

    #[test]
    fn includes_match_source_extension_after_remap() {
        let filter = ResultPathFilter::from_request(&SearchRequest {
            query: "needle".to_string(),
            path: "/Users/acottrell/ocr-test".to_string(),
            regex: false,
            case_sensitive: false,
            hidden: false,
            include_patterns: vec!["*.jpg".to_string()],
            exclude_patterns: vec![],
            follow_symlinks: false,
            multiline: false,
            context_lines: 0,
            min_file_size: String::new(),
            max_file_size: String::new(),
            modified_after: None,
            skip_binary: false,
            encoding: "auto".to_string(),
            max_matches: None,
            respect_gitignore: true,
            ignore_node_modules: false,
            ignore_build_artifacts: false,
        });

        assert!(filter.matches_path(Path::new(
            "/Users/acottrell/ocr-test/invoices/page-1.jpg"
        )));
        assert!(!filter.matches_path(Path::new(
            "/Users/acottrell/ocr-test/invoices/page-1.png"
        )));
    }

    #[test]
    fn excludes_are_applied_to_source_paths() {
        let filter = ResultPathFilter::from_request(&SearchRequest {
            query: "needle".to_string(),
            path: "/Users/acottrell/ocr-test".to_string(),
            regex: false,
            case_sensitive: false,
            hidden: false,
            include_patterns: vec![],
            exclude_patterns: vec!["*.jpg".to_string()],
            follow_symlinks: false,
            multiline: false,
            context_lines: 0,
            min_file_size: String::new(),
            max_file_size: String::new(),
            modified_after: None,
            skip_binary: false,
            encoding: "auto".to_string(),
            max_matches: None,
            respect_gitignore: true,
            ignore_node_modules: false,
            ignore_build_artifacts: false,
        });

        assert!(!filter.matches_path(Path::new(
            "/Users/acottrell/ocr-test/invoices/page-1.jpg"
        )));
        assert!(filter.matches_path(Path::new(
            "/Users/acottrell/ocr-test/invoices/page-1.png"
        )));
    }
}
