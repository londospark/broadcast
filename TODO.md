# Broadcast — TODO

Phase-based task tracker for the `broadcast-plasma` (KDE Plasma) app.

Legend: `[ ]` pending · `[x]` done · `[~]` in progress

## Phase 1 — Flake scaffold + devShell
- [x] `flake.nix` with pinned `nixpkgs` (nixos-unstable) input
- [x] `nix/devshell.nix` — `devShells.default = mkShell` (rust, Qt6, Kirigami/KF6, gtk4 deps, pipewire tools)
- [x] devShell env: `QT_PLUGIN_PATH`, `QML_IMPORT_PATH`, `XDG_DATA_DIRS`, `QT_QPA_PLATFORM`, `LADSPA_PATH`
- [x] `nix/checks.nix` — fmt + clippy `-D warnings` + tests (`cargo test`)
- [x] `nix flake lock` generates `flake.lock`
- [x] Gate: `nix develop` works; `cargo build -p broadcast-ctl` succeeds inside
- [x] Gate: `nix flake check` green (fmt + clippy + 75 core tests on Nix toolchain)

## Phase 2 — DeepFilterNet LADSPA plugin package
- [x] `nix/deepfilter-ladspa.nix` — build `-p deep-filter-ladspa` → `libdeep_filter_ladspa.so`
- [x] Wire `LADSPA_PATH` into devShell + wrapped apps
- [x] Confirm model embedding / vendor model if needed (embedded via `include_bytes!`; `default-model` + `default-model-ll` both baked in)
- [x] Gate: `nix build .#deepfilter-ladspa` + `file`/`nm` checks (ELF, exports `ladspa_descriptor`)
- [x] Gate (Deck): `broadcast-ctl install-config --apply` → `status` shows Filters: loaded; Clean Mic source in `pactl`
- [x] Gate (Deck): audio smoke test — record while typing, noise suppressed (**25 dB** suppression measured; raw −49.6 dBFS → filtered −74.6 dBFS)

## Phase 2 — known issues
- [ ] **6-core cap in nix builds needs verification**: `env.NIX_BUILD_CORES` is **ignored** — Nix's builder force-sets it from the machine core count, and `cargoBuildHook` passes `-j $NIX_BUILD_CORES`. Workaround in place: explicit `-j 6` in the overridden `buildPhase` (`deepfilter-ladspa.nix`) and `checkPhase` (`checks.nix`). Confirm no build saturates all 8 cores; consider a cleaner mechanism (e.g. patching the hook or a wrapper). Interactive builds in `nix develop` are covered by `CARGO_BUILD_JOBS=6`.
- [ ] **`broadcast_filter_sink` not enumerated by `pactl` while suspended** — node exists in `pw-dump`/`wpctl` and is linked (49→70 HDMI), but doesn't appear in `pactl list sinks` until active. Likely breaks per-app "filtered" routing lookups in Phase 5; may need to fall back to `pw-dump`-based sink-index lookup or ensure the sink is active.
- [ ] **Input chain mic feed**: on this Deck WirePlumber linked the real internal mic → `capture.deepfilter_mic` correctly (link 73→47). If the default source is changed to `deepfilter_mic` before the link forms there is a feedback-loop risk (PRD Epic 3). Worth pinning `capture.props.node.target` in the generated config later.

## Phase 3 — `broadcast-plasma` skeleton (Kirigami + cxx-qt)
- [ ] New workspace member `broadcast-plasma` (Cargo.toml, build.rs)
- [ ] cxx-qt `BroadcastController` QObject (active/health/defaultRoute, slots)
- [ ] `AppsModel` QAbstractListModel
- [ ] qrc-embedded `ui/main.qml` — Kirigami window, master toggle, status rows
- [ ] `nix/broadcast-plasma.nix` package (wrapped, qml/icons/desktop)
- [ ] Gate: `nix build .#broadcast-plasma` compiles bridge C++ + links Qt/Kirigami
- [ ] Gate (Deck): `nix run .#broadcast-plasma` opens window; toggle persists state

## Phase 4 — ksni tray + autostart
- [ ] `src/tray.rs` — StatusNotifierItem (state icon, menu, left-click opens window)
- [ ] Icon mapping + menu unit tests
- [ ] `.desktop` + `~/.config/autostart` install; keep systemd oneshot service
- [ ] Gate (Deck): tray in Plasma panel, `qdbus` registration check, toggle/menu work
- [ ] Gate (Deck): survives session restart via autostart

## Phase 5 — Full GUI parity + auto-apply
- [ ] Per-app switch list wired to `routing::list_apps` + `state.route_for`
- [ ] Output/input device selection
- [ ] Default-route toggle
- [ ] Health row + Fix button
- [ ] Live poll (~2s) + re-apply routes on enable / at login
- [ ] Wireplumber default-source pin
- [ ] Unit tests: route-apply logic, AppsModel roles
- [ ] Gate (Deck): integration test with `paplay` — Filtered → filter sink, Direct → hw sink; CLI/GUI agree; prefs persist + auto-apply

## Phase 6 — Polish
- [ ] Notifications (notify-rust) on toggle/health issues
- [ ] Custom tray/window icons
- [ ] README update
- [ ] Optional home-manager/NixOS module
- [ ] Optional CI `nix flake check` job
- [ ] Full suite green: `cargo test`, `nix flake check`, all `nix build`s

## Open / deferred
- [ ] Remove `broadcast-maxine-ladspa` from workspace members (cleanup)
- [ ] `pw-mon` event-driven stream watcher (replaces polling)
