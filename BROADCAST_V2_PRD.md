# Broadcast v2 — PRD & Upgrade Plan

## PRD: Broadcast v2.0 — Robust Noise Suppression with NVIDIA Maxine Support

### 1. Problem Statement

Broadcast is an AI-powered per-application noise suppression tool for PipeWire on Linux. It currently
uses DeepFilterNet as its sole noise suppression backend. Several issues have been identified:

**Quality Issues:**
- DeepFilterNet does not adequately suppress mechanical keyboard clicks — the primary use case
- CPU-only inference limits model quality; the system has an RTX 5080 sitting idle
- No alternative backend to compare or fall back to

**Reliability Issues (observed on current system):**
- **No autostart mechanism** — broadcast filter chains load via PipeWire config, but no service
  ensures routes are applied at login. Apps that start before `broadcast-ctl apply` is run get
  routed to the wrong sink.
- **Default source misconfiguration** — the system's default audio source is the Razer Ripsaw HD
  game capture card, NOT the DeepFilter clean mic. Apps picking up the default source get raw,
  unfiltered audio (explains hearing key clicks).
- **Stale config entries** — `app_routes` contains ghosts like `"brave (deleted)"` and `""` (empty string)
- **Filter state not verified on toggle** — `broadcast-ctl toggle` sets attenuation but doesn't
  confirm the filter chains are actually loaded/running
- **WirePlumber fights routing** — WirePlumber's stream-restore may reassign streams to the wrong
  sink after broadcast routes them, especially on app restart
- **GUI silently swallows errors** — route changes use `let _ =` everywhere, user never knows if
  something failed

**UX Issues:**
- No system tray indicator of filter health (only on/off status via Ironbar poll)
- pavucontrol shows all sinks/sources including virtual ones — confusing to configure
- No guidance on which sink should be default and which sources to mute

---

### 2. Current System Inventory

| Component | Detail |
|-----------|--------|
| **OS** | CachyOS (Arch-based) with Niri compositor |
| **GPU** | NVIDIA RTX 5080 (GB203), driver 595.58.03 |
| **Audio Server** | PipeWire 1.6.2 + WirePlumber |
| **Microphone** | Focusrite Scarlett 2i2 3rd Gen (Input 1 = mic) |
| **Capture Card** | Razer Ripsaw HD (game capture, NOT a mic) |
| **Speakers** | Starship/Matisse HD Audio (ALC887-VD analog stereo) |
| **Headphones** | Scarlett 2i2 headphone out |
| **HDMI** | GB203 HDMI (to monitor) |
| **Controller** | DualSense PS5 (has mic + speaker) |
| **Panel Bar** | Ironbar (with broadcast widget polling every 2s) |
| **Noise Suppression** | DeepFilterNet LADSPA v0.5.6 (CPU-only) |
| **Default Sink** | `broadcast_filter_sink` ✅ (correct) |
| **Default Source** | Razer Ripsaw HD ❌ (should be `deepfilter_mic`) |

### 3. Audio Routing — Current vs Desired

**Current (broken):**
```
Scarlett Mic → [not connected to anything useful]
Ripsaw Game Capture → DeepFilter input chain → "Clean Mic" source (SUSPENDED)
                   → Default Source (raw, unfiltered!) → Apps hear key clicks
Apps → broadcast_filter_sink → DeepFilter output → ALC887 Speakers ✅
```

**Desired:**
```
Scarlett Mic Input 1 → DeepFilter/Maxine input chain → "Clean Mic" source → Default Source
Apps → broadcast_filter_sink → DeepFilter/Maxine output → Preferred Speaker ✅
Ripsaw → left alone (game capture only)
```

---

### 4. Proposed Upgrades

#### Epic 1: NVIDIA Maxine Backend (Quality Upgrade)

**Goal:** Add NVIDIA Maxine Audio Effects SDK as an alternative (and superior) noise suppression
backend, leveraging the RTX 5080's Tensor Cores.

**Approach — LADSPA wrapper plugin:**

The cleanest integration path is building a LADSPA plugin that wraps the NVIDIA Maxine AFX C API,
exactly like DeepFilterNet does with `libdeep_filter_ladspa.so`. This means:

- No changes to PipeWire config structure (just swap the plugin path + label)
- Broadcast's existing filter chain architecture works unchanged
- User can choose backend at config time

**Implementation:**

1. **Create `broadcast-maxine-ladspa` crate** (or separate C project):
   - Wrap NVIDIA Audio Effects SDK C API (`NvAFX_*` functions)
   - Implement LADSPA descriptor for mono (mic) and stereo (output) variants
   - Effects to expose: `NVAFX_EFFECT_DENOISER` (noise removal), optionally
     `NVAFX_EFFECT_DEREVERB` (room echo removal)
   - Control ports: intensity/aggressiveness, model selection (48kHz preferred)
   - Build produces `libmaxine_ladspa.so`

