#!/usr/bin/env bash
# Cut a Qinip release. The only way a version number changes.
#
# Usage:
#   ./scripts/release.sh 1.1.0            # prepare + commit + tag, then asks before pushing
#   ./scripts/release.sh 1.1.0 --dry-run  # run every check, change nothing
#
# What it does, in order (stops at the first failed check):
#   1. You are on master, in sync with origin, with no uncommitted changes.
#   2. The version is valid SemVer, newer than Cargo.toml's, and not tagged yet.
#   3. CHANGELOG.md has entries under "## [Unreleased]".
#   4. cargo test and cargo clippy pass.
#   5. Moves Unreleased into "## [X.Y.Z] - today", bumps Cargo.toml + Cargo.lock,
#      commits "Release vX.Y.Z" and creates an annotated tag vX.Y.Z.
#   6. Asks, then pushes master + the tag. The tag starts the GitHub release
#      workflow, which builds Mac + Windows and publishes the release page
#      with this version's changelog section as its text.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$(dirname "$SCRIPT_DIR")"

VERSION="${1:-}"
DRY_RUN=false
[ "${2:-}" = "--dry-run" ] && DRY_RUN=true

fail() { echo "✗ $*" >&2; exit 1; }
ok() { echo "✓ $*"; }

[ -n "$VERSION" ] || fail "Usage: $0 X.Y.Z [--dry-run]"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "\"$VERSION\" is not X.Y.Z"
TAG="v$VERSION"

# 1. Branch and working tree
BRANCH="$(git branch --show-current)"
[ "$BRANCH" = "master" ] || fail "Releases are cut from master (you are on $BRANCH). Merge your branch first."
[ -z "$(git status --porcelain)" ] || fail "Uncommitted changes. Commit or stash them first."
git fetch -q origin master
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/master)" ] || fail "master differs from origin/master. Pull or push first."
ok "on master, clean, in sync with origin"

# 2. Version
CURRENT="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
newer() { [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -1)" = "$2" ] && [ "$1" != "$2" ]; }
newer "$CURRENT" "$VERSION" || fail "$VERSION is not newer than the current $CURRENT"
git rev-parse -q --verify "refs/tags/$TAG" >/dev/null && fail "tag $TAG already exists"
ok "version $CURRENT → $VERSION"

# 3. Changelog
UNRELEASED="$(awk '/^## \[Unreleased\]/{f=1;next} /^## \[/{f=0} f' CHANGELOG.md | grep -v '^[[:space:]]*$' || true)"
[ -n "$UNRELEASED" ] || fail "CHANGELOG.md has nothing under ## [Unreleased]. Write what changed first."
ok "changelog has $(echo "$UNRELEASED" | grep -c '^- ') entries"

# 4. Quality gate
cargo test --quiet >/dev/null 2>&1 || fail "cargo test failed (run it to see why)"
ok "tests pass"
cargo clippy --all-targets --quiet -- -D warnings >/dev/null 2>&1 || fail "clippy has warnings (CI is strict; run cargo clippy --all-targets)"
ok "clippy clean"

if $DRY_RUN; then
  echo ""
  echo "Dry run: all checks passed. Nothing was changed."
  exit 0
fi

# 5. Prepare the release commit + tag
TODAY="$(date +%Y-%m-%d)"
python3 - "$VERSION" "$TODAY" "$CURRENT" <<'PY'
import re, sys
version, today, previous = sys.argv[1:4]
p = "CHANGELOG.md"
s = open(p).read()
s = s.replace("## [Unreleased]", f"## [Unreleased]\n\n## [{version}] - {today}", 1)
s = re.sub(r"^\[Unreleased\]: (.*)/compare/v[^.]+\.[^.]+\.[^.]+\.\.\.HEAD$",
           lambda m: f"[Unreleased]: {m.group(1)}/compare/v{version}...HEAD\n[{version}]: {m.group(1)}/compare/v{previous}...v{version}",
           s, count=1, flags=re.M)
open(p, "w").write(s)

# Bump only the [package] version (the first `version = ` line).
p = "Cargo.toml"
s = open(p).read()
s = re.sub(r'^version = ".*"$', f'version = "{version}"', s, count=1, flags=re.M)
open(p, "w").write(s)
PY
cargo update --quiet -p qinip
git add CHANGELOG.md Cargo.toml Cargo.lock
git commit -q -m "Release $TAG"
NOTES="$(awk -v v="$VERSION" '$0 ~ "^## \\[" v "\\]" {f=1;next} /^## \[/{f=0} f' CHANGELOG.md)"
git tag -a "$TAG" -m "Qinip $VERSION" -m "$NOTES"
ok "committed and tagged $TAG"

# 6. Publish
echo ""
read -r -p "Push master and $TAG now? This starts the public release build. [y/N] " answer
if [ "$answer" = "y" ] || [ "$answer" = "Y" ]; then
  git push origin master "$TAG"
  ok "pushed. Watch it: https://github.com/Hersh3yy/qinip/actions"
else
  echo "Not pushed. When ready: git push origin master $TAG"
  echo "To undo instead: git tag -d $TAG && git reset --hard HEAD~1"
fi
