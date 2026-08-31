#!/usr/bin/env bash
# scripts/build-maxine-runtime-bundle.sh
#
# Downloads the NVIDIA Maxine Audio Effects SDK's runtime files (shared
# libraries + pretrained models) for every desktop RTX generation and
# packages them into two redistributable tarballs, so end users can
# install Maxine support without their own NGC API key — only this build
# needs one, per scripts/install-maxine-sdk.sh's usual CI usage.
#
# Split into a -libs tarball (nvafx/lib + external/cuda/lib — NVIDIA's
# bundled CUDA/TensorRT/cuDNN runtime, architecture-independent, ~1.4GB
# compressed on its own) and a -models tarball (the per-architecture
# .trtpkg files, much smaller). GitHub rejects any single release asset
# over 2GB; a single combined tarball for all 4 architectures comes in
# just over that limit, so it has to be at least two files regardless of
# how small the models themselves are.
#
# Covers Turing through Blackwell (t4, a10, l40, rtx_pro_6000 — the --gpu
# flags whose architectures show up in consumer/workstation RTX cards;
# a100/h100/etc. are datacenter-only and skipped). The NVIDIA Maxine SDK
# EULA's distribution supplement (https://developer.nvidia.com/downloads/maxine-sdk-license)
# permits redistributing any SDK portion other than its audio/video data
# samples, bundled into our own application — see broadcast-maxine-ladspa/README.md.
#
# Usage:
#   NGC_API_KEY="your_key_here" bash scripts/build-maxine-runtime-bundle.sh <out-prefix>
#   (writes <out-prefix>-libs.tar.gz and <out-prefix>-models.tar.gz)

set -euo pipefail

OUT_PREFIX="${1:?usage: build-maxine-runtime-bundle.sh <out-prefix>}"
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
libs_stage=$(mktemp -d)
models_stage=$(mktemp -d)
trap 'rm -rf "$libs_stage" "$models_stage"' EXIT

for rel in nvafx/lib features/denoiser/lib features/dereverb_denoiser/lib external/cuda/lib; do
    if [ -d "$sdk_dir/$rel" ]; then
        mkdir -p "$libs_stage/$(dirname "$rel")"
        cp -a "$sdk_dir/$rel" "$libs_stage/$rel"
    fi
done

for rel in features/denoiser/models features/dereverb_denoiser/models; do
    if [ -d "$sdk_dir/$rel" ]; then
        mkdir -p "$models_stage/$(dirname "$rel")"
        cp -a "$sdk_dir/$rel" "$models_stage/$rel"
    fi
done

found_any_model=0
for f in "$models_stage"/features/*/models/sm_*/*.trtpkg; do
    [ -f "$f" ] && found_any_model=1
done
if [ "$found_any_model" -eq 0 ]; then
    error "No .trtpkg models found in the staged bundle — model download must have failed"
    exit 1
fi

mkdir -p "$(dirname "$OUT_PREFIX")"
tar czf "${OUT_PREFIX}-libs.tar.gz" -C "$libs_stage" .
tar czf "${OUT_PREFIX}-models.tar.gz" -C "$models_stage" .
success "Runtime bundle written:"
info "  ${OUT_PREFIX}-libs.tar.gz ($(du -h "${OUT_PREFIX}-libs.tar.gz" | cut -f1))"
info "  ${OUT_PREFIX}-models.tar.gz ($(du -h "${OUT_PREFIX}-models.tar.gz" | cut -f1))"
info "Architectures bundled:"
find "$models_stage" -type d -name 'sm_*' | sed "s#.*/##" | sort -u >&2
