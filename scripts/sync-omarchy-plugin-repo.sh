#!/usr/bin/env bash
# scripts/sync-omarchy-plugin-repo.sh
#
# Mirrors omarchy-plugin/<id>/ from this repo into the standalone
# londospark/broadcast-omarchy-plugin repo, so `omarchy plugin add` can
# install straight from GitHub (Omarchy's installer requires manifest.json
# at the target repo's root, which rules out pointing it at this monorepo
# directly).
#
# Also keeps the Maxine install snippet in that repo's README pinned to
# this repo's current commit, per the marketplace's security baseline
# (unpinned remote source execution is flagged as a finding).
#
# Run by .github/workflows/sync-omarchy-plugin.yml on every push to main
# that touches omarchy-plugin/**. Safe to run locally too — it's a no-op
# if there's nothing new to sync.
#
# Usage:
#   bash scripts/sync-omarchy-plugin-repo.sh

set -euo pipefail

PLUGIN_ID="io.github.londospark.broadcast"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC_DIR="$REPO_ROOT/omarchy-plugin/$PLUGIN_ID"
PLUGIN_REPO_URL="${PLUGIN_REPO_URL:-git@github.com:londospark/broadcast-omarchy-plugin.git}"

if [ ! -d "$SRC_DIR" ]; then
    echo "error: $SRC_DIR not found" >&2
    exit 1
fi

BROADCAST_SHA="$(git -C "$REPO_ROOT" rev-parse HEAD)"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

git clone -q "$PLUGIN_REPO_URL" "$tmp_dir"

# The plugin repo's own README.md and LICENSE aren't generated from
# omarchy-plugin/ (README has distro-specific install instructions this
# repo has no notion of); only sync the actual plugin files, plus LICENSE
# which does mirror this repo's.
rsync -a --delete \
    --exclude .git --exclude README.md --exclude LICENSE \
    "$SRC_DIR/" "$tmp_dir/"
cp "$REPO_ROOT/LICENSE" "$tmp_dir/LICENSE"

# Keep the Maxine install snippet's pinned commit current.
sed -i -E "s#(git -C /tmp/broadcast checkout --detach )[0-9a-f]{40}#\1${BROADCAST_SHA}#" \
    "$tmp_dir/README.md"

cd "$tmp_dir"
git add -A
if git diff --cached --quiet; then
    echo "Nothing to sync — plugin repo already matches broadcast@${BROADCAST_SHA:0:7}"
    exit 0
fi

git -c user.name="broadcast-sync" -c user.email="actions@github.com" \
    commit -q -m "Sync plugin from broadcast@${BROADCAST_SHA:0:7}"
git push -q origin main
echo "Synced to $PLUGIN_REPO_URL (broadcast@${BROADCAST_SHA:0:7})"
