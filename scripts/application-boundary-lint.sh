#!/usr/bin/env bash
set -euo pipefail

root="${1:-.}"
files=()
while IFS= read -r file; do
  test -z "$file" || files+=("$root/$file")
done < <(
  cd "$root"
  git ls-files --cached --others --exclude-standard \
    'src/application/*.rs' 'src/application/**/*.rs' |
    LC_ALL=C sort -u
)

check() {
  local label="$1"
  local pattern="$2"
  if ((${#files[@]})) && rg -n "$pattern" "${files[@]}"; then
    echo "application boundary violation: $label" >&2
    exit 1
  fi
}

check "command or TUI modules" '(^|[^[:alnum:]_])((crate|super|self)::)?(\{[^}]*[,{][[:space:]]*)?(commands|tui)(::|[[:space:]};,]|$)'
check "presentation frameworks" '(^|[^[:alnum:]_])(gpui|ratatui|crossterm|dialoguer|colored|console)(::|[[:space:]};,]|$)'
check "terminal progress" '(^|[^[:alnum:]_])((crate|super|self)::)?(\{[^}]*[,{][[:space:]]*)?progress(::|[[:space:]};,]|$)'
check "terminal I/O" 'std::io::(\{[^}]*(stdin|stdout|stderr|IsTerminal)|stdin|stdout|stderr|IsTerminal)'
check "terminal output macros" '(^|[^[:alnum:]_])(std::)?(print|println|eprint|eprintln|dbg)![[:space:]]*\('
