# Builds `libdeep_filter_ladspa.so` — the DeepFilterNet LADSPA plugin used by
# Broadcast's PipeWire filter chains — from a pinned DeepFilterNet source.
#
# The model is embedded at compile time (`include_bytes!` in libDF), so the
# resulting .so is self-contained; no runtime model download is needed.
{ lib, fetchFromGitHub, rustPlatform, pkg-config }:

let
  # Pinned DeepFilterNet revision (main, 2026-08-05).
  rev = "d375b2d8309e0935d165700c91da9de862a99c31";

  src = fetchFromGitHub {
    owner = "Rikorose";
    repo = "DeepFilterNet";
    inherit rev;
    hash = "sha256-JxvK5/PX29o23Pq9cNpTSyuAB7j9HszzDGHC2fOw74Y=";
  };
in
rustPlatform.buildRustPackage {
  pname = "libdeep_filter_ladspa";
  version = "0.5.7";

  inherit src;

  # The upstream workspace includes pyDF/pyDF-data/demo members whose
  # Cargo.lock entries are stale (e.g. `iced`) and which we don't build.
  # Trim the workspace to just libDF + ladspa so resolution succeeds offline.
  cargoPatches = [ ./deepfilter-workspace.patch ];

  cargoLock = {
    lockFile = "${src}/Cargo.lock";
    outputHashes = {
      # Only used by the Python/HDF5 workspace members, not the LADSPA plugin,
      # but importCargoLock needs a hash for every git dependency (keyed by
      # package-name-version).
      # hdf5-rust ships an HDF5 git submodule, so the vendored fetch hash must
      # include submodules.
      "hdf5-src-0.8.1" = "sha256-qWF2mURVblSLPbt4oZSVxIxI/RO3ZNcZdwCdaOTACYs=";
    };
  };

  cargoBuildFlags = [ "-p" "deep-filter-ladspa" ];

  # Just building the plugin is the test; skip the workspace test suite.
  doCheck = false;
  # cargo-auditable needs extra tooling and adds nothing for a LADSPA .so.
  auditable = false;

  nativeBuildInputs = [ pkg-config ];

  env = {
    # The workspace pins LTO=thin for release, which makes this build take a
    # very long time for marginal gains in a LADSPA plugin; disable it.
    CARGO_PROFILE_RELEASE_LTO = "false";
    # Belt-and-suspenders; the explicit `-j 6` in buildPhase is authoritative.
    CARGO_BUILD_JOBS = "6";
  };

  # The cargoBuildHook passes `-j $NIX_BUILD_CORES`, which Nix sets from the
  # machine core count and we can't override through the derivation env. Call
  # cargo directly with `-j 6` so we never saturate all 8 cores.
  buildPhase = ''
    runHook preBuild
    cargo build -j 6 --offline --profile release -p deep-filter-ladspa
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    so="$(find target -name 'libdeep_filter_ladspa.so' -path '*release*' | head -n1)"
    install -Dm755 "$so" "$out/lib/ladspa/libdeep_filter_ladspa.so"
    runHook postInstall
  '';

  meta = with lib; {
    description = "DeepFilterNet LADSPA plugin for real-time noise suppression";
    homepage = "https://github.com/Rikorose/DeepFilterNet";
    license = licenses.mit;
    platforms = platforms.linux;
    maintainers = [ ];
  };
}
