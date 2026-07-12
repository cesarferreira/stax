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
  elif rg -F 'application boundary scanner error:' "$temp_root/output" >/dev/null; then
    record_failure "expected '$expected', not a scanner error, for: $source"
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

assert_scanner_error() {
  local source="$1"
  printf '%s\n' "$source" > "$fixture/src/application/checkout.rs"
  if run_lint >"$temp_root/output" 2>&1; then
    record_failure "expected scanner failure for malformed source: $source"
  elif ! rg -F 'application boundary scanner error:' "$temp_root/output" >/dev/null; then
    record_failure "expected scanner error label for malformed source: $source"
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
assert_rejected 'fn bad() { r#extern::commands::run(); }' 'command or TUI modules'
assert_rejected 'use crate::{tui, git};' 'command or TUI modules'
assert_rejected $'use crate::{\n    commands as cli_commands,\n    git,\n};' 'command or TUI modules'
assert_rejected 'fn bad() { crate :: tui :: run(); }' 'command or TUI modules'
assert_rejected 'mod commands { pub fn run() {} } fn bad() { commands::run(); }' 'command or TUI modules'

assert_rejected 'use gpui::App;' 'presentation frameworks'
assert_rejected 'use {ratatui as terminal_ui, git2};' 'presentation frameworks'
assert_rejected 'fn bad() { ::crossterm::execute!(); }' 'presentation frameworks'
assert_rejected 'use dialoguer as prompt;' 'presentation frameworks'
assert_rejected 'use colored::Colorize;' 'presentation frameworks'
assert_rejected 'use console::Style;' 'presentation frameworks'
assert_rejected 'fn bad() { :: gpui :: App::new(); }' 'presentation frameworks'
assert_rejected 'mod gpui { pub fn run() {} } fn bad() { gpui::run(); }' 'presentation frameworks'

assert_rejected 'use crate::progress::LiveTimer;' 'terminal progress'
assert_rejected 'use crate::{progress as terminal_progress, git};' 'terminal progress'
assert_rejected 'fn bad() { crate::progress::LiveTimer::new(); }' 'terminal progress'
assert_rejected 'fn bad() { crate :: progress :: LiveTimer::new(); }' 'terminal progress'
assert_rejected 'mod progress { pub fn run() {} } fn bad() { progress::run(); }' 'terminal progress'

assert_rejected 'use std::io::{stdout, IsTerminal};' 'terminal I/O'
assert_rejected 'use std::{io::stdout};' 'terminal I/O'
assert_rejected 'use std :: io :: { stderr as terminal_stderr, IsTerminal };' 'terminal I/O'
assert_rejected 'use std::io::stdin;' 'terminal I/O'
assert_rejected 'use std::io::stderr as terminal_stderr;' 'terminal I/O'
assert_rejected 'fn bad() { std::io::stdout(); }' 'terminal I/O'
assert_rejected 'use std::io as io; fn bad() { io::stdout(); }' 'terminal I/O'
assert_rejected 'use std::{io as terminal_io}; fn bad() { terminal_io::stderr(); }' 'terminal I/O'
assert_rejected 'use std::{io::{self as io}}; fn bad() { io::stdin(); }' 'terminal I/O'
assert_rejected 'use std::io as r#io; fn bad() { r#io::stdout(); }' 'terminal I/O'
assert_rejected 'use std::io::*;' 'terminal I/O'
assert_rejected 'use std::{io::*};' 'terminal I/O'
assert_rejected 'use std as standard; fn bad() { standard::io::stdout(); }' 'terminal I/O'
assert_rejected 'use {std as r#standard}; fn bad() { r#standard::io::stdin(); }' 'terminal I/O'
assert_rejected 'use std::{self as standard}; fn bad() { standard::io::stderr(); }' 'terminal I/O'
assert_rejected 'use std as standard; use standard::io as io; fn bad() { io::stdout(); }' 'terminal I/O'
assert_rejected 'extern crate std as standard; fn bad() { standard::io::stderr(); }' 'terminal I/O'
assert_rejected 'extern crate std as r#standard; fn bad() { r#standard::io::stdout(); }' 'terminal I/O'
assert_rejected 'fn bad() { io::stdout(); } use std::io as io;' 'terminal I/O'
assert_rejected 'fn bad() { standard::io::stdout(); } use std as standard;' 'terminal I/O'
assert_rejected 'fn bad() { standard::io::stderr(); } extern crate std as standard;' 'terminal I/O'
assert_rejected $'mod nested {\n    fn bad() { io::stdout(); }\n    use std::io as io;\n}' 'terminal I/O'
assert_rejected $'mod nested {\n    fn bad() { standard::io::stdout(); }\n    use std as standard;\n}' 'terminal I/O'
assert_rejected $'mod nested {\n    fn bad() { io::stderr(); }\n    use standard::io as io;\n    use std as standard;\n}' 'terminal I/O'
assert_rejected 'use std as standard; fn bad() { crate::standard::io::stdout(); }' 'terminal I/O'
assert_rejected $'use std as standard;\nmod nested {\n    fn bad() { super::standard::io::stdout(); }\n}' 'terminal I/O'
assert_rejected $'use std as standard;\nuse standard::io as io;\nmod nested {\n    fn bad() { super::io::stdout(); }\n}' 'terminal I/O'
assert_rejected $'use std as r#standard;\nmod outer {\n    mod nested {\n        fn bad() { super::super::r#standard::io::stderr(); }\n    }\n}' 'terminal I/O'
assert_rejected $'mod nested {\n    use std as standard;\n    fn bad() { self::standard::io::stdin(); }\n}' 'terminal I/O'
assert_rejected $'mod nested {\n    use standard::io as r#io;\n    use std as standard;\n    fn bad() { self::r#io::stderr(); }\n}' 'terminal I/O'
assert_rejected 'fn bad() { super::std::io::stdout(); }' 'terminal I/O'
assert_rejected 'fn bad() { super::r#commands::run(); }' 'command or TUI modules'
assert_rejected $'use std::io as io;\ntrait Output { fn stdout(); }\nfn bad<io: Output>() { self::io::stdout(); }' 'terminal I/O'
assert_rejected $'use std::io as io;\nfn bad<\'io, const io: usize>() { io::stdout(); }' 'terminal I/O'
assert_rejected 'fn bad<commands>() { commands::run(); }' 'command or TUI modules'
assert_rejected $'use std::io as io;\nmod nested {\n    mod io { pub fn stdout() {} }\n    fn clean() { io::stdout(); }\n}\nfn bad() { io::stdout(); }' 'terminal I/O'
assert_rejected $'use std::io as io;\nfn clean() {\n    use crate::model as io;\n    let _: Option<io::RepositorySnapshot> = None;\n}\nfn bad() { io::stdout(); }' 'terminal I/O'
assert_rejected 'use std::io as io; fn bad(io: usize) { let io = io; io::stdout(); }' 'terminal I/O'
assert_rejected $'use std::io as io;\ntrait Output { fn stdout(); }\nfn clean<io: Output>() { io::stdout(); }\nfn bad() { io::stdout(); }' 'terminal I/O'

assert_rejected 'print!("hidden terminal output");' 'terminal output macros'
assert_rejected 'println!("hidden terminal output");' 'terminal output macros'
assert_rejected 'println ! ("hidden spaced terminal output");' 'terminal output macros'
assert_rejected 'use std::{println as terminal_print};' 'terminal output macros'
assert_rejected 'eprint!("hidden terminal error");' 'terminal output macros'
assert_rejected 'std::eprintln!("hidden terminal error");' 'terminal output macros'
assert_rejected ':: std :: eprintln ! ("hidden qualified terminal error");' 'terminal output macros'
assert_rejected 'dbg!("hidden terminal debug");' 'terminal output macros'

assert_rejected 'extern crate gpui as ui;' 'presentation frameworks'
assert_rejected 'extern crate r#console as r#ui;' 'presentation frameworks'
assert_rejected 'extern crate commands as cli_commands;' 'command or TUI modules'
assert_rejected 'extern crate tui;' 'command or TUI modules'
assert_rejected 'extern crate progress as terminal_progress;' 'terminal progress'

assert_accepted 'pub fn commandster_progress(stdout_buffer: usize) -> usize { stdout_buffer }'
assert_accepted '// use crate::commands::submit; println!("not code");'
assert_accepted '/* use gpui::App; /* std::io::stdout(); */ crate::progress::LiveTimer */ pub fn clean() {}'
assert_accepted 'const TEXT: &str = "crate::commands::submit println!(hidden)";'
assert_accepted 'const RAW: &str = r###"use gpui::App; std::io::stdout();"###;'
assert_accepted 'const BYTES: &[u8] = b"use crate::tui; eprintln!(hidden)";'
assert_accepted 'const RAW_BYTES: &[u8] = br##"use dialoguer::Select; dbg!(hidden)"##;'
assert_accepted "const CHARACTER: char = 'p'; const BYTE: u8 = b'p';"
assert_accepted "fn lifetime<'progress>(value: &'progress str) -> &'progress str { value }"
assert_accepted 'use crate::model as r#type; fn clean(value: r#type::RepositorySnapshot) { drop(value); }'
assert_accepted 'use std::io as io; fn clean<T: io::Read>() {}'
assert_accepted 'use std::{io as r#type}; fn clean<T: r#type::Write>() {}'
assert_accepted $'use std::io as io;\nmod nested {\n    mod io { pub fn stdout() {} }\n    fn clean() { io::stdout(); }\n}'
assert_accepted $'use std as standard;\nmod nested {\n    mod standard { pub mod io { pub fn stdout() {} } }\n    fn clean() { standard::io::stdout(); }\n}'
assert_accepted $'use std::io as io;\nfn clean() {\n    use crate::model as io;\n    let _: Option<io::RepositorySnapshot> = None;\n}'
assert_accepted $'use std::io as io;\nfn clean() {\n    mod r#io { pub fn stdout() {} }\n    r#io::stdout();\n}'
assert_accepted $'use std::io as io;\ntrait Output { fn stdout(); }\nfn clean<io: Output>() { io::stdout(); }'
assert_accepted $'fn clean() { std::io::stdout(); }\nmod std { pub mod io { pub fn stdout() {} } }'
assert_accepted $'use std::io as io;\nmod nested {\n    fn clean() { io::stdout(); }\n    mod io { pub fn stdout() {} }\n}'
assert_accepted $'use std as standard;\nmod nested {\n    fn clean() { standard::io::stdout(); }\n    mod standard { pub mod io { pub fn stdout() {} } }\n}'
assert_accepted $'use std::io as io;\nmod nested {\n    fn clean() { io::stdout(); }\n    use self::local_io as io;\n    mod local_io { pub fn stdout() {} }\n}'
assert_accepted $'mod standard { pub mod io { pub fn stdout() {} } }\nfn clean() { crate::standard::io::stdout(); }'
assert_accepted $'use std as standard;\nmod outer {\n    mod nested {\n        fn clean() { super::standard::io::stdout(); }\n    }\n    mod standard { pub mod io { pub fn stdout() {} } }\n}'
assert_accepted $'use std as standard;\nmod nested {\n    fn clean() { self::standard::io::stdout(); }\n    mod standard { pub mod io { pub fn stdout() {} } }\n}'
assert_accepted $'use std::io as io;\ntrait Output { type stdout; }\nfn clean<\'a, io: Output, const N: usize>() where io::stdout: Output {\n    let _: Option<io::stdout> = None;\n}'
assert_accepted $'use std::io as io;\ntrait Output { fn stdout(); }\nstruct Holder<T>(T);\nimpl<io: Output> Holder<io> where io::stdout: Output {\n    fn clean() { io::stdout(); }\n}'
assert_accepted $'use std::io as io;\ntrait Clean<io> where io::stdout: Sized {\n    fn clean() { let _: Option<io::stdout> = None; }\n}'
assert_accepted $'use std::io as io;\nstruct Clean<io> where io::stdout: Sized { value: Option<io::stdout> }\nenum Choice<io> where io::stdout: Sized { Value(io::stdout) }\nunion Either<io> where io::stdout: Copy { value: std::mem::ManuallyDrop<io::stdout> }\ntype Alias<io> where io::stdout: Sized = io::stdout;'
assert_accepted 'use super::RepositorySnapshot;'
assert_accepted '// extern crate gpui as ui; use std::io as io; io::stdout();'
assert_accepted 'const TEXT: &str = "extern crate tui; use std::io::*;";'
assert_scanner_error 'use std::io as #;'
assert_scanner_error 'extern crate safe_dependency as ;'

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
