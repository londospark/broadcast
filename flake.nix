{
  description = "Broadcast — AI-powered per-application noise suppression for PipeWire, with a native KDE Plasma UI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      pkgsFor = system: nixpkgs.legacyPackages.${system};
    in
    {
      devShells = forAllSystems (system: {
        default = import ./nix/devshell.nix { pkgs = pkgsFor system; };
      });

      packages = forAllSystems (system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.callPackage ./nix/deepfilter-ladspa.nix { };
          deepfilter-ladspa = pkgs.callPackage ./nix/deepfilter-ladspa.nix { };
        });

      checks = forAllSystems (system:
        let
          pkgs = pkgsFor system;
        in
        import ./nix/checks.nix { inherit pkgs self; });
    };
}
