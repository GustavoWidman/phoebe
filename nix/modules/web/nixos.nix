# `phoebe web` as a systemd service of its own unprivileged user, in a
# sandbox that leaves it the network and nothing else to write to
{
  config,
  lib,
  ...
}:
let
  cfg = config.services.phoebe;
  web = cfg.web;
  user = "phoebe-web";
in
{
  config = lib.mkIf (cfg.enable && web.enable) {
    users.users.${user} = {
      isSystemUser = true;
      group = user;
      description = "phoebe web";
    };
    users.groups.${user} = { };

    systemd.services.phoebe-web = {
      description = "phoebe web, the fleet's app";
      wantedBy = [ "multi-user.target" ];
      wants = [ "network-online.target" ];
      after = [
        "network-online.target"
        "prometheus.service"
        "loki.service"
        "alertmanager.service"
      ];
      restartTriggers = [ config.environment.etc."phoebe/fleet.json".source ];

      serviceConfig = {
        ExecStart = lib.escapeShellArgs web.command;
        Restart = "always";
        RestartSec = 5;

        User = user;
        Group = user;
        UMask = "0077";

        CapabilityBoundingSet = "";
        DevicePolicy = "closed";
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        NoNewPrivileges = true;
        PrivateDevices = true;
        PrivateTmp = true;
        PrivateUsers = true;
        ProcSubset = "pid";
        ProtectClock = true;
        ProtectControlGroups = true;
        ProtectHome = true;
        ProtectHostname = true;
        ProtectKernelLogs = true;
        ProtectKernelModules = true;
        ProtectKernelTunables = true;
        ProtectProc = "invisible";
        ProtectSystem = "strict";
        RemoveIPC = true;
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
        ];
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [
          "@system-service"
          "~@privileged"
          "~@resources"
        ];
      };
    };
  };
}
