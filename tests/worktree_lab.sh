#!/usr/bin/env bash
set -euo pipefail

if [[ -n "${1:-}" ]]; then
  ROOT="$1"
  mkdir -p "$ROOT"
else
  ROOT="$(mktemp -d /tmp/stax-worktree-lab-XXXXXX)"
fi

REPO="$ROOT/repo"
REMOTE="$ROOT/remote.git"
WT_A="$ROOT/wt-A"
WT_B="$ROOT/wt-B"

STAX_BIN="${STAX_BIN:-/Users/kacper/Documents/Tools/stax/target/debug/stax}"
if [[ ! -x "$STAX_BIN" ]]; then
  echo "Building stax binary..."
  cargo build --bin stax
  STAX_BIN="/Users/kacper/Documents/Tools/stax/target/debug/stax"
fi

run_git() {
  local cwd="$1"
  shift
  git -C "$cwd" "$@"
}

echo "[lab] root: $ROOT"
mkdir -p "$REPO"
run_git "$ROOT" init --bare "$REMOTE" >/dev/null
run_git "$REPO" init -b main >/dev/null
run_git "$REPO" config user.email test@example.com
run_git "$REPO" config user.name "Test User"

echo "init" > "$REPO/README.md"
run_git "$REPO" add README.md
run_git "$REPO" commit -m "Initial commit" >/dev/null
run_git "$REPO" remote add origin "$REMOTE"
run_git "$REPO" push -u origin main >/dev/null

create_branch() {
  local name="$1"
  local file="$2"
  "$STAX_BIN" create "$name" >/dev/null
  echo "$name" > "$REPO/$file"
  run_git "$REPO" add "$file"
  run_git "$REPO" commit -m "$name commit" >/dev/null
  run_git "$REPO" push -u origin "$(run_git "$REPO" rev-parse --abbrev-ref HEAD)" >/dev/null
}

cd "$REPO"
create_branch "A" "a.txt"
A_BRANCH="$(run_git "$REPO" rev-parse --abbrev-ref HEAD)"
create_branch "B" "b.txt"
B_BRANCH="$(run_git "$REPO" rev-parse --abbrev-ref HEAD)"
create_branch "C" "c.txt"
C_BRANCH="$(run_git "$REPO" rev-parse --abbrev-ref HEAD)"
"$STAX_BIN" trunk >/dev/null
create_branch "D" "d.txt"
D_BRANCH="$(run_git "$REPO" rev-parse --abbrev-ref HEAD)"
create_branch "E" "e.txt"
E_BRANCH="$(run_git "$REPO" rev-parse --abbrev-ref HEAD)"

"$STAX_BIN" checkout main >/dev/null
run_git "$REPO" worktree add "$WT_A" "$A_BRANCH" >/dev/null
run_git "$REPO" worktree add "$WT_B" "$B_BRANCH" >/dev/null

echo "main update" > "$REPO/main-update.txt"
run_git "$REPO" add main-update.txt
run_git "$REPO" commit -m "Main update" >/dev/null
run_git "$REPO" push origin main >/dev/null

echo
printf "[lab] branches: main -> %s -> %s -> %s and main -> %s -> %s\n" \
  "$A_BRANCH" "$B_BRANCH" "$C_BRANCH" "$D_BRANCH" "$E_BRANCH"
echo "[lab] worktrees:"
run_git "$REPO" worktree list
echo

echo "[lab] status --json from wt-B"
cd "$WT_B"
if command -v jq >/dev/null 2>&1; then
  "$STAX_BIN" status --json --quiet | jq '.'
else
  "$STAX_BIN" status --json --quiet
fi

echo
echo "[lab] diff from wt-B"
"$STAX_BIN" diff

echo
echo "[lab] restack --all from wt-B"
set +e
"$STAX_BIN" restack --all
RESTACK_EC=$?
set -e
echo "[lab] restack exit code: $RESTACK_EC"

echo
echo "[lab] sync --restack --force --safe --no-delete from wt-B"
set +e
"$STAX_BIN" sync --restack --force --safe --no-delete
SYNC_EC=$?
set -e
echo "[lab] sync exit code: $SYNC_EC"

echo
MAIN_LOCAL="$(run_git "$REPO" rev-parse main)"
MAIN_REMOTE="$(run_git "$REPO" rev-parse origin/main)"
echo "[lab] main local:  $MAIN_LOCAL"
echo "[lab] main remote: $MAIN_REMOTE"

cat <<TXT

Lab ready at:
  $ROOT

Worktree paths:
  $REPO (main)
  $WT_A ($A_BRANCH)
  $WT_B ($B_BRANCH)

You can re-run commands manually using:
  STAX_BIN="$STAX_BIN"
TXT
