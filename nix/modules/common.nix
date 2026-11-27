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
  inherit (cfg.fleet) machines nodes services;

  machine = types.submodule {
    options = {
      os = mkOption {
        type = types.enum [
          "nixos"
          "darwin"
        ];
        description = "The machine's operating system, which decides how its units are watched.";
      };
      alwaysOn = mkOption {
        type = types.bool;
        example = false;
        description = ''
          Whether the machine is meant to be up all the time. MachineDown pages
          only for these. A desktop that is off at night is `false`, so it shows
          as off and never pages.
        '';
      };
      address = mkOption {
        type = types.str;
        example = "100.64.0.1";
        description = ''
          The machine's tailnet IPv4 address. Every machine reaches every
          other at its address, and on darwin each program binds to it
          alone.
        '';
      };
    };
  };

  probe = types.submodule {
    options = {
      kind = mkOption {
        type = types.enum [
          "http"
          "tcp"
          "dns"
          "smtp"
          "imap"
        ];
        description = ''
          How to probe. `http` expects a 2xx answer, `tcp` a connection, `smtp`
          and `imap` their server's greeting, `dns` an answer to `query`.
        '';
      };
      target = mkOption {
        type = types.str;
        example = "https://example.com/health";
        description = ''
          What to probe: a URL for `http`, a `host:port` for `tcp`, `smtp` and
          `imap`, the server's address for `dns`.
        '';
      };
      tls = mkOption {
        type = types.bool;
        default = false;
        description = ''
          Whether a `tcp`, `smtp` or `imap` probe speaks TLS from the first
          byte, as on ports 465 and 993. An `http` probe takes TLS from its URL.
        '';
      };
      query = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "example.com";
        description = "The name a `dns` probe asks for an `A` record of. Only `dns` probes take it.";
      };
    };
  };

  service = types.submodule {
    options = {
      name = mkOption {
        type = types.nonEmptyStr;
        example = "heimdall";
        description = ''
          The service's name. Two machines may each run a service of one name,
          so a service is its machine and its name together.
        '';
      };
      machine = mkOption {
        type = types.str;
        description = "The machine the service runs on, a name in `fleet.machines`.";
      };
      unit = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "heimdall.service";
        description = ''
          The full name of the systemd unit, or the label of the launchd job,
          that runs the service. UnitFailed pages when it fails. `null` for a
          service watched only through its probes.
        '';
      };
      probes = mkOption {
        type = types.listOf probe;
        default = [ ];
        description = ''
          Checks every node runs against the service. ServiceUnreachable pages
          when at least two nodes see one fail.
        '';
      };
      metrics = mkOption {
        type = types.nullOr (
          types.submodule {
            options = {
              port = mkOption {
                type = types.port;
                description = "The port the service serves Prometheus metrics on, at its machine's address.";
              };
              path = mkOption {
                type = types.str;
                default = "/metrics";
                description = "The path of the metrics.";
              };
            };
          }
        );
        default = null;
        description = "Where every node scrapes the service's own metrics, or `null` when it has none.";
      };
      heartbeat = mkOption {
        type = types.nullOr (
          types.submodule {
            options = {
              metric = mkOption {
                type = types.strMatching "[a-zA-Z_:][a-zA-Z0-9_:]*";
                example = "backup_healthy_timestamp_seconds";
                description = ''
                  A gauge on the service's machine, holding the unix time the
                  service last knew itself healthy, in seconds.
                '';
              };
              within = mkOption {
                type = types.ints.positive;
                example = 3600;
                description = "How many seconds the gauge may fall behind before ServiceStale pages.";
              };
            };
          }
        );
        default = null;
        description = ''
          How a service that serves nothing, a batch job say, reports that it
          is healthy. ServiceStale pages when the gauge falls behind or goes
          missing. `null` for a service with no heartbeat.
        '';
      };
    };
  };

  probesWith = predicate: lib.concatMap (service: lib.filter predicate service.probes) services;

  # each (machine, name) registered more than once
  registeredTwice = lib.filter (group: lib.length group > 1) (
    lib.attrValues (lib.groupBy (service: "${service.machine}/${service.name}") services)
  );
