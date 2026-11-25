{
  description = "observability for a nix fleet: every machine and service, their metrics, logs and alerts";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    flake-parts.url = "github:hercules-ci/flake-parts";
    devshell.url = "github:numtide/devshell";
    nix-darwin = {
      url = "github:nix-darwin/nix-darwin";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    inputs@{
      flake-parts,
      crane,
      devshell,
      nix-darwin,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } (
      { self, flake-parts-lib, ... }:
      let
        common = flake-parts-lib.importApply ./nix/modules/common.nix { inherit self; };
      in
      {
        # nixpkgs no longer evaluates for x86_64-darwin
        systems = [
          "aarch64-darwin"
          "aarch64-linux"
          "x86_64-linux"
        ];
        imports = [
          devshell.flakeModule
        ];

        flake = {
          nixosModules.default.imports = [
            common
            ./nix/modules/nixos.nix
          ];
          darwinModules.default.imports = [
            common
            ./nix/modules/darwin.nix
          ];
        };

        perSystem =
          {
            pkgs,
            lib,
            ...
          }:
          let
            # nixpkgs' stable rust, which cache.nixos.org serves ready to run, for
            # builds, checks and the devshell alike
            craneLib = crane.mkLib pkgs;
            phoebe = pkgs.callPackage ./nix/package.nix { inherit craneLib; };
          in
          {
            packages.default = phoebe;

            checks = {
              inherit phoebe;
            }
            // phoebe.passthru.checks
            // import ./nix/checks.nix {
              inherit
                pkgs
                lib
                self
                nix-darwin
                ;
            };

            devshells.default = {
              packages = with pkgs; [
                cargo
                rustc
                clippy
                rustfmt
                rust-analyzer
              ];
              env = [
                {
                  name = "RUST_SRC_PATH";
                  value = "${pkgs.rustPlatform.rustLibSrc}";
                }
              ];

              motd = "";
            };
          };
      }
    );
}
