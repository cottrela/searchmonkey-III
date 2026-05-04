#!/usr/bin/env bash
set -e

ROOT="searchmonkey-demo-data"

rm -rf "$ROOT"
mkdir -p "$ROOT"/{projects/src,archive/{invoices,taxes,income,receipts},docs,logs}

# --- Rust source ---
cat > "$ROOT/projects/src/engine.rs" <<EOF
// TODO: stream partial matches while scanning large folders
// FIXME: optimise regex compilation cache
// BUG: crash when path contains unicode edge cases

fn search_line(line: &str, regex: &Regex) -> bool {
    if regex.is_match(line) {
        // TODO: push structured result instead of string
        return true;
    }
    false
}
EOF

# --- Svelte UI ---
cat > "$ROOT/projects/src/results.svelte" <<EOF
<!-- FIXME: preserve scroll position when results update -->
<!-- TODO: virtualise large result sets -->

<ResultLine match={match} grouped />
EOF

# --- Invoice text ---
cat > "$ROOT/archive/invoices/2025-client-summary.txt" <<EOF
Invoice total for March: £1,240.00
Quarterly invoice reconciliation completed.
Invoice reference: INV-2025-031
EOF

# --- Tax notes ---
cat > "$ROOT/archive/taxes/2024-deductions-notes.md" <<EOF
## 2024 Deduction Notes

Tax category: professional services
Invoice reference: INV-2024-041
Income offset reviewed for Q4

# TODO:
- confirm deductible subscription
EOF

# --- Income CSV ---
cat > "$ROOT/archive/income/freelance-income-report.csv" <<EOF
Date,Client,Amount,Note
2025-03-01,Acme Ltd,1240,Invoice payment received
2025-03-14,Globex,980,Consulting income
EOF

# --- Receipts ---
cat > "$ROOT/archive/receipts/software-subscriptions.md" <<EOF
Software subscriptions:
- Accounting package (tax deductible)
- Hosting provider (monthly invoice)
EOF

# --- Docs ---
cat > "$ROOT/docs/release-notes.md" <<EOF
# Release Notes

BUG: stale index results removed by direct file search
TODO: add multi-root search support
EOF

# --- Logs ---
cat > "$ROOT/logs/app.log" <<EOF
[INFO] Search started
[WARN] Large directory detected
[ERROR] BUG: failed to read file metadata
EOF

# Zip it
zip -r "${ROOT}.zip" "$ROOT" >/dev/null

echo "Created ${ROOT}.zip"