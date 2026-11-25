# the configuration a small fleet generates, checked by the tools of each
# program that reads it, and the alert rules tested against made-up series
{
  pkgs,
  lib,
  self,
}:
let
  fleet = {
    nodes = [
      "n1"
      "n2"
      "n3"
    ];
    machines = {
      n1 = {
        os = "nixos";
        alwaysOn = true;
        address = "10.0.0.1";
      };
      n2 = {
        os = "nixos";
        alwaysOn = true;
        address = "10.0.0.2";
      };
      n3 = {
        os = "darwin";
        alwaysOn = true;
        address = "10.0.0.3";
      };
      desktop = {
        os = "nixos";
        alwaysOn = false;
        address = "10.0.0.4";
      };
    };
    services = {
      web = {
        machine = "n1";
        unit = "web.service";
        probes = [
          {
            kind = "http";
            target = "https://web.example.com";
          }
        ];
      };
      api = {
        machine = "n2";
        unit = "api.service";
        probes = [
          {
            kind = "tcp";
            target = "api.example.com:443";
            tls = true;
          }
        ];
        metrics.port = 9200;
      };
      mail = {
        machine = "n2";
        probes = [
          {
            kind = "smtp";
            target = "mail.example.com:25";
          }
          {
            kind = "imap";
            target = "mail.example.com:993";
            tls = true;
          }
        ];
      };
      resolver = {
        machine = "n1";
        probes = [
          {
            kind = "dns";
            target = "10.0.0.1";
            query = "example.com";
          }
        ];
      };
      coeus = {
        machine = "n3";
        unit = "org.nixos.coeusd";
      };
    };
  };

  cfg =
    (lib.evalModules {
      specialArgs = { inherit pkgs; };
      modules = [
        (import ./modules/common.nix { inherit self; })
        {
          options = {
            assertions = lib.mkOption {
              type = lib.types.listOf lib.types.raw;
              default = [ ];
            };
            environment = lib.mkOption {
              type = lib.types.submodule { freeformType = lib.types.lazyAttrsOf lib.types.raw; };
              default = { };
            };
            networking.hostName = lib.mkOption { type = lib.types.str; };
          };
          config = {
            networking.hostName = "n1";
            services.phoebe = {
              enable = true;
              inherit fleet;
              alerting.webhook = "http://127.0.0.1:8080/alerts";
            };
          };
        }
      ];
    }).config.services.phoebe;

  generated = import ./modules/config.nix {
    inherit lib cfg;
    listen = "0.0.0.0";
    dirs = {
      loki = "/var/lib/loki";
      vector = "/var/lib/vector";
    };
  };

  json = name: value: pkgs.writeText name (builtins.toJSON value);
  rules = json "rules.yml" generated.rules;

  series = name: labels: values: {
    series = "${name}{${
      lib.concatStringsSep ", " (lib.mapAttrsToList (key: value: ''${key}="${value}"'') labels)
    }}";
    inherit values;
  };
  node = machine: {
    job = "node";
    inherit machine;
    instance = "${fleet.machines.${machine}.address}:9100";
  };
  probe = service: kind: target: prober: {
    job = "probe";
    inherit
      service
      kind
      target
      prober
      ;
    machine = fleet.services.${service}.machine;
    instance = "${fleet.machines.${prober}.address}:9115";
  };

  tests = {
    rule_files = [ "${rules}" ];
    evaluation_interval = "15s";
    tests = [
      {
        # MachineDown pages for a machine meant to be on, never for one that
        # may be off
        interval = "15s";
        input_series = [
          (series "up" (node "n1") "1x40")
          (series "up" (node "n2") "0x40")
          (series "up" (node "desktop") "0x40")
        ];
        alert_rule_test = [
          {
            eval_time = "4m";
            alertname = "MachineDown";
            exp_alerts = [ ];
          }
          {
            eval_time = "6m";
            alertname = "MachineDown";
            exp_alerts = [
              {
                exp_labels = node "n2" // {
                  severity = "page";
                };
                exp_annotations.summary = "n2 is down";
              }
            ];
          }
        ];
      }
      {
        # ServiceUnreachable needs two of the three nodes to see a probe fail
        interval = "15s";
        input_series =
          map (prober: series "probe_success" (probe "web" "http" "https://web.example.com" prober) "1x40") [
            "n2"
            "n3"
          ]
          ++ [
            (series "probe_success" (probe "web" "http" "https://web.example.com" "n1") "0x40")
            (series "probe_success" (probe "api" "tcp" "api.example.com:443" "n1") "0x40")
            (series "probe_success" (probe "api" "tcp" "api.example.com:443" "n2") "0x40")
            (series "probe_success" (probe "api" "tcp" "api.example.com:443" "n3") "1x40")
          ];
        alert_rule_test = [
          {
            eval_time = "1m";
            alertname = "ServiceUnreachable";
            exp_alerts = [ ];
          }
          {
            eval_time = "4m";
            alertname = "ServiceUnreachable";
            exp_alerts = [ ];
          }
          {
            eval_time = "6m";
            alertname = "ServiceUnreachable";
            exp_alerts = [
              {
                exp_labels = {
                  service = "api";
                  machine = "n2";
                  kind = "tcp";
                  target = "api.example.com:443";
                  severity = "page";
                };
                exp_annotations.summary = "api is unreachable at api.example.com:443 from 2 nodes";
              }
            ];
          }
        ];
      }
      {
        # a fleet service's unit pages on either platform; any other failed
        # unit only informs
        interval = "15s";
        input_series = [
          (series "node_systemd_unit_state" (
            node "n1"
            // {
              name = "web.service";
              state = "failed";
            }
          ) "1x20")
          (series "node_systemd_unit_state" (
            node "n1"
            // {
              name = "backup.service";
              state = "failed";
            }
          ) "1x20")
          (series "node_systemd_unit_state" (
            node "n2"
            // {
              name = "api.service";
              state = "failed";
            }
          ) "0x20")
          (series "phoebe_launchd_job_failed" (node "n3" // { name = "org.nixos.coeusd"; }) "1x20")
        ];
        alert_rule_test = [
          {
            eval_time = "2m";
            alertname = "UnitFailed";
            exp_alerts = [
              {
                exp_labels = node "n1" // {
                  name = "web.service";
                  state = "failed";
                  service = "web";
                  severity = "page";
                };
                exp_annotations.summary = "web failed on n1";
              }
              {
                exp_labels = node "n1" // {
                  name = "backup.service";
                  state = "failed";
                  severity = "info";
                };
                exp_annotations.summary = "backup.service failed on n1";
              }
              {
                exp_labels = node "n3" // {
                  name = "org.nixos.coeusd";
                  service = "coeus";
                  severity = "page";
                };
                exp_annotations.summary = "coeus failed on n3";
              }
            ];
          }
        ];
      }
      {
        # LogsBacklog tells once a buffer for a node is more than half full
        interval = "1m";
        input_series =
          let
            buffer = component_id: {
              job = "vector";
              machine = "desktop";
              instance = "10.0.0.4:9598";
              inherit component_id;
              buffer_id = component_id;
              buffer_type = "disk";
              component_kind = "sink";
              component_type = "loki";
              stage = "0";
            };
          in
          [
            (series "vector_buffer_size_bytes" (buffer "loki_n1") "200000000x20")
            (series "vector_buffer_size_bytes" (buffer "loki_n2") "1000x20")
            (series "vector_buffer_max_size_bytes" (buffer "loki_n1") "268435488x20")
            (series "vector_buffer_max_size_bytes" (buffer "loki_n2") "268435488x20")
          ];
        alert_rule_test = [
          {
            eval_time = "16m";
            alertname = "LogsBacklog";
            exp_alerts = [
              {
                exp_labels = {
                  machine = "desktop";
                  component_id = "loki_n1";
                  severity = "info";
                };
                exp_annotations.summary = "desktop filled 74.51% of its buffer for loki_n1";
              }
            ];
          }
        ];
      }
    ];
  };
in
pkgs.runCommandLocal "phoebe-rules"
  {
    nativeBuildInputs = [
      pkgs.prometheus.cli
      pkgs.prometheus-alertmanager
      pkgs.prometheus-blackbox-exporter
    ];
  }
  ''
    promtool check config ${json "prometheus.yml" (generated.prometheus rules)}
    promtool test rules ${json "tests.yml" tests}
    amtool check-config ${json "alertmanager.yml" generated.alertmanager}
    blackbox_exporter --config.check --config.file ${json "blackbox.yml" generated.blackbox}
    touch $out
  ''
