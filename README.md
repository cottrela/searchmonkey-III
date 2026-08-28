# Searchmonkey III

**Real-time search for real files.**
No index. No daemon. No stale results.

Searchmonkey III is a modern desktop search tool that searches what is actually on disk — right now.
It does not maintain a background index, and it does not return outdated results.

The Searchmonkey III project is available at https://searchmonkey.dev.

---

## Why

Most desktop search tools trade accuracy for speed by indexing files in the background.

Searchmonkey takes a different approach:

* **Search the filesystem directly**
* **Stream results as they are found**
* **Always reflect the current state of disk**

This makes it particularly useful for:

* developers working with changing codebases
* log inspection
* large directories where indexing is expensive or unreliable
* environments where background daemons are undesirable

---

## Getting started

### Prerequisites

* Node.js (18+ recommended)
* pnpm
* Rust toolchain (for Tauri)

### Install

```sh
pnpm install
```

Download the ripgrep sidecar used by the Tauri app:

```sh
scripts/pull-rg-bin.sh
```

On Windows:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\pull-rg-bin.ps1
```

### Run (development)

```sh
pnpm tauri dev
```

### Build

```sh
pnpm tauri build
```

## Command line

Searchmonkey accepts a small, ripgrep-inspired command line that opens the desktop interface. A pattern starts the search immediately; passing only a path populates the path field without searching.

```sh
# Open with a directory selected (useful for file-manager actions)
searchmonkey --path /my/path

# Search using a regular expression
searchmonkey 'TODO|FIXME' --path ~/projects

# Search for literal text in log files
searchmonkey --fixed-strings 'connection failed' --path /var/log --glob '*.log'
```

Available options:

```text
Usage: searchmonkey [OPTIONS] [PATTERN]

  -p, --path <PATH>            File or directory to search
  -F, --fixed-strings          Treat PATTERN as literal text
  -s, --case-sensitive         Enable case-sensitive matching
  -g, --glob <GLOB>            Include or exclude a glob; repeatable
  -H, --hidden                 Search hidden files and directories
  -L, --follow                 Follow symbolic links
  -C, --context <LINES>        Show surrounding context (maximum 20)
      --no-ignore              Do not respect ignore files
      --no-start               Populate the form without starting the search
  -h, --help                   Print help
  -V, --version                Print version
```

Relative `--path` values are resolved from the invoking process's working directory. With no path option, Searchmonkey retains its normal home-directory default. If the app is already running, another invocation focuses the existing window and applies the new search.

---

## How it works

Searchmonkey scans files directly and streams matches as they are discovered.

It is designed to be:

* **stateless** — no index database
* **transparent** — what is searched is what exists
* **predictable** — no background processes affecting results

---

## Plugins

Searchmonkey can be extended via *sidecar artifacts*.

For example:

```text
document.pdf
document.pdf.sm.txt
document.pdf.sm.meta
```

Plugins can generate these files to make otherwise opaque formats searchable.

This enables:

* PDF → text extraction
* DOCX → structured text
* logs → normalized formats

The core application remains simple, while plugins provide additional capabilities.

---

## Project structure

```text
src/            SvelteKit frontend
src-tauri/      Rust (Tauri) backend
```

---

## Roadmap (high level)

* improved search performance and filtering
* richer match context and navigation
* plugin system for file enrichment
* optional account-backed features (sync, etc.)

---

## License

Licensed under the MIT License — see [LICENSE](./LICENSE)

---

## 👤 Author

Searchmonkey is developed by Axonara Ltd
https://axonara.co.uk
