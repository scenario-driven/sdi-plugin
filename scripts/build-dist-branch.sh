#!/bin/sh
# Manual dist-branch builder. Use only as a fallback when release.yml is
# broken — the CI workflow is the canonical path. This script builds for
# the local platform only and pushes a dist branch containing that single
# binary; the marketplace will refuse to install on any other platform.
#
# Usage:  scripts/build-dist-branch.sh v0.x.0
set -eu

TAG="${1:-}"
if [ -z "$TAG" ]; then
  echo "usage: $0 <tag>  (e.g. v0.1.0)" >&2
  exit 64
fi
if [ -n "$(git status --porcelain)" ]; then
  echo "fatal: working tree not clean — commit or stash first" >&2
  exit 1
fi

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

OS="$(uname -s)"
ARCH="$(uname -m)"
case "$OS/$ARCH" in
  Darwin/x86_64)  TRIPLE=x86_64-apple-darwin ;;
  Darwin/arm64)   TRIPLE=aarch64-apple-darwin ;;
  Linux/x86_64)   TRIPLE=x86_64-unknown-linux-gnu ;;
  Linux/aarch64)  TRIPLE=aarch64-unknown-linux-gnu ;;
  *) echo "unsupported platform: $OS/$ARCH" >&2; exit 78 ;;
esac

echo "[dist] building for $TRIPLE"
cargo build --release -p sdi-cli -p sdi-daemon

STAGE="$(mktemp -d)"
mkdir -p "$STAGE/plugin/bin" "$STAGE/plugin/daemon/bin"
cp "target/release/sdi"  "$STAGE/plugin/bin/sdi-$TRIPLE"
cp "target/release/sdid" "$STAGE/plugin/daemon/bin/sdid-$TRIPLE"

cat > "$STAGE/plugin/bin/sdi" <<'EOSH'
#!/bin/sh
set -e
dir="$(cd "$(dirname "$0")" && pwd)"
os="$(uname -s)"
arch="$(uname -m)"
case "$os/$arch" in
  Darwin/x86_64)  triple=x86_64-apple-darwin ;;
  Darwin/arm64)   triple=aarch64-apple-darwin ;;
  Linux/x86_64)   triple=x86_64-unknown-linux-gnu ;;
  Linux/aarch64)  triple=aarch64-unknown-linux-gnu ;;
  *) echo "[sdi] unsupported platform: $os/$arch" >&2; exit 78 ;;
esac
exec "$dir/sdi-$triple" "$@"
EOSH
chmod +x "$STAGE/plugin/bin/sdi"

cat > "$STAGE/plugin/daemon/bin/sdid" <<'EOSH'
#!/bin/sh
set -e
dir="$(cd "$(dirname "$0")" && pwd)"
os="$(uname -s)"
arch="$(uname -m)"
case "$os/$arch" in
  Darwin/x86_64)  triple=x86_64-apple-darwin ;;
  Darwin/arm64)   triple=aarch64-apple-darwin ;;
  Linux/x86_64)   triple=x86_64-unknown-linux-gnu ;;
  Linux/aarch64)  triple=aarch64-unknown-linux-gnu ;;
  *) echo "[sdi] unsupported platform: $os/$arch" >&2; exit 78 ;;
esac
exec "$dir/sdid-$triple" "$@"
EOSH
chmod +x "$STAGE/plugin/daemon/bin/sdid"

# Copy plugin shell + marketplace manifest (source-tree, excluding binaries).
# Exclude patterns are relative to the rsync source (plugin/). web/dist/ IS
# included if you built it locally (the daemon's ServeDir picks it up); to
# build it run `pnpm --dir plugin/web install && pnpm --dir plugin/web build`
# before invoking this script.
rsync -a \
  --exclude='bin/' \
  --exclude='daemon/bin/' \
  --exclude='web/node_modules/' \
  --exclude='web/*.tsbuildinfo' \
  --exclude='web/.vite/' \
  plugin/ "$STAGE/plugin/"
cp -R .claude-plugin "$STAGE/.claude-plugin"
cp README.md LICENSE "$STAGE/" 2>/dev/null || true
echo "$TAG" > "$STAGE/plugin/.sdi-version"

WORKTREE="$(mktemp -d)"
git worktree add --orphan -b dist "$WORKTREE"
cd "$WORKTREE"
git rm -rf . >/dev/null 2>&1 || true
cp -R "$STAGE"/. .
git add -A
git commit -m "manual release ${TAG} (single-platform: ${TRIPLE})"
echo
echo "[dist] worktree ready at: $WORKTREE"
echo "[dist] inspect, then:  git -C $WORKTREE push origin dist --force"
echo "[dist] cleanup:        git worktree remove --force $WORKTREE"
