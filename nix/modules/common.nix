{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  inherit (lib) mkOption types;
  cfg = config.services.phoebe;
in
{
  options.services.phoebe = {
    enable = lib.mkEnableOption "phoebe on this machine";

    package = mkOption {
      type = types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "phoebe.packages.\${system}.default";
      description = "The phoebe package to install.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
  };
}
