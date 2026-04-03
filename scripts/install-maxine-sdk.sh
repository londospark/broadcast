#!/usr/bin/env bash
# scripts/install-maxine-sdk.sh
#
# Downloads the NVIDIA Maxine Audio Effects SDK (v2.x) from NGC and
# builds the broadcast-maxine-ladspa LADSPA plugin.
#
# Prerequisites:
#   - An NGC API key: https://org.ngc.nvidia.com/setup/api-key
#   - NVIDIA driver 570+ installed
#   - Rust toolchain (cargo) on PATH
#
# Usage:
#   NGC_API_KEY="your_key_here" bash scripts/install-maxine-sdk.sh

set -euo pipefail

INSTALL_DIR="${NVAFX_SDK_INSTALL_DIR:-$HOME/.local/share/nvidia-maxine-sdk}"
CURRENT_LINK="$INSTALL_DIR/current"
NGC_ORG="nvidia"
NGC_TEAM="maxine"
SDK_RESOURCE="maxine_linux_audio_effects_sdk"
SDK_VERSION="2.1.0"
NGC_CLI_VERSION="4.16.0"
NGC_CLI_DIR="$HOME/.local/share/ngc-cli"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; NC='\033[0m'
info()    { echo -e "${BLUE}[info]${NC}  $*"; }
success() { echo -e "${GREEN}[ok]${NC}    $*"; }
warn()    { echo -e "${YELLOW}[warn]${NC}  $*"; }
error()   { echo -e "${RED}[error]${NC} $*" >&2; }

# ── Preflight ──────────────────────────────────────────────────────────
if [ -z "${NGC_API_KEY:-}" ]; then
    error "NGC_API_KEY is not set."
    echo ""
    echo "  1. Create a free NVIDIA developer account at https://developer.nvidia.com"
    echo "  2. Generate an API key at https://org.ngc.nvidia.com/setup/api-key"
    echo "  3. Re-run:  NGC_API_KEY=your_key bash scripts/install-maxine-sdk.sh"
    exit 1
fi

if ! command -v cargo &>/dev/null; then
    error "cargo not found — install Rust from https://rustup.rs"
    exit 1
fi

# ── Install ngc CLI ─────────────────────────────────────────────────────
install_ngc_cli() {
    if command -v ngc &>/dev/null && ngc --version &>/dev/null; then
        success "ngc CLI already installed"
        return
    fi

    info "Downloading ngc CLI ${NGC_CLI_VERSION}..."
    local tmp_dir
    tmp_dir=$(mktemp -d)
    wget -q "https://api.ngc.nvidia.com/v2/resources/nvidia/ngc-apps/ngc_cli/versions/${NGC_CLI_VERSION}/files/ngccli_linux.zip" \
        -O "$tmp_dir/ngccli_linux.zip"
    unzip -q "$tmp_dir/ngccli_linux.zip" -d "$tmp_dir"
    rm -rf "$NGC_CLI_DIR"
    cp -r "$tmp_dir/ngc-cli" "$NGC_CLI_DIR"
    rm -rf "$tmp_dir"

    mkdir -p "$HOME/.local/bin"
    ln -sf "$NGC_CLI_DIR/ngc" "$HOME/.local/bin/ngc"
    export PATH="$HOME/.local/bin:$PATH"
    success "ngc CLI installed at $NGC_CLI_DIR"
}

# ── Configure ngc ──────────────────────────────────────────────────────
configure_ngc() {
    mkdir -p "$HOME/.ngc"
    cat > "$HOME/.ngc/config" <<EOF
[CURRENT]
apikey = ${NGC_API_KEY}
format_type = ascii
EOF
    success "ngc CLI configured"
}

# ── Download SDK ────────────────────────────────────────────────────────
download_sdk() {
    mkdir -p "$INSTALL_DIR"

    local sdk_dir="$INSTALL_DIR/${SDK_RESOURCE}_v${SDK_VERSION}"
    if [ -f "$sdk_dir/nvafx/include/nvAudioEffects.h" ]; then
        success "SDK v${SDK_VERSION} already present at $sdk_dir"
        echo "$sdk_dir"
        return
    fi

    info "Downloading Maxine Audio Effects SDK v${SDK_VERSION}..."
    ngc registry resource download-version \
        --org "$NGC_ORG" --team "$NGC_TEAM" \
        --dest "$INSTALL_DIR" \
        "${NGC_ORG}/${NGC_TEAM}/${SDK_RESOURCE}:${SDK_VERSION}"

    # The download lands a .tar.gz — find and extract it
    local tarball
    tarball=$(find "$INSTALL_DIR" -maxdepth 2 -name "*.tar.gz" | sort | tail -1)
    if [ -n "$tarball" ]; then
        info "Extracting SDK archive..."
        tar xzf "$tarball" -C "$INSTALL_DIR"
    fi

    # Locate the extracted Audio_Effects_SDK directory
    sdk_dir=$(find "$INSTALL_DIR" -maxdepth 2 -name "nvAudioEffects.h" -exec dirname {} \; 2>/dev/null | \
              xargs -I{} dirname {} | head -1 || true)
    if [ -z "$sdk_dir" ]; then
        # Fallback: look for Audio_Effects_SDK folder
        sdk_dir=$(find "$INSTALL_DIR" -maxdepth 2 -type d -name "Audio_Effects_SDK" | head -1 || true)
    fi
    if [ -z "$sdk_dir" ] || [ ! -f "$sdk_dir/nvafx/include/nvAudioEffects.h" ]; then
        error "Could not locate extracted SDK. Expected nvafx/include/nvAudioEffects.h under $INSTALL_DIR"
        exit 1
    fi

    success "SDK at: $sdk_dir"
    echo "$sdk_dir"
}

