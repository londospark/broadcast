#!/usr/bin/env bash
# scripts/install-maxine-runtime.sh
#
# Installs NVIDIA Maxine GPU noise suppression from broadcast's own GitHub
# release — no NGC account or API key needed. Downloads the pre-built
# runtime bundle (shared libraries + pretrained models for desktop RTX
# GPUs, Turing through Blackwell — see build-maxine-runtime-bundle.sh) and
# the compiled LADSPA plugin, and installs them to the same locations
# install-maxine-sdk.sh would.
#
# If your GPU architecture isn't covered by the bundle (anything other
# than a Turing/Ampere/Ada/Blackwell RTX card), use install-maxine-sdk.sh
# instead, which builds directly from NVIDIA's SDK with your own NGC key.
#
# Usage:
#   bash scripts/install-maxine-runtime.sh

set -euo pipefail

REPO="londospark/broadcast"
INSTALL_DIR="${NVAFX_SDK_INSTALL_DIR:-$HOME/.local/share/nvidia-maxine-sdk}"
CURRENT_LINK="$INSTALL_DIR/current"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; NC='\033[0m'
info()    { echo -e "${BLUE}[info]${NC}  $*" >&2; }
success() { echo -e "${GREEN}[ok]${NC}    $*" >&2; }
warn()    { echo -e "${YELLOW}[warn]${NC}  $*" >&2; }
error()   { echo -e "${RED}[error]${NC} $*" >&2; }

if ! command -v gh &>/dev/null; then
    error "gh (GitHub CLI) not found — install it from https://cli.github.com"
    exit 1
fi

tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT

info "Fetching latest release info..."
tag=$(gh release view --repo "$REPO" --json tagName -q .tagName)

info "Downloading Maxine runtime bundle and plugin from $tag..."
gh release download "$tag" --repo "$REPO" \
    -p 'broadcast-maxine-runtime-linux-x86_64.tar.gz' \
    -p 'libbroadcast_maxine_ladspa.so' \
    --dir "$tmp_dir" --clobber

sdk_dir="$INSTALL_DIR/bundled-$tag"
rm -rf "$sdk_dir"
mkdir -p "$sdk_dir"
tar xzf "$tmp_dir/broadcast-maxine-runtime-linux-x86_64.tar.gz" -C "$sdk_dir"
ln -sfn "$sdk_dir" "$CURRENT_LINK"
success "Runtime installed: $CURRENT_LINK → $sdk_dir"

mkdir -p "$HOME/.local/lib/ladspa"
cp "$tmp_dir/libbroadcast_maxine_ladspa.so" "$HOME/.local/lib/ladspa/libmaxine_ladspa.so"
success "Plugin installed: $HOME/.local/lib/ladspa/libmaxine_ladspa.so"

echo ""
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}  NVIDIA Maxine runtime installed!${NC}"
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo "  Activate the Maxine backend:"
echo "    broadcast-ctl set-backend maxine"
echo "    broadcast-ctl install-config --apply"
echo ""
echo "  Or change the Backend dropdown in broadcast-gui."
echo ""
echo "  broadcast-ctl auto-detects your GPU architecture at PipeWire start"
echo "  and picks the matching model. If your card isn't Turing/Ampere/Ada/"
echo "  Blackwell RTX, this bundle won't have a matching model — use"
echo "  install-maxine-sdk.sh with your own NGC key instead."
echo ""
