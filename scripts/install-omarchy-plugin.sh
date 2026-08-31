#!/usr/bin/env bash
# scripts/install-omarchy-plugin.sh
#
# Syncs the Broadcast Omarchy bar-widget plugin from this repo into
# ~/.config/omarchy/plugins/. Omarchy's plugin validator rejects symlinks
# inside a plugin folder, so this is a real copy rather than a symlink —
# re-run this script after editing files under omarchy-plugin/ to push
# changes live (Quickshell picks up the change automatically).
#
# Usage:
#   bash scripts/install-omarchy-plugin.sh [--enable] [--put]

set -euo pipefail

PLUGIN_ID="io.github.londospark.broadcast"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC_DIR="$REPO_ROOT/omarchy-plugin/$PLUGIN_ID"
DEST_DIR="$HOME/.config/omarchy/plugins/$PLUGIN_ID"

if [ ! -d "$SRC_DIR" ]; then
    echo "error: $SRC_DIR not found" >&2
    exit 1
fi

mkdir -p "$(dirname "$DEST_DIR")"
rsync -a --delete "$SRC_DIR/" "$DEST_DIR/"
echo "Synced $SRC_DIR -> $DEST_DIR"

if command -v omarchy >/dev/null 2>&1; then
    omarchy plugin validate "$DEST_DIR"
    echo "Manifest valid."
fi

for arg in "$@"; do
    case "$arg" in
        --enable)
            omarchy plugin enable "$PLUGIN_ID"
            ;;
        --put)
            omarchy bar put "$PLUGIN_ID"
            ;;
    esac
done

echo ""
echo "Next steps (if not passed as flags above):"
echo "  omarchy plugin enable $PLUGIN_ID"
echo "  omarchy bar put $PLUGIN_ID"