# ── Download denoiser models ────────────────────────────────────────────
download_models() {
    local sdk_dir="$1"
    local features_dir="$sdk_dir/features"
    local download_script="$features_dir/download_features.sh"

    if [ ! -f "$download_script" ]; then
        warn "download_features.sh not found — skipping model download"
        return
    fi

    # Map nvidia-smi compute capability to GPU flag used by the script
    local gpu_flag=""
    if command -v nvidia-smi &>/dev/null; then
        local sm_ver
        sm_ver=$(nvidia-smi --query-gpu=compute_cap --format=csv,noheader 2>/dev/null | head -1 | tr -d '.' || echo "")
        case "$sm_ver" in
            120*) gpu_flag="--gpu rtx_pro_6000" ;;  # Blackwell (RTX 5080/5090)
            89*)  gpu_flag="--gpu l40" ;;            # Ada Lovelace (RTX 4090)
            86*)  gpu_flag="--gpu a10" ;;            # Ampere (RTX 3090)
            80*)  gpu_flag="--gpu a100" ;;           # Ampere (A100)
            75*)  gpu_flag="--gpu t4" ;;             # Turing (RTX 2080)
        esac
        [ -n "$gpu_flag" ] && info "Detected GPU SM${sm_ver} → $gpu_flag"
    fi

    info "Downloading denoiser-48k models..."
    chmod +x "$download_script"
    pushd "$features_dir" > /dev/null
    NGC_API_KEY="$NGC_API_KEY" bash "$download_script" \
        $gpu_flag --effects denoiser-48k \
        --ngc-org "$NGC_ORG" --ngc-team "$NGC_TEAM" \
    || NGC_API_KEY="$NGC_API_KEY" bash "$download_script" \
        $gpu_flag --effects denoiser-48k \
    || warn "Model download failed — you can download manually from NGC"
    popd > /dev/null
    success "Models downloaded"
}

# ── Create versioned symlink ────────────────────────────────────────────
setup_symlink() {
    local sdk_dir="$1"
    ln -sfn "$sdk_dir" "$CURRENT_LINK"
    success "Symlink: $CURRENT_LINK → $sdk_dir"
}

# ── Update fish config ──────────────────────────────────────────────────
update_fish_config() {
    local fish_cfg="$HOME/dotfiles/fish/.config/fish/config.fish"
    [ -f "$fish_cfg" ] || fish_cfg="$HOME/.config/fish/config.fish"
    [ -f "$fish_cfg" ] || return

    # Set NVAFX_SDK if not already present
    if ! grep -q "NVAFX_SDK" "$fish_cfg" 2>/dev/null; then
        cat >> "$fish_cfg" <<'FISHEOF'

# NVIDIA Maxine Audio Effects SDK for broadcast noise suppression
set -gx NVAFX_SDK "$HOME/.local/share/nvidia-maxine-sdk/current"
fish_add_path --path "$NVAFX_SDK/nvafx/lib"
fish_add_path --path "$NVAFX_SDK/features/denoiser/lib"
fish_add_path --path "$NVAFX_SDK/external/cuda/lib"
if set -q LD_LIBRARY_PATH
    set -gx LD_LIBRARY_PATH "$NVAFX_SDK/nvafx/lib:$NVAFX_SDK/features/denoiser/lib:$NVAFX_SDK/external/cuda/lib:$LD_LIBRARY_PATH"
else
    set -gx LD_LIBRARY_PATH "$NVAFX_SDK/nvafx/lib:$NVAFX_SDK/features/denoiser/lib:$NVAFX_SDK/external/cuda/lib"
end
FISHEOF
        success "Updated $fish_cfg"
    else
        success "fish config already has NVAFX_SDK"
    fi
}

# ── Build the LADSPA plugin ─────────────────────────────────────────────
build_plugin() {
    local repo_root
    repo_root="$(cd "$(dirname "$0")/.." && pwd)"

    info "Building broadcast-maxine-ladspa plugin..."
    NVAFX_SDK="$CURRENT_LINK" cargo build --release \
        -p broadcast-maxine-ladspa \
        --manifest-path "$repo_root/Cargo.toml"

    local so="$repo_root/target/release/libbroadcast_maxine_ladspa.so"
    if [ ! -f "$so" ]; then
        error "Build failed — $so not found"
        exit 1
    fi

    # Verify the ladspa_descriptor symbol is present (non-empty build)
    if ! nm -D "$so" 2>/dev/null | grep -q "ladspa_descriptor"; then
        error "Plugin built but ladspa_descriptor symbol missing — SDK may not have been found"
        exit 1
    fi

    mkdir -p "$HOME/.local/lib/ladspa"
    cp "$so" "$HOME/.local/lib/ladspa/libmaxine_ladspa.so"
    success "Plugin installed: $HOME/.local/lib/ladspa/libmaxine_ladspa.so"
}

# ── Main ────────────────────────────────────────────────────────────────
main() {
    echo ""
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${BLUE}  broadcast — NVIDIA Maxine SDK Installer${NC}"
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo ""

    install_ngc_cli
    configure_ngc
    local sdk_dir
    sdk_dir=$(download_sdk)
    download_models "$sdk_dir"
    setup_symlink "$sdk_dir"
    update_fish_config
    build_plugin

    echo ""
    echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${GREEN}  NVIDIA Maxine SDK installed!${NC}"
    echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo ""
    echo "  Activate the Maxine backend:"
    echo "    broadcast-ctl set-backend maxine"
    echo "    broadcast-ctl install-config --apply"
    echo ""
    echo "  Or change the Backend dropdown in broadcast-gui."
    echo ""
    echo "  Reload your shell to pick up the new LD_LIBRARY_PATH:"
    echo "    exec fish"
    echo ""
}

main "$@"
