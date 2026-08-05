# Development shell for Broadcast (KDE Plasma edition).
#
#   nix develop
#
# Provides the full Rust + Qt6 + Kirigami toolchain plus PipeWire tooling,
# with the runtime search paths pre-wired so `cargo run` works from the shell.
{ pkgs }:

let
  deps = import ./deps.nix { inherit pkgs; };
  inherit (pkgs) lib;

  # The DeepFilterNet LADSPA plugin, built by the same expression used for
  # `packages.deepfilter-ladspa`, so the shell runs with a working LADSPA_PATH.
  deepfilterLadspa = pkgs.callPackage ./deepfilter-ladspa.nix { };

  # Where QML modules live for the packages that ship them.
  qmlPath = lib.concatMapStringsSep ":" (p: "${p}/lib/qt6/qml") deps.qml;
  # Share dirs for icons, translations, and QML module fallback.
  sharePath = lib.concatMapStringsSep ":" (p: "${p}/share") (deps.qt ++ deps.kde);
in
pkgs.mkShell {
  packages = deps.all ++ [ deepfilterLadspa ];

  env = {
    # Qt runtime resolution — never fall back to the system's older Qt/KDE.
    QT_PLUGIN_PATH = with pkgs.qt6; "${qtbase}/${qtbase.qtPluginPrefix}";
    QML_IMPORT_PATH = qmlPath;
    XDG_DATA_DIRS = sharePath;
    QT_QPA_PLATFORM = "wayland";
    # Make the DeepFilterNet plugin visible to PipeWire filter chains.
    LADSPA_PATH = "${deepfilterLadspa}/lib/ladspa";
    # Keep 2 cores free (real-time audio + desktop) during interactive builds.
    CARGO_BUILD_JOBS = "6";
  };

  shellHook = ''
    echo "Broadcast dev shell — nix develop"
    echo "  rustc    : $(rustc --version)"
    echo "  Qt6      : $(pkg-config --modversion Qt6Core 2>/dev/null || echo n/a)"
    echo "  Kirigami : ${lib.optionalString (pkgs ? kdePackages) (lib.getVersion pkgs.kdePackages.kirigami)}"
    echo "  build    : cargo build -p broadcast-ctl   (or: cargo build --workspace)"
  '';
}
