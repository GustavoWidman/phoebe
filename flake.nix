{
  description = "observability for a nix fleet: every machine and service, their metrics, logs and alerts";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    flake-parts.url = "github:hercules-ci/flake-parts";
    devshell.url = "github:numtide/devshell";
    bun2nix = {
      url = "github:baileyluTCD/bun2nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
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
      bun2nix,
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
            system,
            ...
          }:
          let
            # nixpkgs' stable rust, which cache.nixos.org serves ready to run, for
            # builds, checks and the devshell alike
            craneLib = crane.mkLib pkgs;
            phoebe = pkgs.callPackage ./nix/package.nix { inherit craneLib; };
            bunLib = bun2nix.packages.${system}.default;
            phoebe-web = pkgs.callPackage ./nix/web.nix {
              bun2nix = bunLib;
              inherit (phoebe) version;
            };
          in
          {
            packages = {
              default = phoebe;
              inherit phoebe-web;
            };

            checks = {
              inherit phoebe phoebe-web;
            }
            // phoebe.passthru.checks
            // lib.mapAttrs' (name: lib.nameValuePair "phoebe-web-${name}") phoebe-web.passthru.checks
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
                bun
              ];
              env = [
                {
                  name = "RUST_SRC_PATH";
                  value = "${pkgs.rustPlatform.rustLibSrc}";
                }
                # the browsers of the Playwright release web/package.json pins
                {
                  name = "PLAYWRIGHT_BROWSERS_PATH";
                  value = "${pkgs.playwright-driver.browsers}";
                }
                {
                  name = "PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS";
                  value = "1";
                }
              ];

              commands = [
                {
                  name = "e2e";
                  help = "build phoebe and the web app, then run the end-to-end tests on a phone and a desktop";
                  command = ''
                    set -eu
                    cd "$PRJ_ROOT"
                    cargo build --release
                    cd web
                    bun install --frozen-lockfile
                    bun run build
                    bunx playwright test "$@"
                  '';
                }
                {
                  name = "shots";
                  help = "build phoebe and the web app, then photograph every screen and state into web/shots";
                  command = ''
                    set -eu
                    cd "$PRJ_ROOT"
                    cargo build --release
                    cd web
                    bun install --frozen-lockfile
                    bun run build
                    bun scripts/shots.ts "$@"
                  '';
                }
                {
                  name = "lock-web";
                  help = "regenerate web/.bun.nix from web/bun.lock";
                  # bun2nix refuses bun.lock's lockfileVersion 2, although the package layout
                  # it reads is the same as in version 1
                  command = ''
                    set -eu
                    cd "$PRJ_ROOT/web"
                    lock=$(mktemp)
                    trap 'rm -f "$lock"' EXIT
                    sed 's/"lockfileVersion": 2,/"lockfileVersion": 1,/' bun.lock > "$lock"
                    ${lib.getExe bunLib} --lock-file "$lock" --output-file .bun.nix
                  '';
                }
              ];

              motd = "";
            };
          };
      }
    );
}
