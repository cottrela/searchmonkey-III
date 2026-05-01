# Searchmonkey III

**Real-time search for real files.**
No index. No daemon. No stale results.

Searchmonkey III is a modern desktop search tool that searches what is actually on disk — right now.
It does not maintain a background index, and it does not return outdated results.

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

### Run (development)

```sh
pnpm tauri dev
```

### Build

```sh
pnpm tauri build
```

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

Searchmonkey is developed by Axanara Ltd
https://axanara.co.uk