2. **Add backend selection to broadcast-core:**
   - New config field: `"backend": "deepfilter" | "maxine"` (default: `"deepfilter"`)
   - PipeWire filter chain config templates for each backend
   - `broadcast-ctl set-backend <name>` command
   - GUI backend selector dropdown

3. **PipeWire config generation:**
   - Broadcast should generate/manage its own filter chain configs in
     `~/.config/pipewire/pipewire.conf.d/` rather than relying on manually-placed dotfiles
   - Template configs for each backend, written on `set-backend` or first run

4. **Dependencies:**
   - NVIDIA CUDA Toolkit (runtime)
   - NVIDIA Audio Effects SDK (download from NVIDIA, or package via AUR)
   - TensorRT (bundled with SDK)

5. **Fallback:**
   - If Maxine SDK not installed or GPU not available, fall back to DeepFilterNet
   - `broadcast-ctl status` should report which backend is active

**Alternatives Considered:**
- **maxine-pipewire (Darudas):** Community project, but it's a standalone PipeWire module, not a
  LADSPA plugin. Would require rearchitecting broadcast's filter chain approach. Better to build
  our own LADSPA wrapper for clean integration.
- **SPA plugin:** More "native" to PipeWire but significantly more complex to build and maintain.
  LADSPA is the pragmatic choice.

---

#### Epic 2: Reliability & Robustness

**Goal:** Broadcast should "just work" from login without manual intervention.

##### 2a. Systemd User Service

Create `broadcast.service` (systemd user unit):
```ini
[Unit]
Description=Broadcast Noise Suppression Router
After=pipewire.service wireplumber.service
Wants=pipewire.service

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStartPre=/bin/sleep 2
ExecStart=/home/londo/.local/bin/broadcast-ctl apply
ExecStop=/home/londo/.local/bin/broadcast-ctl off

[Install]
WantedBy=default.target
```

- Runs `broadcast-ctl apply` after PipeWire is ready
- Ensures saved routes are applied at login
- `broadcast-ctl install-service` command to generate and enable this

##### 2b. WirePlumber Default Source Override

Create a WirePlumber config to pin the default source to the clean mic:
```lua
-- ~/.config/wireplumber/wireplumber.conf.d/50-broadcast-defaults.conf
monitor.alsa.rules = [
  {
    matches = [{ node.name = "deepfilter_mic" }]
    actions = { update-props = { priority.session = 2000 } }
  }
]
```

Or use `wpctl set-default` in the systemd service to force the default source.

##### 2c. Filter Health Verification

- `broadcast-ctl toggle` and `apply` should verify:
  1. Filter chain nodes exist in PipeWire (`filters_loaded()` — already exists)
  2. Filter chains are not SUSPENDED
  3. LADSPA plugin is actually processing (check node state)
  4. Default source is the clean mic
- If unhealthy, attempt recovery (restart filter chain, re-link)
- Report health in `broadcast-ctl status --json` for Ironbar

##### 2d. Config Cleanup

