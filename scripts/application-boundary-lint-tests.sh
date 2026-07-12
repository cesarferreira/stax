#!/usr/bin/env bash
set -euo pipefail

root="$(mktemp -d)"
trap 'rm -rf "$root"' EXIT
mkdir -p "$root/src/application"
git -C "$root" init -q

assert_rejected() {
  local source="$1"
  local expected="$2"
  printf '%s\n' "$source" > "$root/src/application/checkout.rs"
  if bash scripts/application-boundary-lint.sh "$root" >"$root/output" 2>&1; then
    echo "expected boundary lint rejection for: $source" >&2
    exit 1
  fi
  rg -F "$expected" "$root/output" >/dev/null
}

printf '%s\n' 'pub fn clean() {}' > "$root/src/application/checkout.rs"
bash scripts/application-boundary-lint.sh "$root"

assert_rejected 'use crate::commands::submit;' 'command or TUI modules'
assert_rejected 'use crate::{commands::submit, git::GitRepo};' 'command or TUI modules'
assert_rejected 'use crate::commands as cli_commands;' 'command or TUI modules'
assert_rejected 'fn bad() { crate::commands::submit::run(); }' 'command or TUI modules'
assert_rejected 'use crate::{tui, git};' 'command or TUI modules'

assert_rejected 'use gpui::App;' 'presentation frameworks'
assert_rejected 'use {ratatui as terminal_ui, git2};' 'presentation frameworks'
assert_rejected 'fn bad() { ::crossterm::execute!(); }' 'presentation frameworks'
assert_rejected 'use dialoguer as prompt;' 'presentation frameworks'
assert_rejected 'use colored::Colorize;' 'presentation frameworks'
assert_rejected 'use console::Style;' 'presentation frameworks'

assert_rejected 'use crate::progress::LiveTimer;' 'terminal progress'
assert_rejected 'use crate::{progress as terminal_progress, git};' 'terminal progress'
assert_rejected 'fn bad() { crate::progress::LiveTimer::new(); }' 'terminal progress'

assert_rejected 'use std::io::{stdout, IsTerminal};' 'terminal I/O'
assert_rejected 'use std::io::stdin;' 'terminal I/O'
assert_rejected 'use std::io::stderr as terminal_stderr;' 'terminal I/O'
assert_rejected 'fn bad() { std::io::stdout(); }' 'terminal I/O'

assert_rejected 'print!("hidden terminal output");' 'terminal output macros'
assert_rejected 'println!("hidden terminal output");' 'terminal output macros'
assert_rejected 'eprint!("hidden terminal error");' 'terminal output macros'
assert_rejected 'std::eprintln!("hidden terminal error");' 'terminal output macros'
assert_rejected 'dbg!("hidden terminal debug");' 'terminal output macros'

printf '%s\n' 'pub fn clean() {}' > "$root/src/application/checkout.rs"
mkdir -p "$root/src/application/nested/future"
printf '%s\n' 'use dialoguer as prompt;' > "$root/src/application/nested/future/module.rs"
if bash scripts/application-boundary-lint.sh "$root" >"$root/output" 2>&1; then
  echo "expected recursive boundary lint rejection" >&2
  exit 1
fi
rg -F 'presentation frameworks' "$root/output" >/dev/null

if ! rg -F 'bash scripts/application-boundary-lint.sh' scripts/lint.sh >/dev/null; then
  echo "expected scripts/lint.sh to run the application boundary lint" >&2
  exit 1
fi

printf '%s\n' "application boundary lint tests passed"
