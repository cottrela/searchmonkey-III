use super::{debug_logging_enabled, ripgrep, SearchMatch, SearchState};
use crate::plugins::{registry::PluginRegistry, result_mapper};
use std::io::{BufRead, BufReader, Read};
use std::process::Child;
use std::sync::Arc;
use std::time::Instant;

pub struct SearchRunOptions {
    pub search_id: u64,
    pub result_limit: usize,
    pub modified_after: Option<u64>,
    pub plugin_registry: Arc<PluginRegistry>,
    pub result_path_filter: ripgrep::ResultPathFilter,
}

pub struct SearchRunSummary {
    pub raw_stdout_lines: usize,
    pub raw_match_lines: usize,
    pub remapped_or_plain_matches: usize,
    pub skipped_result_path_filter: usize,
    pub total_matches: usize,
    pub buffered_matches: usize,
    pub limit_reached: bool,
    pub skipped_modified: usize,
    pub read_errors: usize,
    pub exit_status: String,
    pub final_state: SearchState,
    pub error_message: Option<String>,
    pub elapsed_secs: f64,
}

pub fn run_rg_child<R, F>(
    mut child: Child,
    stdout: R,
    options: SearchRunOptions,
    mut on_match: F,
) -> SearchRunSummary
where
    R: Read,
    F: FnMut(SearchMatch, usize),
{
    let search_id = options.search_id;
    let filter_summary = if debug_logging_enabled() {
        Some(options.result_path_filter.debug_summary())
    } else {
        None
    };
    let mut raw_stdout_lines = 0usize;
    let mut raw_match_lines = 0usize;
    let mut remapped_or_plain_matches = 0usize;
    let mut skipped_result_path_filter = 0usize;
    let mut total_matches = 0usize;
    let mut buffered_matches = 0usize;
    let mut limit_reached = false;
    let mut skipped_modified = 0usize;
    let mut read_errors = 0usize;
    let started_at = Instant::now();
    let reader = BufReader::new(stdout);
    let debug_logging = debug_logging_enabled();

    if debug_logging {
        eprintln!(
            "searchmonkey search {search_id}: rg runner started pid={} result_limit={} {}",
            child.id(),
            options.result_limit,
            filter_summary.unwrap_or_default()
        );
    }

    for line in reader.split(b'\n') {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                read_errors += 1;
                eprintln!("searchmonkey search {search_id}: failed to read rg stdout line: {err}");
                continue;
            }
        };
        raw_stdout_lines += 1;

        let Some(result) = ripgrep::RipgrepSidecarProvider::parse_match(&line) else {
            continue;
        };
        raw_match_lines += 1;

        let Some(mut result) = result_mapper::map_search_match(result, &options.plugin_registry)
        else {
            continue;
        };
        remapped_or_plain_matches += 1;
        if !options
            .result_path_filter
            .matches_path(std::path::Path::new(&result.path))
        {
            skipped_result_path_filter += 1;
            continue;
        }

        if options.modified_after.is_some() {
            ripgrep::add_file_metadata(&mut result);
            if !ripgrep::matches_modified_filter(&result, options.modified_after) {
                skipped_modified += 1;
                continue;
            }
        }

        total_matches += 1;
        if buffered_matches < options.result_limit {
            buffered_matches += 1;
            on_match(result, total_matches);
        }
        if buffered_matches >= options.result_limit {
            limit_reached = true;
            if debug_logging {
                eprintln!(
                    "searchmonkey search {search_id}: result limit {} reached; terminating rg pid={}",
                    options.result_limit,
                    child.id()
                );
            }
            if let Err(err) = terminate_child(&mut child) {
                eprintln!(
                    "searchmonkey search {search_id}: failed to terminate rg at limit: {err}"
                );
            }
            break;
        }

        if debug_logging && total_matches % 10_000 == 0 {
            eprintln!(
                "searchmonkey search {search_id}: parsed {total_matches} matches in {:.2}s",
                started_at.elapsed().as_secs_f64()
            );
        }
    }

    let exit_status = match child.wait() {
        Ok(status) => {
            if debug_logging {
                eprintln!("searchmonkey search {search_id}: rg exited: {status}");
            }
            status.to_string()
        }
        Err(err) => {
            let message = format!("failed waiting for rg: {err}");
            eprintln!("searchmonkey search {search_id}: {message}");
            message
        }
    };

    let final_state = if read_errors > 0 {
        SearchState::Failed
    } else {
        SearchState::Completed
    };
    let error_message = if final_state == SearchState::Failed {
        Some(format!(
            "Search failed while reading results. {exit_status}"
        ))
    } else {
        None
    };
    let elapsed_secs = started_at.elapsed().as_secs_f64();

    if debug_logging {
        eprintln!(
            "searchmonkey search {search_id}: rg runner finished raw_stdout_lines={raw_stdout_lines} raw_match_lines={raw_match_lines} remapped_or_plain_matches={remapped_or_plain_matches} skipped_result_path_filter={skipped_result_path_filter} total_matches={total_matches} buffered_matches={buffered_matches} limit_reached={limit_reached} skipped_modified={skipped_modified} read_errors={read_errors} elapsed={elapsed_secs:.2}s exit={exit_status}"
        );
    }

    SearchRunSummary {
        raw_stdout_lines,
        raw_match_lines,
        remapped_or_plain_matches,
        skipped_result_path_filter,
        total_matches,
        buffered_matches,
        limit_reached,
        skipped_modified,
        read_errors,
        exit_status,
        final_state,
        error_message,
        elapsed_secs,
    }
}

fn terminate_child(child: &mut Child) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        unsafe {
            if libc::kill(-pid, libc::SIGTERM) == 0 {
                return Ok(());
            }
        }
    }

    child.kill()
}
