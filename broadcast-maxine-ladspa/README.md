# broadcast-maxine-ladspa

LADSPA wrapper around the [NVIDIA Maxine Audio Effects SDK](https://developer.nvidia.com/nvidia-audio-effects-sdk) for use as a PipeWire filter chain plugin — a GPU-accelerated drop-in replacement for the DeepFilterNet plugin used by the `broadcast` app.

## Requirements

| Requirement | Install (CachyOS/Arch) |
|-------------|----------------------|
| RTX GPU (20xx+) | — |
| NVIDIA driver ≥ 570 | `pacman -S nvidia-open-dkms nvidia-utils` (or `nvidia-dkms` on non-open kernels) |
| LADSPA headers | `pacman -S ladspa` |
| Rust toolchain | `mise use -g rust@latest` (or `rustup`) |
| A free NGC API key | https://org.ngc.nvidia.com/setup/api-key |

The SDK ships its own bundled CUDA/TensorRT/cuDNN under `external/cuda/`, so a
system-wide CUDA Toolkit install is **not** required just to build or run this
plugin. `cmake` isn't needed either — the crate compiles `src/maxine_ladspa.c`
directly via Cargo's `cc` crate.

## Install (no NGC key needed)

Every [release](https://github.com/londospark/broadcast/releases) bundles a
pre-built Maxine runtime (shared libraries + models) covering desktop RTX
GPUs from Turing through Blackwell — NVIDIA's Maxine SDK license permits
redistributing these as part of our own application (see
[Licensing](#licensing) below). Most users don't need their own NGC account:

```bash
bash scripts/install-maxine-runtime.sh
```

This downloads the bundle and the compiled plugin from the latest release and
installs both. `broadcast-ctl` detects your GPU's architecture automatically
(via `nvidia-smi`) when it writes the PipeWire filter chain config, and the
plugin picks the matching model at load time — no manual GPU selection
needed. If your card predates Turing, or isn't a desktop/workstation RTX part
the bundle covers, use the NGC-key build below instead.

## Build from NVIDIA's SDK directly

Needed only if the bundled runtime above doesn't cover your GPU, or you're
building the plugin itself for development. The SDK is gated behind an NGC
account (free) and can't be scripted without a key.
`scripts/install-maxine-sdk.sh` (at the repo root) handles the rest: it
installs the `ngc` CLI, downloads and extracts the SDK + denoiser models for
your GPU's compute capability, symlinks it to a `current` path, and builds
this crate against it.

```bash
# 1. Get a free NGC API key: https://org.ngc.nvidia.com/setup/api-key
#    (also accept the SDK's license by opening its resource page once while
#    logged in: https://catalog.ngc.nvidia.com/orgs/nvidia/teams/maxine/resources/maxine_linux_audio_effects_sdk)
cd broadcast
NGC_API_KEY="your_key_here" bash scripts/install-maxine-sdk.sh
```

This installs the SDK to `~/.local/share/nvidia-maxine-sdk/current` and the
built plugin to `~/.local/lib/ladspa/libmaxine_ladspa.so`.

### Manual build

If the SDK is already unpacked somewhere (e.g. `~/.local/share/nvidia-maxine-sdk/current`,
with `nvafx/include/nvAudioEffects.h` inside it):

```bash
cd broadcast
NVAFX_SDK=~/.local/share/nvidia-maxine-sdk/current cargo build --release -p broadcast-maxine-ladspa
cp target/release/libbroadcast_maxine_ladspa.so ~/.local/lib/ladspa/libmaxine_ladspa.so
```

`build.rs` also auto-detects the SDK at that default path with no `NVAFX_SDK`
set, so a plain `cargo build --release` picks it up once installed. If the
SDK isn't found, this crate builds an empty stub with a warning rather than
failing the workspace build.

## Enable in broadcast

Once the plugin is installed:
```bash
broadcast-ctl set-backend maxine
broadcast-ctl install-config --apply
```

To switch back to DeepFilterNet (CPU):
```bash
broadcast-ctl set-backend deepfilter
broadcast-ctl install-config --apply
```

Or use the Backend selector in `broadcast-gui` or the Omarchy panel — both
call the same commands.

## Model files

`install-maxine-sdk.sh` downloads the `denoiser-48k` feature package matched
to your GPU automatically; the release-bundled runtime
(`build-maxine-runtime-bundle.sh`) instead fetches all four desktop
architectures (`t4`, `a10`, `l40`, `rtx_pro_6000` — Turing/Ampere/Ada/
Blackwell) into the same tree, each under its own `models/sm_<N>/`
subdirectory. Either way, models live under the SDK's `features/denoiser/`
directory; the plugin discovers them relative to `NVAFX_SDK` at runtime.
When several architectures are present, set `NVAFX_SM` (e.g. `NVAFX_SM=86`)
to pick one explicitly — `broadcast-ctl` sets this automatically from
`nvidia-smi` when it's not already set, so this normally doesn't need to be
done by hand.

## Licensing

This plugin's own code is GPL-3.0-or-later, same as the rest of `broadcast`.
The NVIDIA Maxine SDK components it links against and (optionally) bundles
are proprietary, under [NVIDIA's own SDK license](https://developer.nvidia.com/downloads/maxine-sdk-license) —
its distribution supplement permits redistributing any SDK portion other
than its audio/video data samples, incorporated into our own application,
which is exactly what `build-maxine-runtime-bundle.sh` and
`install-maxine-runtime.sh` do. Powered by NVIDIA Maxine™; see NVIDIA's
[Maxine SDK branding guidelines](https://www.nvidia.com/maxine-sdk-guidelines)
for attribution requirements before using NVIDIA's marks elsewhere.

### Effect and model preference

At runtime the plugin tries, in order, the best effect/model it can find and
actually get the SDK to load:

1. **`dereverb_denoiser`** (noise + room echo removal combined — closer to
   NVIDIA Broadcast's Windows defaults) if that feature package was also
   downloaded (`download_features.sh --effects dereverb_denoiser-48k`).
2. **Denoiser "v2"** model (`denoiser_v2_48k.trtpkg`) — a newer retrain with
   noticeably better suppression of sharp transients (keyboard/mouse
   clicks) than the original.
3. **Denoiser "v1"** (`denoiser_48k.trtpkg`) — the original model, as a
   last resort.

A candidate's model file existing on disk doesn't guarantee the SDK will
actually initialise it (this varies by SDK/GPU generation); the plugin logs
which one it ends up running via `journalctl --user -u pipewire | grep
broadcast-maxine`. Set `NVAFX_EFFECT=denoiser` to skip `dereverb_denoiser`
even when present.

## LADSPA labels

| Label | Description |
|-------|-------------|
| `maxine_denoiser_mono` | Single-channel (mic input filter chain) |
| `maxine_denoiser_stereo` | Two-channel (speaker output filter chain) |

## Ports

| Port | Type | Description |
|------|------|-------------|
| 0: Input L | Audio In | Left/mono input |
| 1: Input R | Audio In | Right input (stereo only) |
| 2: Output L | Audio Out | Left/mono processed output |
| 3: Output R | Audio Out | Right processed output (stereo only) |
| 4: Intensity | Control | Denoising strength 0.0–1.0 (default 1.0) |
