#!/bin/sh
set -eu

VERSION="15.1.0"
BASE="https://github.com/BurntSushi/ripgrep/releases/download/${VERSION}"
OUT="src-tauri/binaries"
TMP="/tmp/searchmonkey-rg"

rm -rf "$TMP"
mkdir -p "$OUT" "$TMP"

pull_tar() {
  target="$1"
  output_target="${2:-$target}"
  archive="$TMP/ripgrep-${VERSION}-${target}.tar.gz"
  output="$OUT/rg-${output_target}"

  curl -fL "$BASE/ripgrep-${VERSION}-${target}.tar.gz" -o "$archive"
  tar -xzf "$archive" -C "$TMP"

  rm -f "$output"
  cp "$TMP/ripgrep-${VERSION}-${target}/rg" "$output"
  chmod 755 "$output"
}

pull_zip() {
  target="$1"
  archive="$TMP/ripgrep-${VERSION}-${target}.zip"
  output="$OUT/rg-${target}.exe"

  curl -fL "$BASE/ripgrep-${VERSION}-${target}.zip" -o "$archive"
  unzip -q -o "$archive" -d "$TMP"

  rm -f "$output"
  cp "$TMP/ripgrep-${VERSION}-${target}/rg.exe" "$output"
  chmod 755 "$output"
}

pull_tar "aarch64-apple-darwin"
pull_tar "x86_64-apple-darwin"
pull_tar "x86_64-unknown-linux-musl" "x86_64-unknown-linux-gnu"
pull_zip "x86_64-pc-windows-msvc"

echo "Downloaded ripgrep ${VERSION} sidecars to ${OUT}"
ls -l "$OUT"/rg-*
