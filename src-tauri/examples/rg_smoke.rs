use searchmonkey_lib::plugins::registry::PluginRegistry;
use searchmonkey_lib::search::runner::{run_rg_child, SearchRunOptions};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const DEFAULT_PATH: &str = "/Users/acottrell/Documents/Pebl Legal Action";
const DEFAULT_QUERY: &str = "a";
const DEFAULT_TIMEOUT_MS: u64 = 5_000;

#[derive(Clone, Copy)]
struct Scenario {
    name: &'static str,
    stderr_null: bool,
    pre_exec_setpgid: bool,
    split_bytes: bool,
    parse_json: bool,
    metadata: bool,
    result_store: bool,
    status_store: bool,
    arc_child_wait: bool,
    async_session: bool,
    stop_at_result_limit: bool,
    shared_runner: bool,
}

struct Summary {
    status: String,
    lines: usize,
    matches: usize,
    stored: usize,
    metadata_reads: usize,
    stderr: String,
    samples: Vec<String>,
}

fn main() -> std::io::Result<()> {
    let config = Config::from_args();
    let scenarios = scenarios();

    if config.list {
        for scenario in &scenarios {
            println!("{}", scenario.name);
        }
        return Ok(());
    }

    let selected = if config.scenario == "all" {
        scenarios
    } else {
        scenarios
            .into_iter()
            .filter(|scenario| scenario.name == config.scenario)
            .collect()
    };

    if selected.is_empty() {
        eprintln!("unknown scenario: {}", config.scenario);
        eprintln!("use --list to see available scenarios");
        std::process::exit(2);
    }

    for scenario in selected {
        run_scenario(scenario, &config)?;
    }

    Ok(())
}

fn run_scenario(scenario: Scenario, config: &Config) -> std::io::Result<()> {
    let started = Instant::now();
    let args = rg_args(config);

    println!("\n== {} ==", scenario.name);
    println!("command: {}", debug_command_line(&config.rg_path, &args));

    let mut command = Command::new(&config.rg_path);
    command
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped());

    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    if scenario.stderr_null {
        command.stderr(Stdio::null());
    } else {
        command.stderr(Stdio::piped());
    }

    #[cfg(unix)]
    if scenario.pre_exec_setpgid {
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

    let mut child = command.spawn()?;
    let pid = child.id();
    let stdout = child.stdout.take().expect("rg stdout should be piped");
    let stderr = child.stderr.take();
    let timeout = Duration::from_millis(config.timeout_ms);
    let result_limit = config.max_matches;
    let (sender, receiver) = mpsc::channel();
    let status_probe = Arc::new(Mutex::new(0usize));
    let status_probe_for_worker = status_probe.clone();

    if scenario.shared_runner {
        thread::spawn(move || {
            let summary =
                read_with_shared_runner(stdout, child, status_probe_for_worker, result_limit);
            let _ = sender.send(summary);
        });

        match receiver.recv_timeout(timeout) {
            Ok(result) => print_summary(result, pid, started),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = kill_process_id(pid);
                println!(
                    "result: timeout pid={} elapsed={:.3}s killed=true",
                    pid,
                    started.elapsed().as_secs_f64()
                );
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                println!(
                    "result: error pid={} elapsed={:.3}s worker disconnected",
                    pid,
                    started.elapsed().as_secs_f64()
                );
            }
        }

        return Ok(());
    }

    let child = Arc::new(Mutex::new(child));
    let child_for_worker = child.clone();

    thread::spawn(move || {
        let summary = read_and_wait(
            scenario,
            stdout,
            stderr,
            child_for_worker,
            status_probe_for_worker,
            result_limit,
        );
        let _ = sender.send(summary);
    });

    if scenario.async_session {
        println!("start_search returned immediately; polling status store while worker owns rg");
        let poll_started = Instant::now();
        while poll_started.elapsed() < timeout {
            if let Ok(total) = status_probe.lock() {
                println!(
                    "poll: elapsed={:.3}s total_matches={}",
                    poll_started.elapsed().as_secs_f64(),
                    *total
                );
            }
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(result) => {
                    print_summary(result, pid, started);
                    return Ok(());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    println!(
                        "result: error pid={} elapsed={:.3}s worker disconnected",
                        pid,
                        started.elapsed().as_secs_f64()
                    );
                    return Ok(());
                }
            }
        }

        if let Ok(mut child) = child.lock() {
            let _ = kill_search_process(&mut child);
        }
        println!(
            "result: timeout pid={} elapsed={:.3}s killed=true",
            pid,
            started.elapsed().as_secs_f64()
        );
        return Ok(());
    }

    match receiver.recv_timeout(timeout) {
        Ok(result) => print_summary(result, pid, started),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            if let Ok(mut child) = child.lock() {
                let _ = kill_search_process(&mut child);
            }
            println!(
                "result: timeout pid={} elapsed={:.3}s killed=true",
                pid,
                started.elapsed().as_secs_f64()
            );
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            println!(
                "result: error pid={} elapsed={:.3}s worker disconnected",
                pid,
                started.elapsed().as_secs_f64()
            );
        }
    }

    Ok(())
}