in
{
  options.services.phoebe = {
    enable = lib.mkEnableOption ''
      phoebe on this machine: node exporter and Vector, plus Prometheus,
      Loki, Alertmanager and the blackbox exporter when the machine is one of
      `fleet.nodes`
    '';

    package = mkOption {
      type = types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "phoebe.packages.\${system}.default";
      description = "The phoebe package to install.";
    };

    machine = mkOption {
      type = types.str;
      default = config.networking.hostName;
      defaultText = lib.literalExpression "config.networking.hostName";
      description = "This machine's name in `fleet.machines`.";
    };

    fleet = {
      nodes = mkOption {
        type = types.listOf types.str;
        example = [
          "oracle-1"
          "oracle-2"
        ];
        description = ''
          The machines that store and serve: each runs Prometheus, Loki, an
          Alertmanager of one cluster, and the blackbox exporter, and every
          machine sends its logs to each of them. Any one of them can be down
          while the others keep every metric, log and alert.
        '';
      };

      machines = mkOption {
        type = types.attrsOf machine;
        description = "Every machine in the fleet, by name. Every node scrapes each of them.";
      };

      services = mkOption {
        type = types.listOf service;
        default = [ ];
        example = lib.literalExpression ''
          [
            { name = "norn"; machine = "oracle-1"; unit = "norn.service"; }
            { name = "norn"; machine = "oracle-2"; unit = "norn.service"; }
          ]
        '';
        description = ''
          Every service the fleet runs. A service is its machine and its name
          together: two machines may run a service of one name, one machine
          only one.
        '';
      };
    };

    node.retention = {
      metrics = mkOption {
        type = types.str;
        default = "90d";
        description = "How long each node's Prometheus keeps metrics.";
      };
      metricsSize = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "20GB";
        description = "The most disk each node's Prometheus fills, dropping the oldest metrics past it. `null` for no limit.";
      };
      logs = mkOption {
        type = types.str;
        default = "30d";
        description = "How long each node's Loki keeps logs.";
      };
    };

    textfileDirectories = mkOption {
      type = types.listOf types.str;
      default = [ ];
      example = [ "/var/lib/backup/metrics" ];
      description = ''
        Directories, globs allowed, of `*.prom` files this machine's node
        exporter serves besides its own, where a job on the machine writes the
        gauges of its heartbeat. The node exporter's user must be able to read
        them.
      '';
    };

    logs.bufferSize = mkOption {
      type = types.ints.positive;
      default = 512 * 1024 * 1024;
      defaultText = lib.literalExpression "512 * 1024 * 1024";
      description = ''
        Bytes of logs Vector holds on disk for each node while it can't reach
        that node, at least 268435488. Past it, the newest logs for that node
        are dropped. LogsBacklog fires once a buffer is half full. Vector
        refuses to start when the buffers, one per node, add up to more than
        the disk they live on.
      '';
    };

    generated = mkOption {
      type = types.raw;
      internal = true;
      readOnly = true;
      description = "Every program's configuration as this platform's module generates it, for the checks to read.";
    };

    alerting.webhook = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "http://127.0.0.1:8080/alerts";
      description = ''
        The URL each node's Alertmanager posts alerts to, in Alertmanager's
        webhook format. The cluster sends each notification from one node
        only. `null` sends nothing.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = machines ? ${cfg.machine};
        message = "phoebe: services.phoebe.fleet.machines has no ${cfg.machine}, this machine.";
      }
      {
        assertion =
          (machines.${cfg.machine}.os or null)
          == (if pkgs.stdenv.hostPlatform.isDarwin then "darwin" else "nixos");
        message = "phoebe: services.phoebe.fleet.machines.${cfg.machine}.os names another operating system than this one.";
      }
      {
        assertion = cfg.logs.bufferSize >= 268435488;
        message = "phoebe: services.phoebe.logs.bufferSize is below Vector's smallest disk buffer, 268435488 bytes.";
      }
      {
        assertion = probesWith (probe: (probe.kind == "dns") != (probe.query != null)) == [ ];
        message = "phoebe: every dns probe, and only a dns probe, sets query.";
      }
    ]
    ++ map (node: {
      assertion = machines ? ${node};
      message = "phoebe: services.phoebe.fleet.nodes names ${node}, which fleet.machines lacks.";
    }) nodes
    ++ map (service: {
      assertion = machines ? ${service.machine};
      message = "phoebe: services.phoebe.fleet.services has ${service.name} on ${service.machine}, which fleet.machines lacks.";
    }) services
    ++ map (
      group:
      let
        service = lib.head group;
      in
      {
        assertion = false;
        message = "phoebe: services.phoebe.fleet.services lists ${service.name} on ${service.machine} ${toString (lib.length group)} times, but a machine runs at most one service of each name.";
      }
    ) registeredTwice;

    environment.systemPackages = [ cfg.package ];
  };
}
