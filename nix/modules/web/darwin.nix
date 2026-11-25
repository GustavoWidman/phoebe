# `phoebe web` as a launchd daemon of the stores' own hidden user, logging
# to the file vector ships to every node's loki
{
  config,
  lib,
  ...
}:
let
  cfg = config.services.phoebe;
  web = cfg.web;
  log = "/var/log/phoebe-web.log";
in
{
  config = lib.mkIf (cfg.enable && web.enable) {
    system.activationScripts.postActivation.text = ''
      touch ${log}
      chown _phoebe:_phoebe ${log}
    '';

    launchd.daemons.phoebe-web = {
      command = lib.escapeShellArgs web.command;
      serviceConfig = {
        RunAtLoad = true;
        KeepAlive = true;
        UserName = "_phoebe";
        GroupName = "_phoebe";
        StandardOutPath = log;
        StandardErrorPath = log;
      };
    };
  };
}
