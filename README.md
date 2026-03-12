# Broadcast

AI-powered per-application noise suppression for PipeWire on Linux.

Broadcast routes your audio streams through [DeepFilterNet](https://github.com/Rikorose/DeepFilterNet) to remove typing, keyboard, and background noise — with per-app control over which streams get filtered.

Think of it as NVIDIA Broadcast for Linux, but open source and running on any hardware.

## Features

- **Per-app routing** — choose which apps get noise filtering (e.g., filter browser audio but leave Spotify clean)
- **Mic input filtering** — remove typing/background noise from your microphone
- **Output filtering** — remove noise from audio you're listening to (YouTube, calls)
- **CLI tool** (`broadcast-ctl`) — toggle, status, per-app routing from the terminal
- **GTK4 GUI** (`broadcast-gui`) — visual per-app toggle switches with live stream list
- **PipeWire native** — uses filter chains, no JACK bridge needed
- **Configurable** — node names are user-configurable for custom filter chain setups

## Screenshots

_Coming soon_

## Requirements

- **PipeWire** (with `pactl` / PulseAudio compatibility)
- **DeepFilterNet LADSPA plugin** — provides the AI noise suppression
  - Arch/CachyOS: `paru -S libdeep_filter_ladspa-git`
- **GTK4 + Libadwaita** (for the GUI)
- **gtk4-layer-shell** (for the `--menu` popup mode — already present on any system running Ironbar)
- PipeWire filter chain configs (see [examples/](examples/))

## Installation

### Arch Linux / CachyOS / Manjaro (AUR)

```sh
paru -S broadcast-bin   # pre-built binaries
# or
paru -S broadcast-git   # build from source
```

### Ubuntu / Debian / Mint / Pop!_OS (.deb)

Download the `.deb` from the [latest release](https://github.com/londospark/broadcast/releases/latest):

```sh
# Install both packages
sudo dpkg -i broadcast-ctl_*.deb broadcast-gui_*.deb
sudo apt-get install -f  # resolve any missing dependencies
```

### Fedora / openSUSE (.rpm)

Download the `.rpm` from the [latest release](https://github.com/londospark/broadcast/releases/latest):

```sh
sudo rpm -i broadcast-ctl-*.rpm broadcast-gui-*.rpm
```

### From GitHub Releases (any distro)

```sh
gh release download --repo londospark/broadcast -p 'broadcast-ctl' -p 'broadcast-gui' --dir ~/.local/bin/
chmod +x ~/.local/bin/broadcast-ctl ~/.local/bin/broadcast-gui
```

### From source

```sh
git clone https://github.com/londospark/broadcast.git
cd broadcast
cargo build --release
cp target/release/broadcast-ctl target/release/broadcast-gui ~/.local/bin/
```

## Setup

### 1. Install DeepFilterNet LADSPA plugin

```sh
# Arch/CachyOS
paru -S libdeep_filter_ladspa-git ladspa
```

### 2. Install PipeWire filter chain configs

Copy the example configs to your PipeWire config directory:

```sh
mkdir -p ~/.config/pipewire/pipewire.conf.d/
cp examples/50-deepfilter-input.conf ~/.config/pipewire/pipewire.conf.d/
cp examples/50-deepfilter-output.conf ~/.config/pipewire/pipewire.conf.d/
systemctl --user restart pipewire
```

### 3. Verify filter chains loaded

```sh
broadcast-ctl status
# Should show: Filters: loaded
```

## Usage

### CLI

```sh
broadcast-ctl toggle              # Toggle output noise suppression on/off
broadcast-ctl status              # Show current state
broadcast-ctl status --ironbar    # Compact status for bar widgets
broadcast-ctl status --json       # JSON output
broadcast-ctl apps                # List running audio streams
broadcast-ctl route brave filtered  # Route Brave through the filter
broadcast-ctl route spotify direct  # Keep Spotify unfiltered
broadcast-ctl apply               # Re-apply saved routing preferences
```

### GUI

```sh
broadcast-gui           # open as a normal application window
broadcast-gui --menu    # open as a popup/flyout (no decorations, closes on focus loss)
```

### Desktop integration

Broadcast works well with status bars. Example for [Ironbar](https://github.com/JakeStanger/ironbar):

```toml
[[end]]
type = "script"
name = "broadcast"
mode = "poll"
cmd = "broadcast-ctl status --ironbar"
interval = 2000
on_click_left = "broadcast-ctl toggle"
on_click_right = "broadcast-gui --menu"
```

Passing `--menu` opens the GUI as an undecorated popup that closes automatically when
it loses focus, giving a flyout-style experience from the bar.

## Configuration

State is persisted at `~/.local/state/broadcast/config.json`. You can configure:

- **`master`** — global enable/disable
- **`output_filter`** — whether output routing is active
- **`default_route`** — `"filtered"` or `"direct"` for new audio streams
- **`app_routes`** — per-app routing preferences (persisted across restarts)
- **`nodes`** — PipeWire node names for your filter chains:
  ```json
  {
    "nodes": {
      "input_capture": "capture.deepfilter_mic",
      "output_sink": "broadcast_filter_sink"
    }
  }
  ```

## How it works

1. PipeWire filter chains load the DeepFilterNet LADSPA plugin
2. A virtual "Clean Mic" source captures from your real mic through DeepFilterNet
3. A virtual "Broadcast Filter" sink processes output audio through DeepFilterNet
4. `broadcast-ctl` / `broadcast-gui` move app audio streams between the filter sink and real speakers using `pactl`

```
Mic Input:    Real Mic → DeepFilterNet (mono) → "Clean Mic" virtual source
App Output:   App → "Broadcast Filter" sink → DeepFilterNet (stereo) → Speakers
```

## License

GPL-3.0-or-later — see [LICENSE](LICENSE).
