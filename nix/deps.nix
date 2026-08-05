# Shared dependency lists used by the dev shell and the flake checks.
# Keeping them in one place guarantees `nix develop` and `nix flake check`
# test against the exact same environment.
{ pkgs }:

let
  inherit (pkgs) lib;
in
rec {
  # Rust toolchain
  rust = with pkgs; [ rustc cargo rustfmt clippy ];

  # C/C++ toolchain + build tooling (needed by cxx-qt and native crates)
  c = with pkgs; [ pkg-config cmake gcc gdb ];

  # Qt 6 core modules. Note: since Qt 6.9 Qt Quick Controls 2 ship inside
  # qtdeclarative, so there is no separate qtquickcontrols2 package.
  qt = with pkgs; [
    qt6.qtbase
    qt6.qtdeclarative
    qt6.qtsvg
    qt6.qtwayland
    qt6.qttools
  ];

  # KDE / Kirigami
  kde = with pkgs; [
    kdePackages.kirigami
    kdePackages.kirigami-addons
    kdePackages.extra-cmake-modules
  ];

  # Existing GTK4 GUI dependencies (broadcast-gui still builds)
  gtk = with pkgs; [ gtk4 libadwaita gtk4-layer-shell ];

  # PipeWire / PulseAudio tooling for runtime testing
  audio = with pkgs; [ pipewire pulseaudio wireplumber ];

  # Misc shell conveniences
  misc = with pkgs; [ git jq ];

  # Packages that carry QML modules under /lib/qt6/qml — these must appear on
  # QML_IMPORT_PATH for a Kirigami app to load.
  qml = with pkgs; [
    qt6.qtdeclarative
    kdePackages.kirigami
    kdePackages.kirigami-addons
  ];

  # All packages that should be visible in the dev shell environment.
  all = rust ++ c ++ qt ++ kde ++ gtk ++ audio ++ misc;
}
