# broadcast-maxine-ladspa

LADSPA wrapper around the [NVIDIA Maxine Audio Effects SDK](https://developer.nvidia.com/nvidia-audio-effects-sdk) for use as a PipeWire filter chain plugin — a GPU-accelerated drop-in replacement for the DeepFilterNet plugin used by the `broadcast` app.

## Requirements

| Requirement | Install (CachyOS/Arch) |
|-------------|----------------------|
| RTX GPU (20xx+) | — |
| NVIDIA driver ≥ 520 | `pacman -S nvidia` |
| CUDA Toolkit | `pacman -S cuda` |
| LADSPA headers | `pacman -S ladspa` |
| CMake ≥ 3.20 | `pacman -S cmake` |
| NVIDIA Maxine AFX SDK | Download below |

### Get the Maxine SDK

1. Go to https://developer.nvidia.com/nvidia-audio-effects-sdk
2. Accept the licence and download the Linux package
3. Extract to e.g. `~/nvidia-maxine-sdk`

## Build

```bash
cd broadcast-maxine-ladspa
mkdir build && cd build
cmake .. -DNVAFX_SDK=$HOME/nvidia-maxine-sdk -DCMAKE_BUILD_TYPE=Release
make -j$(nproc)
sudo make install   # installs to /usr/local/lib/ladspa/maxine_ladspa.so
```

Or install to user path:
```bash
cmake .. -DNVAFX_SDK=$HOME/nvidia-maxine-sdk \
         -DCMAKE_INSTALL_PREFIX=$HOME/.local \
         -DCMAKE_BUILD_TYPE=Release
make -j$(nproc) && make install
# Add ~/.local/lib/ladspa to LADSPA_PATH:
# export LADSPA_PATH=$HOME/.local/lib/ladspa:/usr/lib/ladspa
```

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

## Model files

By default the plugin looks for model files at `/usr/share/nvafx/models`.
Override with the `NVAFX_MODEL_DIR` environment variable:
```bash
export NVAFX_MODEL_DIR=/path/to/models
```

The model files ship inside the SDK download under `models/`.
Copy them to the expected location:
```bash
sudo mkdir -p /usr/share/nvafx/models
sudo cp ~/nvidia-maxine-sdk/models/* /usr/share/nvafx/models/
```

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
