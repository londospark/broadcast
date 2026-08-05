# Plan: `broadcast-plasma` — a native KDE Plasma app for Broadcast

AI-powered per-application noise suppression for PipeWire, delivered as a native
KDE Plasma application: **StatusNotifierItem tray** (ksni) + **Kirigami QML
window** bridged with **cxx-qt**, reusing `broadcast-core` unchanged.

## Confirmed decisions

- **No Maxine / NVIDIA** — this machine (SteamOS Deck, AMD VanGogh APU) has no
  NVIDIA GPU. DeepFilterNet is the only backend. The `broadcast-maxine-ladspa`
  crate is not shipped by the flake (it still compiles as an empty stub without
  the SDK, so it does not block workspace builds).
- **Rust↔QML bridge: cxx-qt** (KDAB, actively maintained, Qt 6.9 + QML support).
- **Routing: poll + auto-apply** — re-apply saved routes on a ~2s timer, on
  enable, and at login. A `pw-mon` event watcher is a later enhancement.
- **Flake is fully self-contained** — everything is built with `nix develop` /
  `nix build` from `flake.nix`; nothing depends on system toolchains or headers
  (the Deck has no system Rust, no Qt6 dev headers, no cmake/gcc).
- **devShell**: standard modern interface `devShells.default = pkgs.mkShell { … }`
  (nixpkgs has no `pkgs.devShell`; `devShellTools` is low-level groundwork only).
- **EasyEffects** (already installed on the Deck) remains a valid *parallel*
  "global clean mic" layer. Broadcast's differentiator — and this app's purpose —
  is **per-app routing**, tray status, and automation.

## Key environment facts (SteamOS Deck)

| Item | State |
|------|-------|
| Desktop | KDE Plasma 6.4.3, Wayland |
| GPU | AMD VanGogh (no NVIDIA) |
| Nix | 2.34.7, flakes + nix-command enabled |
| Rust | not installed (flake provides) |
| Qt6 dev headers | not installed (flake provides) |
| `libdeep_filter_ladspa.so` | only inside the EasyEffects flatpak sandbox — host PipeWire can't load it (**must be packaged in the flake**) |
| Filter chains / state | not configured yet |

## Test conventions used in every phase

- **Automated (logical):** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test` — following the existing `MockBackend` pattern in `broadcast-core`
  (75 tests, no live PipeWire required).
- **Flake-native:** `nix flake check` runs the suite reproducibly and sandboxed.
- **Runtime (manual, on the Deck):** verified against real PipeWire with
  `pactl` / `pw-dump` / `qdbus` as described per phase.

---

## Phase 1 — Flake scaffold + devShell

**Work:** pinned `nixpkgs` input; `devShells.default` (`rustc cargo clippy
rustfmt`, `gcc cmake pkg-config`, Qt6 + Kirigami/KF6, `gtk4 libadwaita
gtk4-layer-shell` for the existing GUI, `pipewire pulseaudio wireplumber`
for runtime testing); env `QT_PLUGIN_PATH` / `QML_IMPORT_PATH` /
`XDG_DATA_DIRS` / `QT_QPA_PLATFORM` / `LADSPA_PATH`; `checks` output wired to
fmt + clippy + tests.

**Testing:**
- `nix flake check` → fmt, clippy `-D warnings`, and the full existing
  broadcast-core suite (75 tests) on the Nix toolchain; must be green.
- Inside `nix develop`: `cargo build -p broadcast-ctl` succeeds; `which cargo rustc`
  resolves into `/nix/store` (proves no reliance on a system toolchain).
- **Gate:** all exit 0; shell fully self-contained.

## Phase 2 — DeepFilterNet LADSPA plugin package

**Work:** `nix/deepfilter-ladspa.nix` building `-p deep-filter-ladspa` from a
pinned DeepFilterNet source → `libdeep_filter_ladspa.so`; wire `LADSPA_PATH`
into devShell + wrapped app; confirm whether the model is embedded or must be
vendored at build time.

**Testing:**
- Build: `nix build .#deepfilter-ladspa`; `result/lib/ladspa/libdeep_filter_ladspa.so`
  exists, is ELF (`file`), exports `ladspa_descriptor` (`nm -D`).
- Runtime on Deck: `broadcast-ctl install-config --apply` → `broadcast-ctl status`
  shows **"Filters: loaded"**; `pactl list sources` lists the virtual **Clean Mic**;
  `pw-dump` contains `capture.deepfilter_mic`.
- Audio smoke: `broadcast-ctl on`, pin `deepfilter_mic` as default source, record
  ~5s (`pw-record`) while typing — noise visibly suppressed vs raw mic.