- Sanitize `app_routes` on load: remove empty keys, `"(deleted)"` entries
- Validate node names exist in PipeWire before applying routes
- Warn (don't crash) when preferred devices are missing

##### 2e. Stream Watcher Daemon

Instead of a one-shot service, optionally run a persistent daemon:
- Watch PipeWire for new stream connections (via `pw-mon` or native PipeWire API)
- Automatically route new streams according to saved preferences
- React to WirePlumber re-routing by re-asserting broadcast's routes
- This eliminates the 3-second GUI poll and the race condition with app startup

---

#### Epic 3: Input Chain Fix (Immediate — Critical)

**This is the cause of the key clicks problem.**

The DeepFilter input chain is currently capturing from the **Razer Ripsaw game capture card** — 
not the Scarlett 2i2 microphone. And the system default source is the Ripsaw, not the clean mic.

**Fixes needed:**

1. **PipeWire input filter config** — change `capture.props` to explicitly target the Scarlett:
   ```
   capture.props = {
     node.name    = "capture.deepfilter_mic"
     node.passive = true
     audio.rate   = 48000
     node.target  = "alsa_input.usb-Focusrite_Scarlett_2i2_USB_Y8FHQBT99230EA-00.HiFi__Mic1__source"
   }
   ```

2. **Set default source** to `deepfilter_mic`:
   ```bash
   wpctl set-default <deepfilter_mic_node_id>
   ```

3. **broadcast-core: preferred_input_source** should be the Scarlett, and broadcast should manage
   connecting the filter chain capture to it.

4. **Add `broadcast-ctl fix-routing`** command that verifies and repairs:
   - Input filter connected to correct physical mic
   - Default source is the clean (filtered) mic
   - Output filter connected to correct speaker
   - No feedback loops

---

#### Epic 4: Pavucontrol / Sound Settings Guidance

**Recommended pavucontrol settings for this system:**

##### Output Devices (Playback) tab:
| Sink | Action | Notes |
|------|--------|-------|
| **Broadcast Filter** (default) | Keep as default, unmuted | All apps route here for filtering |
| **Starship/Matisse Analog Stereo** | Unmuted, not default | Physical speakers — filter output goes here |
| **Scarlett 2i2 Headphones** | Unmuted, use when needed | Headphone output |
| **GB203 HDMI** | Mute unless using monitor speakers | HDMI audio to display |
| **Razer Ripsaw Analog Stereo** | **Mute** | This is a capture card output, not speakers |
| **DualSense Controller** | Mute unless gaming | PS5 controller speaker |

##### Input Devices (Recording) tab:
| Source | Action | Notes |
|--------|--------|-------|
| **Clean Mic (DeepFilter)** | **Set as default** | Filtered mic — all apps should use this |
| **Scarlett 2i2 Input 1** | Not default, unmuted | Raw mic — only for DAW/recording |
| **Scarlett 2i2 Input 2** | Mute unless in use | Instrument/line input |
| **Razer Ripsaw HD sources** (both) | Not default | Game capture audio in, not a mic |
| **Starship/Matisse Analog** | **Mute** | Motherboard line-in, probably unused |
| **DualSense Mic** | Mute unless gaming | PS5 controller mic |
| **Monitor sources** (.monitor) | Ignore | Internal loopback, not user-facing |

##### Ironbar volume widget:
The Ironbar `volume` widget controls the **default sink** which is correctly set to
`broadcast_filter_sink`. Scrolling volume up/down on the widget adjusts the filter sink volume.
This is correct behavior — it controls your main listening volume through the filter chain.

---

#### Epic 5: GUI & Status Improvements

1. **Health indicator in status output:**
   ```json
   {
     "active": true,
     "backend": "maxine",
     "health": "ok",
     "input_filter": "running",
     "output_filter": "running", 
     "default_source_correct": true,
     "issues": []
   }
   ```

2. **Ironbar widget enhancement:**
   - Show backend name (DF/Maxine)
   - Color-code: green=healthy, yellow=degraded, red=broken
   - Tooltip with details

3. **GUI improvements:**
   - Backend selector (DeepFilter / Maxine)
   - Health status panel with per-filter state
   - "Fix Routing" button that runs the repair logic
   - Error toasts instead of silent `let _ =`

---

### 5. Priority & Phasing

| Phase | Epic | Impact | Effort |
|-------|------|--------|--------|
| **Phase 1** | Epic 3: Input Chain Fix | 🔴 Critical — fixes key clicks NOW | Small — config change + `wpctl` |
| **Phase 2** | Epic 2: Reliability | 🟠 High — fixes "not always working" | Medium — systemd + daemon |
| **Phase 3** | Epic 4: Pavucontrol guidance | 🟡 Medium — reduces confusion | None — documentation only |
| **Phase 4** | Epic 1: Maxine Backend | 🟢 Enhancement — better quality | Large — new LADSPA plugin |
| **Phase 5** | Epic 5: GUI & Status | 🟢 Enhancement — better UX | Medium |

---

### 6. Immediate Actions (Can Do Right Now)

These don't require code changes to broadcast itself:

1. **Fix the DeepFilter input chain** to capture from Scarlett Mic 1 (not Ripsaw)
2. **Set default source** to `deepfilter_mic` 
3. **Clean up stale app_routes** in config.json
4. **Create systemd user service** for auto-apply at login
5. **Set WirePlumber priority** for the clean mic source

---

### 7. Technical Notes

**NVIDIA Maxine SDK on CachyOS:**
- RTX 5080 with driver 595.58.03 is fully supported
- SDK requires CUDA toolkit + TensorRT (available via AUR or NVIDIA's repos)
- The AFX C API is straightforward: `NvAFX_CreateEffect()` → `NvAFX_SetString()` (model) →
  `NvAFX_Load()` → `NvAFX_Run()` per audio buffer
- 48kHz sample rate matches current PipeWire config
- GPU memory usage: ~200MB for denoiser model

**DeepFilterNet as fallback:**
- Keep DeepFilterNet as the default backend for systems without NVIDIA GPUs
- Both backends should be hot-swappable without restarting PipeWire (swap filter chain configs
  and reload)

---

## Summary of Issues & Fixes

### Root Cause of Key Clicks
The DeepFilter **input chain is capturing from the Razer Ripsaw game capture card**, not your microphone. Apps are using the **raw Ripsaw audio as the default source**, completely bypassing the filter.

### Root Cause of "Not Always Working"
No autostart mechanism. Routes only apply when you manually run `broadcast-ctl apply`. If an app starts before that, it routes to the wrong sink.

### Recommended Pavucontrol Settings (Immediate)
**Input Devices tab:**
- Set **"Clean Mic (DeepFilter)"** as default ← This is the most important fix
- Mute: "Starship/Matisse Analog Stereo" (line-in), "DualSense Mic", Ripsaw sources

**Output Devices tab:**
- Broadcast Filter = default ✅ (already correct)
- Mute: "Razer Ripsaw Analog Stereo", "DualSense Controller", "GB203 HDMI" (unless needed)

### Ironbar Volume Widget
Working correctly — it controls the broadcast_filter_sink volume.