fn print_summary(result: std::io::Result<Summary>, pid: u32, started: Instant) {
    match result {
        Ok(summary) => {
            println!(
                "result: ok status={} lines={} matches={} stored={} metadata_reads={} elapsed={:.3}s",
                summary.status,
                summary.lines,
                summary.matches,
                summary.stored,
                summary.metadata_reads,
                started.elapsed().as_secs_f64()
            );
            if !summary.stderr.trim().is_empty() {
                println!("stderr:\n{}", summary.stderr);
            }
            for (index, sample) in summary.samples.iter().enumerate() {
                println!("sample {}: {}", index + 1, sample);
            }
        }
        Err(err) => {
            println!(
                "result: error pid={} elapsed={:.3}s error={}",
                pid,
                started.elapsed().as_secs_f64(),
                err
            );
        }
    }
}

fn read_and_wait(
    scenario: Scenario,
    stdout: impl Read,
    stderr: Option<impl Read>,
    child: Arc<Mutex<Child>>,
    status_probe: Arc<Mutex<usize>>,
    result_limit: usize,
) -> std::io::Result<Summary> {
    let mut lines = 0usize;
    let mut matches = 0usize;
    let mut metadata_reads = 0usize;
    let mut samples = Vec::new();
    let result_store = Arc::new(Mutex::new(Vec::new()));
    let status_store = Arc::new(Mutex::new(0usize));
    let reader = BufReader::new(stdout);

    if scenario.split_bytes {
        for line in reader.split(b'\n') {
            let line = line?;
            lines += 1;
            let text = String::from_utf8_lossy(&line).to_string();
            if samples.len() < 5 {
                samples.push(text.clone());
            }

            if let Some(match_path) = parse_match_line(scenario, &line, &text) {
                matches += 1;
                if scenario.metadata {
                    let _ = std::fs::metadata(match_path);
                    metadata_reads += 1;
                }
                if scenario.result_store {
                    if let Ok(mut results) = result_store.lock() {
                        results.push(text);
                    }
                }
                if scenario.status_store {
                    if let Ok(mut total) = status_store.lock() {
                        *total = matches;
                    }
                    if let Ok(mut total) = status_probe.lock() {
                        *total = matches;
                    }
                }
                if scenario.stop_at_result_limit && matches >= result_limit {
                    break;
                }
            }
        }
    } else {
        for line in reader.lines() {
            let line = line?;
            lines += 1;
            if samples.len() < 5 {
                samples.push(line.clone());
            }

            if let Some(match_path) = parse_match_line(scenario, line.as_bytes(), &line) {
                matches += 1;
                if scenario.metadata {
                    let _ = std::fs::metadata(match_path);
                    metadata_reads += 1;
                }
                if scenario.result_store {
                    if let Ok(mut results) = result_store.lock() {
                        results.push(line);
                    }
                }
                if scenario.status_store {
                    if let Ok(mut total) = status_store.lock() {
                        *total = matches;
                    }
                    if let Ok(mut total) = status_probe.lock() {
                        *total = matches;
                    }
                }
                if scenario.stop_at_result_limit && matches >= result_limit {
                    break;
                }
            }
        }
    }

    let status = if scenario.arc_child_wait {
        child
            .lock()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "child lock poisoned"))?
            .wait()?
            .to_string()
    } else {
        child
            .lock()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "child lock poisoned"))?
            .wait()?
            .to_string()
    };

    let mut stderr_text = String::new();
    if let Some(mut stderr) = stderr {
        stderr.read_to_string(&mut stderr_text)?;
    }

    let stored = result_store
        .lock()
        .map(|results| results.len())
        .unwrap_or(0);

    Ok(Summary {
        status,
        lines,
        matches,
        stored,
        metadata_reads,
        stderr: stderr_text,
        samples,
    })
}

fn read_with_shared_runner(
    stdout: impl Read,
    child: Child,
    status_probe: Arc<Mutex<usize>>,
    result_limit: usize,
) -> std::io::Result<Summary> {
    let mut stored = 0usize;
    let summary = run_rg_child(
        child,
        stdout,
        SearchRunOptions {
            search_id: 9_999,
            result_limit,
            modified_after: None,
            plugin_registry: Arc::new(PluginRegistry::default()),
        },
        |_result, total_matches| {
            stored += 1;
            if let Ok(mut total) = status_probe.lock() {
                *total = total_matches;
            }
        },
    );

    Ok(Summary {
        status: summary.exit_status,
        lines: 0,
        matches: summary.total_matches,
        stored,
        metadata_reads: 0,
        stderr: String::new(),
        samples: Vec::new(),
    })
}

