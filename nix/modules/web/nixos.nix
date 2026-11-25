# `phoebe web` as a systemd service of its own unprivileged user, sandboxed
# with the cloudflared and tailscale CLI it runs
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.phoebe;
  web = cfg.web;
  user = "phoebe-web";
  state = "/var/lib/phoebe-web";
  tailscale = lib.optional config.services.tailscale.enable "tailscaled.service";
  fetchCert = web.signin.certFile == null;

  # a credential systemd cannot find in /run/credstore is left out, where a
  # missing file named by its path fails the start, and the switch with it.
  # phoebe web exiting at once would fail it too, so it waits a while first,
  # and systemd starts it again with whatever exists by then.
  waits = lib.mapAttrsToList (name: file: ''
    if [ ! -e "$CREDENTIALS_DIRECTORY/${name}" ]; then
      echo "${file} does not exist; trying again in a minute" >&2
      sleep 60
      exit 1
    fi
  '') web.secrets;
  start = pkgs.writeShellScript "phoebe-web-start" ''
    ${lib.concatStrings waits}
    exec ${lib.escapeShellArgs web.command}
  '';
in
{
  config = lib.mkIf (cfg.enable && web.enable) {
    users.users.${user} = {
      isSystemUser = true;
      group = user;
      description = "phoebe web";
    };
    users.groups.${user} = { };

    systemd.tmpfiles.rules = lib.mapAttrsToList (
      name: file: "L+ /run/credstore/${name} - - - - ${file}"
    ) web.secrets;

    systemd.services.phoebe-web = {
      description = "phoebe web, the fleet's app";
      wantedBy = [ "multi-user.target" ];
      wants = [ "network-online.target" ] ++ tailscale ++ lib.optional fetchCert "phoebe-web-cert.service";
      after = [
        "network-online.target"
        "prometheus.service"
        "loki.service"
        "alertmanager.service"
      ]
      ++ tailscale;
      restartTriggers = [ config.environment.etc."phoebe/fleet.json".source ];
      path = lib.optional (web.tailscalePackage != null) web.tailscalePackage;

      # a missing secret keeps it retrying for as long as it takes
      startLimitIntervalSec = 0;

      serviceConfig = {
        ExecStart = start;
        Restart = "always";
        RestartSec = 5;

        User = user;
        Group = user;
        LoadCredential = lib.attrNames web.secrets;
        # the origin socket and cloudflared's config
        RuntimeDirectory = "phoebe-web";
        RuntimeDirectoryMode = "0700";
        # the fetched certificate
        StateDirectory = "phoebe-web";
        StateDirectoryMode = "0700";
        UMask = "0077";

        # cloudflared and the tailscale CLI run inside this sandbox too
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
        # go raises its open file limit at start, and lives with being refused
        SystemCallErrorNumber = "EPERM";
      };
    };

    systemd.services.phoebe-web-cert = lib.mkIf fetchCert {
      description = "phoebe web's tailnet certificate from tailscaled";
      wants = [ "network-online.target" ] ++ tailscale;
      after = [ "network-online.target" ] ++ tailscale;
      path = [ "/run/current-system/sw" ];
      serviceConfig = {
        Type = "oneshot";
        ExecStart = import ./tailscale-cert.nix {
          inherit lib pkgs;
          tailscale = if web.tailscalePackage == null then "tailscale" else lib.getExe web.tailscalePackage;
          owner = user;
          dir = state;
        };
      };
    };

    systemd.timers.phoebe-web-cert = lib.mkIf fetchCert {
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnCalendar = "hourly";
        Persistent = true;
      };
    };

    networking.firewall.interfaces.${cfg.interface}.allowedTCPPorts = [ web.signin.port ];
  };
}
