{
  pkgs,
  lib,
  self,
  nix-darwin,
}:
{
  rules = import ./rules-check.nix { inherit pkgs lib self; };
}
// lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
  nixos = pkgs.testers.runNixOSTest (import ./test.nix { inherit self; });
}
// lib.optionalAttrs pkgs.stdenv.hostPlatform.isDarwin {
  darwin-module = import ./darwin-check.nix { inherit pkgs self nix-darwin; };
}