- **Gate:** `nix build` + filter chains load on the Deck (fixes the current
  blocker where the plugin only exists inside the EasyEffects flatpak).

## Phase 3 — `broadcast-plasma` skeleton (Kirigami + cxx-qt)

**Work:** new workspace crate; cxx-qt `BroadcastController` + `AppsModel`
(QAbstractListModel); qrc-embedded `main.qml` Kirigami window with master
toggle + status rows.

**Testing:**
- Automated: `cargo test -p broadcast-plasma` (controller logic: toggle flips
  `state.active`, calls `set_filter_active`, persists via `BroadcastState`; error
  paths) using `MockBackend`; `cargo clippy -p broadcast-plasma -- -D warnings`;
  `cargo fmt --check`.
- Bridge/build: `nix build .#broadcast-plasma` must compile the cxx-qt-generated
  C++, link Qt/Kirigami, embed QML via qrc (build success *is* the toolchain test).
- Runtime on Deck (manual): `nix run .#broadcast-plasma` opens the window on
  Wayland; toggling the master switch updates `~/.local/state/broadcast/config.json`
  and the status rows reflect active/bypassed; with filters not yet loaded the
  health row shows degraded state without crashing.
- **Gate:** window renders + state persists + errors surface in the UI.

## Phase 4 — ksni tray + autostart

**Work:** StatusNotifierItem (state icon, menu: Toggle ✓ / Open / Fix Routing /
Quit; left-click opens window); `.desktop` + `~/.config/autostart`; keep the
existing systemd oneshot routing service.

**Testing:**
- Automated: pure-function unit tests for icon mapping (active/off/degraded →
  correct icon name) and menu construction.
- Runtime on Deck (manual): tray icon appears in the Plasma panel; registration
  confirmed via
  `qdbus org.kde.StatusNotifierWatcher /StatusNotifierWatcher org.kde.StatusNotifierWatcher.RegisteredStatusNotifierItems`;
  icon changes on toggle; menu items work; left-click opens window; Quit exits cleanly.
- Autostart: install autostart `.desktop`, **log out/in of the Plasma session** →
  tray appears with no manual launch.
- **Gate:** tray lives in the native Plasma tray and survives session restart.

## Phase 5 — Full GUI parity + auto-apply

**Work:** per-app switch list, device selection, default-route toggle, health/Fix,
live poll + re-apply routes on enable/at login; wireplumber default-source pin.

**Testing:**
- Automated: unit tests for route-apply logic (fake state + `MockBackend` apps:
  correct `AppRoute` saved to `state.app_routes`, `apply_routes` invoked,
  filter-output never loops to the filter sink) and `AppsModel` roles.
- Integration (manual, Deck, real audio): play a tone with `paplay` (or open an
  app); it appears in the list; toggle **Filtered** → `pactl list sink-inputs`
  shows it on `broadcast_filter_sink`; toggle **Direct** → moves to the hardware
  sink; default source pinned (`pactl info`).
- Cross-check: `broadcast-ctl apps` / `broadcast-ctl status --json` agree with the GUI.
- Persistence: set prefs, quit, relaunch → routes re-applied automatically.
- **Gate:** GUI + CLI produce identical routing results.

## Phase 6 — Polish

**Work:** notifications (notify-rust), custom icons, README, optional
home-manager/NixOS module, optional CI flake job.

**Testing:**
- Full suite: `cargo test` (whole workspace), `nix flake check`,
  `nix build .#broadcast-plasma .#broadcast-ctl .#broadcast-gui` all green.
- Notification: force a health issue → Plasma notification appears.
- Desktop integration: `desktop-file-validate broadcast-plasma.desktop`; launch
  from the app launcher; icons render in tray + window.
- Optional CI: add a `nix flake check` job alongside the existing Arch container job.
- **Gate:** everything above green; documented.

---

## Notes

- The flake builds with `--workspace --exclude broadcast-maxine-ladspa` where a
  package is shipped; workspace-wide `cargo build`/`test` still includes the
  (stub) maxine crate, which is harmless without the NVIDIA SDK.
- Removing `broadcast-maxine-ladspa` from workspace members is a proposed
  follow-up cleanup once the Plasma app is the primary UI.
- SteamOS quirk: the wrapped app must not pick up the system's older Qt/KDE —
  the Nix wrapper pins `QT_PLUGIN_PATH` / `QML_IMPORT_PATH` / `XDG_DATA_DIRS` /
  `LADSPA_PATH` to the nix store.
