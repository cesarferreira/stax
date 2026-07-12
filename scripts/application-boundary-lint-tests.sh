#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
wrapper="$script_dir/application-boundary-lint.sh"
temp_root="$(mktemp -d)"
fixture="$temp_root/fixture"
unrelated="$temp_root/unrelated"
trap 'rm -rf "$temp_root"' EXIT
mkdir -p "$fixture/src/application" "$unrelated"
git -C "$fixture" init -q

failures=0

record_failure() {
  echo "$1" >&2
  failures=$((failures + 1))
}

run_lint() {
  (
    cd "$unrelated"
    bash "$wrapper" "$fixture"
  )
}

assert_rejected() {
  local source="$1"
  local expected="$2"
  printf '%s\n' "$source" > "$fixture/src/application/checkout.rs"
  if run_lint >"$temp_root/output" 2>&1; then
    record_failure "expected boundary lint rejection for: $source"
  elif ! rg -F "$expected" "$temp_root/output" >/dev/null; then
    record_failure "expected '$expected' for rejected source: $source"
  fi
}

assert_accepted() {
  local source="$1"
  printf '%s\n' "$source" > "$fixture/src/application/checkout.rs"
  if ! run_lint >"$temp_root/output" 2>&1; then
    record_failure "expected boundary lint acceptance for: $source"
  fi
}

printf '%s\n' 'pub fn clean() {}' > "$fixture/src/application/checkout.rs"
run_lint
git -C "$fixture" add src/application/checkout.rs

assert_rejected 'use crate::commands::submit;' 'command or TUI modules'
assert_rejected 'use crate::{commands::submit, git::GitRepo};' 'command or TUI modules'
assert_rejected 'use crate::commands as cli_commands;' 'command or TUI modules'
assert_rejected 'fn bad() { crate::commands::submit::run(); }' 'command or TUI modules'
assert_rejected 'fn bad() { crate::r#commands::submit::run(); }' 'command or TUI modules'
assert_rejected 'use crate::{tui, git};' 'command or TUI modules'
assert_rejected $'use crate::{\n    commands as cli_commands,\n    git,\n};' 'command or TUI modules'
assert_rejected 'fn bad() { crate :: tui :: run(); }' 'command or TUI modules'

assert_rejected 'use gpui::App;' 'presentation frameworks'
assert_rejected 'use {ratatui as terminal_ui, git2};' 'presentation frameworks'
assert_rejected 'fn bad() { ::crossterm::execute!(); }' 'presentation frameworks'
assert_rejected 'use dialoguer as prompt;' 'presentation frameworks'
assert_rejected 'use colored::Colorize;' 'presentation frameworks'
assert_rejected 'use console::Style;' 'presentation frameworks'
assert_rejected 'fn bad() { :: gpui :: App::new(); }' 'presentation frameworks'

assert_rejected 'use crate::progress::LiveTimer;' 'terminal progress'
assert_rejected 'use crate::{progress as terminal_progress, git};' 'terminal progress'
assert_rejected 'fn bad() { crate::progress::LiveTimer::new(); }' 'terminal progress'
assert_rejected 'fn bad() { crate :: progress :: LiveTimer::new(); }' 'terminal progress'

assert_rejected 'use std::io::{stdout, IsTerminal};' 'terminal I/O'
assert_rejected 'use std::{io::stdout};' 'terminal I/O'
assert_rejected 'use std :: io :: { stderr as terminal_stderr, IsTerminal };' 'terminal I/O'
assert_rejected 'use std::io::stdin;' 'terminal I/O'
assert_rejected 'use std::io::stderr as terminal_stderr;' 'terminal I/O'
assert_rejected 'fn bad() { std::io::stdout(); }' 'terminal I/O'

assert_rejected 'print!("hidden terminal output");' 'terminal output macros'
assert_rejected 'println!("hidden terminal output");' 'terminal output macros'
assert_rejected 'println ! ("hidden spaced terminal output");' 'terminal output macros'
assert_rejected 'use std::{println as terminal_print};' 'terminal output macros'
assert_rejected 'eprint!("hidden terminal error");' 'terminal output macros'
assert_rejected 'std::eprintln!("hidden terminal error");' 'terminal output macros'
assert_rejected ':: std :: eprintln ! ("hidden qualified terminal error");' 'terminal output macros'
assert_rejected 'dbg!("hidden terminal debug");' 'terminal output macros'

assert_accepted 'pub fn commandster_progress(stdout_buffer: usize) -> usize { stdout_buffer }'
assert_accepted '// use crate::commands::submit; println!("not code");'
assert_accepted '/* use gpui::App; /* std::io::stdout(); */ crate::progress::LiveTimer */ pub fn clean() {}'
assert_accepted 'const TEXT: &str = "crate::commands::submit println!(hidden)";'
assert_accepted 'const RAW: &str = r###"use gpui::App; std::io::stdout();"###;'
assert_accepted 'const BYTES: &[u8] = b"use crate::tui; eprintln!(hidden)";'
assert_accepted 'const RAW_BYTES: &[u8] = br##"use dialoguer::Select; dbg!(hidden)"##;'
assert_accepted "const CHARACTER: char = 'p'; const BYTE: u8 = b'p';"
assert_accepted "fn lifetime<'progress>(value: &'progress str) -> &'progress str { value }"

printf '%s\n' 'pub fn clean() {}' > "$fixture/src/application/checkout.rs"
mkdir -p "$fixture/src/application/nested/future"
printf '%s\n' 'use dialoguer as prompt;' > "$fixture/src/application/nested/future/module.rs"
if run_lint >"$temp_root/output" 2>&1; then
  record_failure "expected recursive boundary lint rejection"
elif ! rg -F 'presentation frameworks' "$temp_root/output" >/dev/null; then
  record_failure "expected presentation framework label for nested module"
fi
rm -rf "$fixture/src/application/nested"

printf '\377' > "$fixture/src/application/checkout.rs"
if run_lint >"$temp_root/output" 2>&1; then
  record_failure "expected invalid UTF-8 scanner failure"
fi

non_git="$temp_root/non-git"
mkdir -p "$non_git/src/application"
printf '%s\n' 'pub fn clean() {}' > "$non_git/src/application/clean.rs"
if (
  cd "$unrelated"
  bash "$wrapper" "$non_git"
) >"$temp_root/output" 2>&1; then
  record_failure "expected git discovery failure"
fi

if ! rg -F 'bash scripts/application-boundary-lint.sh' "$repo_root/scripts/lint.sh" >/dev/null; then
  record_failure "expected scripts/lint.sh to run the application boundary lint"
fi

if ((failures > 0)); then
  echo "application boundary lint tests failed: $failures" >&2
  exit 1
fi

printf '%s\n' "application boundary lint tests passed"
