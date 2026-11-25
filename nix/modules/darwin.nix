# the agent and the stores as launchd daemons, since nix-darwin has modules
# for none of the stores. every program binds to the machine's tailnet
# address alone, never to the lan. the stores run as one hidden user; vector
# and the launchd job reporter run as root, to read every log and every job.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  inherit (lib) mkOption types;
  cfg = config.services.phoebe;
  user = "_phoebe";
  state = "/var/lib/phoebe";
  textfiles = "${state}/textfile";
  log = name: "/var/log/phoebe-${name}.log";

  generated = import ./config.nix {
    inherit lib cfg;
    listen = address;
    dirs = {
      loki = "${state}/loki";
      vector = "${state}/vector";
    };
    logFiles = cfg.logs.files;
    logExclude = [ (log "vector") ];
  };
  inherit (generated) ports isNode;
  inherit (generated.me) address;
  at = port: "${address}:${toString port}";

  # each file is checked by the program that reads it, while the system builds
  checked =
    name: value: tool: check:
    pkgs.runCommand name { nativeBuildInputs = [ tool ]; } ''
      ln -s ${pkgs.writeText name (builtins.toJSON value)} $out
      ${check} $out
    '';
  rules = checked "phoebe-rules.yml" generated.rules pkgs.prometheus.cli "promtool check rules";
  files = {
    prometheus =
      checked "prometheus.yml" (generated.prometheus rules) pkgs.prometheus.cli
        "promtool check config";
    alertmanager =
      checked "alertmanager.yml" generated.alertmanager pkgs.prometheus-alertmanager
        "amtool check-config";
    loki = checked "loki.json" generated.loki pkgs.grafana-loki "loki -verify-config -config.file";
    vector = checked "vector.json" generated.vector pkgs.vector "vector validate --no-environment";
    blackbox =
      checked "blackbox.yml" generated.blackbox pkgs.prometheus-blackbox-exporter
        "blackbox_exporter --config.check --config.file";
  };

  # which of the jobs a fleet service names, or nix-darwin made, failed: its
  # last run exited nonzero and it isn't running again, or it isn't loaded
  declared = lib.mapAttrsToList (_: service: service.unit) (
    lib.filterAttrs (
      _: service: service.machine == cfg.machine && service.unit != null
    ) cfg.fleet.services
  );
  launchdState = pkgs.writeShellScript "phoebe-launchd-state" ''
    set -eu
    tmp=$(/usr/bin/mktemp ${textfiles}/.launchd.XXXXXX)
    {
      echo '# HELP phoebe_launchd_job_failed Whether a launchd job failed and is not running.'
      echo '# TYPE phoebe_launchd_job_failed gauge'
      /bin/launchctl list | /usr/bin/awk -F '\t' -v declared=${lib.escapeShellArg (toString declared)} '
        BEGIN { n = split(declared, d, " "); for (i = 1; i <= n; i++) want[d[i]] = 1 }
        NR > 1 && ($3 ~ /^org\.nixos\./ || ($3 in want)) {
          printf "phoebe_launchd_job_failed{name=\"%s\"} %d\n", $3, ($1 == "-" && $2 != "0")
          seen[$3] = 1
        }
        END { for (label in want) if (!(label in seen)) printf "phoebe_launchd_job_failed{name=\"%s\"} 1\n", label }
      '
    } > "$tmp"
    /bin/chmod 0644 "$tmp"
    /bin/mv "$tmp" ${textfiles}/launchd.prom
  '';

  daemon = name: arguments: extra: {
    command = lib.escapeShellArgs arguments;
    serviceConfig = {
      RunAtLoad = true;
      # also restarts a program that exits because tailscaled hasn't brought
      # its address up yet
      KeepAlive = true;
      StandardOutPath = log name;
      StandardErrorPath = log name;
    }
    // extra;
  };
  store =
    name: arguments:
    daemon name arguments {
      UserName = user;
      GroupName = user;
    };
in
{
  options.services.phoebe = {
    uid = mkOption {
      type = types.int;
      default = 472;
      description = "The uid, and gid, of `${user}`, the user the stores run as.";
    };
    logs.files = mkOption {
      type = types.listOf types.str;
      default = [ "/var/log/*.log" ];
      description = "Globs of the log files Vector reads and sends to every node.";
    };
  };

  config = lib.mkIf cfg.enable (
    lib.mkMerge [
      {
        services.phoebe.generated = generated;
        environment.etc."phoebe/fleet.json".text = builtins.toJSON generated.fleet;

        services.prometheus.exporters.node = {
          enable = true;
          listenAddress = address;
          port = ports.node;
          extraFlags = [ "--collector.textfile.directory=${textfiles}" ];
        };

        system.activationScripts.postActivation.text = ''
          install -d -m 0755 ${state} ${textfiles}
          install -d -m 0700 ${state}/vector
        '';

        launchd.daemons.phoebe-vector = daemon "vector" [
          (lib.getExe pkgs.vector)
          "--config"
          files.vector
        ] { };

        launchd.daemons.phoebe-launchd-state = {
          command = "${launchdState}";
          serviceConfig = {
            RunAtLoad = true;
            StartInterval = 30;
            ProcessType = "Background";
            StandardOutPath = log "launchd-state";
            StandardErrorPath = log "launchd-state";
          };
        };
      }

      (lib.mkIf isNode {
        users.knownUsers = [ user ];
        users.knownGroups = [ user ];
        users.users.${user} = {
          inherit (cfg) uid;
          gid = cfg.uid;
          home = state;
          shell = "/usr/bin/false";
          isHidden = true;
          description = "phoebe";
        };
        users.groups.${user} = {
          gid = cfg.uid;
          description = "phoebe";
        };

        system.activationScripts.postActivation.text = ''
          for dir in prometheus loki alertmanager; do
            install -d -m 0700 -o ${user} -g ${user} ${state}/$dir
          done
          for name in prometheus loki alertmanager blackbox; do
            touch /var/log/phoebe-$name.log
            chown ${user}:${user} /var/log/phoebe-$name.log
          done
        '';

        launchd.daemons.phoebe-prometheus = store "prometheus" (
          [
            (lib.getExe pkgs.prometheus)
            "--config.file=${files.prometheus}"
            "--storage.tsdb.path=${state}/prometheus"
            "--storage.tsdb.retention.time=${cfg.node.retention.metrics}"
            "--web.listen-address=${at ports.prometheus}"
          ]
          ++ lib.optional (
            cfg.node.retention.metricsSize != null
          ) "--storage.tsdb.retention.size=${cfg.node.retention.metricsSize}"
        );

        launchd.daemons.phoebe-alertmanager = store "alertmanager" (
          [
            (lib.getExe' pkgs.prometheus-alertmanager "alertmanager")
            "--config.file=${files.alertmanager}"
            "--storage.path=${state}/alertmanager"
            "--web.listen-address=${at ports.alertmanager}"
            "--cluster.listen-address=${at ports.cluster}"
            "--cluster.advertise-address=${at ports.cluster}"
          ]
          ++ map (peer: "--cluster.peer=${peer}:${toString ports.cluster}") generated.peers
        );

        launchd.daemons.phoebe-loki = store "loki" [
          (lib.getExe' pkgs.grafana-loki "loki")
          "-config.file=${files.loki}"
        ];

        launchd.daemons.phoebe-blackbox = store "blackbox" [
          (lib.getExe pkgs.prometheus-blackbox-exporter)
          "--config.file=${files.blackbox}"
          "--web.listen-address=${at ports.blackbox}"
        ];
      })
    ]
  );
}
