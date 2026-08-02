use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CliLaunchRequest {
    pub query: Option<String>,
    pub path: Option<String>,
    pub regex: bool,
    pub case_sensitive: bool,
    pub globs: Vec<String>,
    pub hidden: bool,
    pub follow_symlinks: bool,
    pub context_lines: Option<u64>,
    pub no_ignore: bool,
    pub start: bool,
}

pub enum CliAction {
    Launch(Option<CliLaunchRequest>),
    Print(String),
}

const HELP: &str = "Searchmonkey III\n\nUsage: searchmonkey [OPTIONS] [PATTERN]\n\nArguments:\n  [PATTERN]                    Search expression (a regular expression by default)\n\nOptions:\n  -p, --path <PATH>            File or directory to search\n  -F, --fixed-strings          Treat PATTERN as literal text\n  -s, --case-sensitive         Enable case-sensitive matching\n  -g, --glob <GLOB>            Include or exclude a glob; repeatable\n  -H, --hidden                 Search hidden files and directories\n  -L, --follow                 Follow symbolic links\n  -C, --context <LINES>        Show surrounding context (maximum 20)\n      --no-ignore              Do not respect ignore files\n      --no-start               Populate the form without starting the search\n  -h, --help                   Print help\n  -V, --version                Print version";

pub fn parse<I, S>(args: I, cwd: &Path) -> Result<CliAction, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter().map(Into::into).peekable();
    let _executable = args.next();
    if args.peek().is_none() {
        return Ok(CliAction::Launch(None));
    }

    let mut request = CliLaunchRequest {
        query: None,
        path: None,
        regex: true,
        case_sensitive: false,
        globs: Vec::new(),
        hidden: false,
        follow_symlinks: false,
        context_lines: None,
        no_ignore: false,
        start: true,
    };
    let mut no_start = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(CliAction::Print(HELP.to_string())),
            "-V" | "--version" => {
                return Ok(CliAction::Print(format!(
                    "searchmonkey {}",
                    env!("CARGO_PKG_VERSION")
                )))
            }
            "-F" | "--fixed-strings" => request.regex = false,
            "-s" | "--case-sensitive" => request.case_sensitive = true,
            "-H" | "--hidden" => request.hidden = true,
            "-L" | "--follow" => request.follow_symlinks = true,
            "--no-ignore" => request.no_ignore = true,
            "--no-start" => no_start = true,
            _ if arg.starts_with("-psn_") => {}
            "-p" | "--path" => {
                request.path = Some(resolve_path(&required_value(&mut args, &arg)?, cwd));
            }
            "-g" | "--glob" => request.globs.push(required_value(&mut args, &arg)?),
            "-C" | "--context" => {
                let value = required_value(&mut args, &arg)?;
                let lines = value
                    .parse::<u64>()
                    .map_err(|_| format!("invalid context line count: {value}"))?;
                if lines > 20 {
                    return Err("context line count cannot exceed 20".to_string());
                }
                request.context_lines = Some(lines);
            }
            "--" => {
                for positional in args.by_ref() {
                    set_pattern(&mut request, positional)?;
                }
            }
            _ if arg.starts_with("--path=") => {
                request.path = Some(resolve_path(&arg[7..], cwd));
            }
            _ if arg.starts_with("--glob=") => request.globs.push(arg[7..].to_string()),
            _ if arg.starts_with("--context=") => {
                let value = &arg[10..];
                let lines = value
                    .parse::<u64>()
                    .map_err(|_| format!("invalid context line count: {value}"))?;
                if lines > 20 {
                    return Err("context line count cannot exceed 20".to_string());
                }
                request.context_lines = Some(lines);
            }
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            _ => set_pattern(&mut request, arg)?,
        }
    }

    request.start = request.query.is_some() && !no_start;
    Ok(CliAction::Launch(Some(request)))
}

fn required_value<I>(args: &mut std::iter::Peekable<I>, option: &str) -> Result<String, String>
where
    I: Iterator<Item = String>,
{
    args.next()
        .ok_or_else(|| format!("option {option} requires a value"))
}

fn set_pattern(request: &mut CliLaunchRequest, pattern: String) -> Result<(), String> {
    if request.query.is_some() {
        return Err(format!("unexpected extra argument: {pattern}"));
    }
    request.query = Some(pattern);
    Ok(())
}

fn resolve_path(value: &str, cwd: &Path) -> String {
    let path = PathBuf::from(value);
    if path.is_absolute() || value == "~" || value.starts_with("~/") || value.starts_with("~\\") {
        value.to_string()
    } else {
        cwd.join(path).to_string_lossy().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch(args: &[&str]) -> CliLaunchRequest {
        match parse(args.iter().copied(), Path::new("/work")).unwrap() {
            CliAction::Launch(Some(request)) => request,
            _ => panic!("expected launch request"),
        }
    }

    #[test]
    fn path_only_populates_without_starting() {
        let request = launch(&["searchmonkey", "--path", "docs"]);
        assert_eq!(request.path.as_deref(), Some("/work/docs"));
        assert_eq!(request.query, None);
        assert!(!request.start);
    }

    #[test]
    fn parses_rg_style_search_options() {
        let request = launch(&[
            "searchmonkey",
            "TODO|FIXME",
            "-p",
            "/src",
            "-F",
            "-s",
            "-g",
            "*.rs",
            "-g",
            "!target/**",
            "-H",
            "-L",
            "-C",
            "3",
            "--no-ignore",
        ]);
        assert_eq!(request.query.as_deref(), Some("TODO|FIXME"));
        assert_eq!(request.path.as_deref(), Some("/src"));
        assert!(!request.regex);
        assert!(request.case_sensitive);
        assert_eq!(request.globs, ["*.rs", "!target/**"]);
        assert_eq!(request.context_lines, Some(3));
        assert!(request.start);
    }

    #[test]
    fn no_start_overrides_pattern_auto_start() {
        assert!(!launch(&["searchmonkey", "needle", "--no-start"]).start);
    }

    #[test]
    fn rejects_unknown_options_and_extra_patterns() {
        assert!(parse(["searchmonkey", "--wat"], Path::new("/work")).is_err());
        assert!(parse(["searchmonkey", "one", "two"], Path::new("/work")).is_err());
    }
}
