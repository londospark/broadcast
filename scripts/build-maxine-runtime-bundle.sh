#!/usr/bin/env bash
# scripts/build-maxine-runtime-bundle.sh
#
# Downloads the NVIDIA Maxine Audio Effects SDK's runtime files (shared
# libraries + pretrained models) for every desktop RTX generation and
# packages them into a single redistributable tarball, so end users can
# install Maxine support without their own NGC API key — only this build
# needs one, per scripts/install-maxine-sdk.sh's usual CI usage.
#
# Covers Turing through Blackwell (t4, a10, l40, rtx_pro_6000 — the --gpu
# flags whose architectures show up in consumer/workstation RTX cards;
# a100/h100/etc. are datacenter-only and skipped). The NVIDIA Maxine SDK
# EULA's distribution supplement (https://developer.nvidia.com/downloads/maxine-sdk-license)
# permits redistributing any SDK portion other than its audio/video data
# samples, bundled into our own application — see broadcast-maxine-ladspa/README.md.
#
# Usage:
#   NGC_API_KEY="your_key_here" bash scripts/build-maxine-runtime-bundle.sh <output.tar.gz>

set -euo pipefail

OUT_TARBALL="${1:?usage: build-maxine-runtime-bundle.sh <output.tar.gz>}"
export MAXINE_GPU_ARCHES="t4 a10 l40 rtx_pro_6000"
export NVAFX_SDK_INSTALL_DIR="${NVAFX_SDK_INSTALL_DIR:-$HOME/.local/share/nvidia-maxine-sdk}"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=install-maxine-sdk.sh
source "$REPO_ROOT/scripts/install-maxine-sdk.sh"

install_ngc_cli
configure_ngc
sdk_dir=$(download_sdk)
download_models "$sdk_dir"

info "Packaging runtime bundle..."
stage_dir=$(mktemp -d)
trap 'rm -rf "$stage_dir"' EXIT

# Only what the LADSPA plugin needs at runtime (per maxine_ladspa.c's
# search order) — not nvafx/include (dev headers) or the sample apps.
for rel in nvafx/lib features/denoiser/lib features/denoiser/models \
           features/dereverb_denoiser/lib features/dereverb_denoiser/models \
           external/cuda/lib; do
    if [ -d "$sdk_dir/$rel" ]; then
        mkdir -p "$stage_dir/$(dirname "$rel")"
        cp -a "$sdk_dir/$rel" "$stage_dir/$rel"
    fi
done

found_any_model=0
for f in "$stage_dir"/features/*/models/sm_*/*.trtpkg; do
    [ -f "$f" ] && found_any_model=1
done
if [ "$found_any_model" -eq 0 ]; then
    error "No .trtpkg models found in the staged bundle — model download must have failed"
    exit 1
fi

mkdir -p "$(dirname "$OUT_TARBALL")"
tar czf "$OUT_TARBALL" -C "$stage_dir" .
success "Runtime bundle written: $OUT_TARBALL ($(du -h "$OUT_TARBALL" | cut -f1))"
info "Architectures bundled:"
find "$stage_dir" -type d -name 'sm_*' | sed "s#.*/##" | sort -u >&2
