{
  lib,
  craneLib,
}:
let
  root = ../.;

  sources = craneLib.fileset.commonCargoSources root;

  args = {
    src = lib.fileset.toSource {
      inherit root;
      fileset = sources;
    };
    strictDeps = true;
  };

  cargoVendorDir = craneLib.vendorCargoDeps args;

  locked = builtins.readFile ../Cargo.lock;
  workspace = builtins.filter (package: !(package ? source)) (builtins.fromTOML locked).package;
  entry = name: version: ''
    name = "${name}"
    version = "${version}"
  '';
  unversionedLock =
    let
      versioned = map (package: entry package.name package.version) workspace;
      unversioned = map (package: entry package.name "0.0.0") workspace;
    in
    assert lib.assertMsg (lib.all (lib.flip lib.hasInfix locked) versioned)
      "Cargo.lock lays out phoebe's own entry differently than expected";
    builtins.toFile "Cargo.lock" (builtins.replaceStrings versioned unversioned locked);

  # its name, version and vendored sources are given, so crane never reads
  # this generated source while evaluating
  cargoArtifacts = craneLib.buildDepsOnly (
    removeAttrs args [ "src" ]
    // {
      inherit cargoVendorDir;
      pname = "phoebe";
      version = "0.0.0";
      dummySrc = craneLib.mkDummySrc (
        args
        // {
          src = lib.fileset.toSource {
            inherit root;
            fileset = lib.fileset.difference sources ../Cargo.lock;
          };
          cargoLock = unversionedLock;
          cleanCargoTomlFilter =
            path:
            craneLib.filters.cargoTomlDefault path
            &&
              path != [
                "package"
                "version"
              ];
        }
      );
    }
  );
in
craneLib.buildPackage (
  args
  // {
    inherit cargoArtifacts;

    doCheck = false;

    passthru.checks = {
      test = craneLib.cargoTest (args // { inherit cargoArtifacts; });
      clippy = craneLib.cargoClippy (
        args
        // {
          inherit cargoArtifacts;
          cargoClippyExtraArgs = "--all-targets -- --deny warnings";
        }
      );
    };

    meta = {
      description = "Observability for a Nix fleet: every machine and service, their metrics, logs and alerts";
      homepage = "https://github.com/GustavoWidman/phoebe";
      license = lib.licenses.mit;
      mainProgram = "phoebe";
    };
  }
)
