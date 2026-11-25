# `phoebe web` as a launchd daemon of the stores' own hidden user, which must
# be able to read the files it is given, logging to the file vector ships to
# every node's loki
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.phoebe;
  web = cfg.web;
  user = "_phoebe";
  state = "/var/lib/phoebe-web";
  log = "/var/log/phoebe-web.log";
  path = lib.concatStringsSep ":" (
    lib.optional (web.tailscalePackage != null) "${web.tailscalePackage}/bin"
    ++ [
      "/run/current-system/sw/bin"
      "/usr/bin"
      "/bin"
      "/usr/sbin"
      "/sbin"
    ]
  );
in
{
  config = lib.mkIf (cfg.enable && web.enable) {
    # the origin socket, cloudflared's config and the fetched certificate,
    # and a log the daemon's user may write
    system.activationScripts.postActivation.text = ''
      install -d -m 0700 -o ${user} -g ${user} ${state}
      touch ${log}
      chown ${user}:${user} ${log}
    '';

    launchd.daemons.phoebe-web = {
      command = lib.escapeShellArgs web.command;
      serviceConfig = {
        RunAtLoad = true;
        KeepAlive = true;
        UserName = user;
        GroupName = user;
        # cloudflared stays in the process group, so it dies with phoebe web
        AbandonProcessGroup = false;
        StandardOutPath = log;
        StandardErrorPath = log;
        EnvironmentVariables.PATH = path;
      };
    };

    launchd.daemons.phoebe-web-cert = lib.mkIf (web.signin.certFile == null) {
      command = "${import ./tailscale-cert.nix {
        inherit lib pkgs;
        tailscale =
          if web.tailscalePackage == null then
            "$(command -v tailscale || echo /Applications/Tailscale.app/Contents/MacOS/Tailscale)"
          else
            lib.getExe web.tailscalePackage;
        owner = user;
        dir = state;
      }}";
      serviceConfig = {
        RunAtLoad = true;
        StartInterval = 3600;
        StandardOutPath = log;
        StandardErrorPath = log;
        EnvironmentVariables.PATH = path;
      };
    };
  };
}
