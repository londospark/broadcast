# Flake checks: `nix flake check`
#
# Runs the same gates as CI (fmt, clippy -D warnings, cargo test) but on the
# Nix toolchain and sandboxed. Cargo dependencies are vendored from the
# workspace Cargo.lock via `buildRustPackage`'s `cargoLock` support (the fetch
# is a fixed-output derivation, so no network is needed during the build).
{ pkgs, self }:

let
  deps = import ./deps.nix { inherit pkgs; };
  inherit (pkgs) lib;

  # Copy the flake source into a writable workdir (store paths are read-only).
  copySource = ''
    cp -r "$src" work
    chmod -R u+w work
    cd work
  '';
in
{
  # Formatting check — no dependencies required.
  fmt = pkgs.stdenv.mkDerivation {
    pname = "broadcast-fmt-check";
    version = "0.4.1";
    src = self;
    dontUnpack = true;
    nativeBuildInputs = [ pkgs.cargo pkgs.rustfmt ];
    buildPhase = ''
      ${copySource}
      echo "--- cargo fmt --check ---"
      cargo fmt --check
      touch "$out"
    '';
    installPhase = "touch $out";
  };

  # Clippy + full workspace test suite, with deps vendored from Cargo.lock.
  tests = pkgs.rustPlatform.buildRustPackage {
    pname = "broadcast-checks";
    version = "0.4.1";
    src = self;
    cargoLock = {
      lockFile = ../Cargo.lock;
    };
    nativeBuildInputs = deps.rust ++ deps.c;
    buildInputs = deps.qt ++ deps.kde ++ deps.gtk;
    # qtbase in buildInputs wants app wrapping; this is just a test runner.
    dontWrapQtApps = true;
    # We only care about clippy + tests, not a separate release build.
    dontCargoBuild = true;
    env = {
      # Belt-and-suspenders; the explicit `-j 6` below is authoritative.
      CARGO_BUILD_JOBS = "6";
    };
    checkPhase = ''
      echo "--- cargo clippy --all-targets -- -D warnings ---"
      cargo clippy -j 6 --all-targets -- -D warnings
      echo "--- cargo test --workspace ---"
      cargo test -j 6 --workspace
    '';
    installPhase = "touch $out";
  };
}
