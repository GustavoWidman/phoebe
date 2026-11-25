# evaluates a nix-darwin system running phoebe and checks what it installs,
# without building the system
{
  pkgs,
  self,
  nix-darwin,
}:
let
  darwin = nix-darwin.lib.darwinSystem {
    modules = [
      self.darwinModules.default
      {
        nixpkgs.hostPlatform = pkgs.stdenv.hostPlatform.system;
        system.stateVersion = 6;
        networking.hostName = "altar";
        services.phoebe.enable = true;
      }
    ];
  };
  inherit (darwin) config;
  facts = builtins.toJSON {
    # forces the whole system, every daemon's plist and /etc included
    system = config.system.build.toplevel.drvPath;
    packages = map (package: package.name) config.environment.systemPackages;
    phoebe = config.services.phoebe.package.name;
  };
in
pkgs.runCommandLocal "phoebe-darwin-module"
  {
    facts = builtins.unsafeDiscardStringContext facts;
    nativeBuildInputs = [ pkgs.jq ];
  }
  ''
    jq -e '.phoebe as $phoebe | .packages | index($phoebe) != null' <<< "$facts"
    touch $out
  ''
