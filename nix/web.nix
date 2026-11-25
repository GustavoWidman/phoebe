{
  lib,
  bun2nix,
  version,
}:
let
  sources = [
    ../web/package.json
    ../web/bun.lock
    ../web/.npmrc
    ../web/tsconfig.json
    ../web/vite.config.ts
    ../web/index.html
    ../web/scripts/compress.ts
    ../web/scripts/service-worker.ts
    ../web/src
    ../web/static
  ];

  # the web sources sit beside the rest of the repository, and every build runs in web/
  source =
    extra:
    lib.fileset.toSource {
      root = ../.;
      fileset = lib.fileset.unions (sources ++ extra);
    };

  common = {
    inherit version;
    bunDeps = bun2nix.fetchBunDeps { bunNix = ../web/.bun.nix; };
    sourceRoot = "source/web";
    dontFixup = true;
  };

  # a check: runs `script` in the web sources, with their dependencies installed
  check =
    name: extra: script:
    bun2nix.mkDerivation (
      common
      // {
        pname = "phoebe-web-${name}";
        src = source extra;
        buildPhase = ''
          runHook preBuild
          ${script}
          runHook postBuild
        '';
        installPhase = "touch $out";
      }
    );

  # the static build, with a .br, .gz and .etag beside each file for the server
  # to send as they are
  web = bun2nix.mkDerivation (
    common
    // {
      pname = "phoebe-web";
      src = source [ ];

      buildPhase = ''
        runHook preBuild
        bun --bun run build
        runHook postBuild
      '';

      installPhase = ''
        runHook preInstall
        mkdir -p $out
        cp -r build/. $out/
        runHook postInstall
      '';

      passthru.checks = {
        # the parser, which reads every message of contract/web.json, what the
        # screens derive from the fleet, and the design gate's own tests
        unit = check "unit" [
          ../contract/web.json
          ../contract/push.json
          ../web/design/system.md
          ../web/scripts/design-gate.ts
          ../web/scripts/design-gate.test.ts
        ] "bun --bun run test";
        # fails when a route's cold path outgrows web/perf-budget.json
        budget = check "budget" [ ../web/scripts/budget.ts ../web/perf-budget.json ] ''
          bun --bun run build
          bun --bun scripts/budget.ts
        '';
        # fails on any value in the built CSS or the markup that web/design/system.md
        # does not list
        design = check "design" [ ../web/design/system.md ../web/scripts/design-gate.ts ] ''
          bun --bun scripts/design-gate.ts ${web}
        '';
        # types and formatting
        check =
          check "check"
            [
              ../web/design
              ../web/scripts
              ../web/tests
              ../web/perf-budget.json
              ../web/playwright.config.ts
              ../web/prettier.config.js
              ../web/.prettierignore
            ]
            ''
              bun --bun run check
              bun --bun run lint
            '';
      };
    }
  );
in
web