fn parse_match_line(scenario: Scenario, line: &[u8], text: &str) -> Option<String> {
    if !scenario.parse_json {
        return text.contains(r#""type":"match""#).then(|| String::new());
    }

    let Ok(json) = serde_json::from_slice::<Value>(line) else {
        return None;
    };

    if json["type"] != "match" {
        return None;
    }

    let data = &json["data"];
    let line_text = data["lines"]["text"]
        .as_str()
        .unwrap_or_default()
        .trim_end();
    let path = data["path"]["text"].as_str().unwrap_or_default();
    let _line_number = data["line_number"].as_u64().unwrap_or(0);
    let _submatches = data["submatches"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let start = item["start"].as_u64()? as usize;
                    let end = item["end"].as_u64()? as usize;
                    if start >= end || end > line_text.len() {
                        return None;
                    }
                    Some((start, end))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Some(path.to_string())
}

fn rg_args(config: &Config) -> Vec<String> {
    vec![
        "--json".to_string(),
        "--line-number".to_string(),
        "--no-messages".to_string(),
        "--max-filesize".to_string(),
        "10M".to_string(),
        "--fixed-strings".to_string(),
        "--ignore-case".to_string(),
        config.query.clone(),
        config.path.clone(),
    ]
}

fn scenarios() -> Vec<Scenario> {
    let mut scenario = Scenario {
        name: "baseline",
        stderr_null: false,
        pre_exec_setpgid: false,
        split_bytes: false,
        parse_json: false,
        metadata: false,
        result_store: false,
        status_store: false,
        arc_child_wait: false,
        async_session: false,
        stop_at_result_limit: false,
        shared_runner: false,
    };
    let mut scenarios = vec![scenario];

    scenario = Scenario {
        name: "stderr-null",
        stderr_null: true,
        ..scenario
    };
    scenarios.push(scenario);

    scenario = Scenario {
        name: "split-bytes",
        split_bytes: true,
        ..scenario
    };
    scenarios.push(scenario);

    scenario = Scenario {
        name: "parse-json",
        parse_json: true,
        ..scenario
    };
    scenarios.push(scenario);

    scenario = Scenario {
        name: "metadata",
        metadata: true,
        ..scenario
    };
    scenarios.push(scenario);

    scenario = Scenario {
        name: "result-store",
        result_store: true,
        ..scenario
    };
    scenarios.push(scenario);

    scenario = Scenario {
        name: "status-store",
        status_store: true,
        ..scenario
    };
    scenarios.push(scenario);

    scenario = Scenario {
        name: "arc-child-wait",
        arc_child_wait: true,
        ..scenario
    };
    scenarios.push(scenario);

    scenario = Scenario {
        name: "pre-exec-setpgid",
        pre_exec_setpgid: true,
        ..scenario
    };
    scenarios.push(scenario);

    scenario = Scenario {
        name: "async-session",
        async_session: true,
        ..scenario
    };
    scenarios.push(scenario);

    scenario = Scenario {
        name: "stop-at-result-limit",
        stop_at_result_limit: true,
        ..scenario
    };
    scenarios.push(scenario);

    scenarios.push(Scenario {
        name: "prod-like",
        stop_at_result_limit: false,
        ..scenario
    });

    scenarios.push(Scenario {
        name: "shared-runner",
        shared_runner: true,
        ..scenario
    });

    scenarios
}

fn debug_command_line(program: &str, args: &[String]) -> String {
    std::iter::once(shell_quote(program))
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

fn kill_search_process(child: &mut Child) -> std::io::Result<()> {
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

fn kill_process_id(pid: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let pid = pid as i32;
        unsafe {
            if libc::kill(-pid, libc::SIGTERM) == 0 {
                return Ok(());
            }
        }
        unsafe {
            if libc::kill(pid, libc::SIGTERM) == 0 {
                return Ok(());
            }
        }
        return Err(std::io::Error::last_os_error());
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "kill by pid is unsupported on this platform",
        ))
    }
}

struct Config {
    path: String,
    query: String,
    scenario: String,
    timeout_ms: u64,
    max_matches: usize,
    rg_path: String,
    list: bool,
}

impl Config {
    fn from_args() -> Self {
        let mut config = Self {
            path: DEFAULT_PATH.to_string(),
            query: DEFAULT_QUERY.to_string(),
            scenario: "all".to_string(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            max_matches: 100_000,
            rg_path: default_rg_path(),
            list: false,
        };
        let mut args = std::env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--list" => config.list = true,
                "--scenario" => {
                    config.scenario = args.next().unwrap_or_else(|| config.scenario.clone());
                }
                "--timeout-ms" => {
                    config.timeout_ms = args
                        .next()
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(config.timeout_ms);
                }
                "--rg" => {
                    config.rg_path = args.next().unwrap_or_else(|| config.rg_path.clone());
                }
                "--max-matches" => {
                    config.max_matches = args
                        .next()
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(config.max_matches);
                }
                "--path" => {
                    config.path = args.next().unwrap_or_else(|| config.path.clone());
                }
                "--query" => {
                    config.query = args.next().unwrap_or_else(|| config.query.clone());
                }
                value => {
                    if config.path == DEFAULT_PATH {
                        config.path = value.to_string();
                    } else if config.query == DEFAULT_QUERY {
                        config.query = value.to_string();
                    }
                }
            }
        }

        config
    }
}

fn default_rg_path() -> String {
    let target_debug = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/debug/rg");
    target_debug.to_string_lossy().to_string()
}
