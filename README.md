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
broadcast-ctl toggle              # Toggle noise suppression on/off
broadcast-ctl on                  # Enable filtering
broadcast-ctl off                 # Disable filtering (passthrough)
broadcast-ctl status              # Show current state
broadcast-ctl status --ironbar    # Compact status for bar widgets
broadcast-ctl status --json       # JSON output
broadcast-ctl apps                # List running audio streams
broadcast-ctl route brave filtered  # Route Brave through the filter
broadcast-ctl route spotify direct  # Keep Spotify unfiltered
broadcast-ctl apply               # Re-apply saved routing preferences
broadcast-ctl devices             # List available audio devices
broadcast-ctl set-device output <name>  # Set preferred output device
broadcast-ctl set-device input <name>   # Set preferred input device
broadcast-ctl --version           # Show version
```

### GUI

```sh
broadcast-gui                                   # normal application window
broadcast-gui --menu                            # popup/flyout (no decorations, closes on focus loss)
broadcast-gui --menu --margin-top 48 --margin-right 10  # custom popup position
```

### Desktop integration

Broadcast works well with Wayland status bars. The `--menu` flag opens the GUI as
an undecorated popup that closes automatically when it loses focus, giving a
flyout-style experience from the bar. You can tune its position with
`--margin-top` and `--margin-right` (defaults: 48 and 10).

#### [Ironbar](https://github.com/JakeStanger/ironbar)

```toml
[[end]]
type = "script"
name = "broadcast"
mode = "poll"
cmd = "broadcast-ctl status --ironbar"
interval = 2000
on_click_left = "broadcast-ctl toggle"
on_click_right = "broadcast-gui --menu --margin-top 48 --margin-right 10"
```

#### [Waybar](https://github.com/Alexays/Waybar)

```jsonc
// In your waybar config
"custom/broadcast": {
    "exec": "broadcast-ctl status --ironbar",
    "interval": 2,
    "on-click": "broadcast-ctl toggle",
    "on-click-right": "broadcast-gui --menu --margin-top 38 --margin-right 10",
    "tooltip": false
}
```

Add `"custom/broadcast"` to your `modules-right` (or whichever side you prefer).

#### [Yambar](https://codeberg.org/dnkl/yambar)

```yaml
- script:
    path: /bin/sh
    args:
      - -c
      - broadcast-ctl status --ironbar
    poll-interval: 2000
    content:
      string:
        text: "{broadcast-ctl status --ironbar}"
        on-click:
          left: broadcast-ctl toggle
          right: broadcast-gui --menu --margin-top 38 --margin-right 10
```

#### [AGS](https://github.com/Aylur/ags) / [Eww](https://github.com/elkowar/eww)

Both AGS and Eww can poll `broadcast-ctl status --json` for structured data and
`broadcast-ctl toggle` for click actions. Example Eww widget:

```yuck
(deflisten broadcast-status :initial "{}"
  `watch -n2 -t broadcast-ctl status --json`)

(defwidget broadcast []
  (button :onclick "broadcast-ctl toggle"
          :onrightclick "broadcast-gui --menu"
    (label :text {broadcast-status.active ? "󰍬" : "󰍭"})))
```

#### Generic (any bar with script support)

Any bar that can poll a command and trigger click actions works. You need:

| Function | Command |
|----------|---------|
| Status text | `broadcast-ctl status --ironbar` |
| JSON status | `broadcast-ctl status --json` |
| Toggle | `broadcast-ctl toggle` |
| Popup GUI | `broadcast-gui --menu` |

## Configuration

State is persisted at `~/.local/state/broadcast/config.json`. You can configure:

- **`active`** — global enable/disable for all filtering
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
- **`preferred_output_sink`** / **`preferred_input_source`** — preferred device by PipeWire node name (`null` = auto-detect)

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
