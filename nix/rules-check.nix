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
    # web and sync each run on two machines, under one name
    services = [
      {
        name = "web";
        machine = "n1";
        unit = "web.service";
        probes = [
          {
            kind = "http";
            target = "https://web.example.com";
          }
        ];
        metrics.port = 9201;
      }
      {
        name = "web";
        machine = "n2";
        unit = "web.service";
        probes = [
          {
            kind = "http";
            target = "https://web.example.com";
          }
        ];
        metrics.port = 9201;
      }
      {
        name = "api";
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
      }
      {
        name = "mail";
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
      }
      {
        name = "resolver";
        machine = "n1";
        probes = [
          {
            kind = "dns";
            target = "10.0.0.1";
            query = "example.com";
          }
        ];
      }
      {
        name = "coeus";
        machine = "n3";
        unit = "org.nixos.coeusd";
      }
      {
        name = "backup";
        machine = "n1";
        heartbeat = {
          metric = "backup_healthy_timestamp_seconds";
          within = 3600;
        };
      }
      {
        name = "sync";
        machine = "n1";
        unit = "sync.service";
        heartbeat = {
          metric = "sync_healthy_timestamp_seconds";
          within = 600;
        };
      }
      {
        name = "sync";
        machine = "n2";
        unit = "sync.service";
        heartbeat = {
          metric = "sync_healthy_timestamp_seconds";
          within = 600;
        };
      }
      {
        name = "report";
        machine = "n3";
        heartbeat = {
          metric = "report_healthy_timestamp_seconds";
          within = 86400;
        };
      }
    ];
  };

  # the registry's options over `fleet`, as a nixos machine of it evaluates them
  evaluate =
    fleet:
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
    }).config;
  cfg = (evaluate fleet).services.phoebe;

  # the messages of the assertions a fleet fails. the machine evaluating it
  # is n1, a nixos machine, so on darwin n1's os fails too
  failed =
    fleet:
    map (assertion: assertion.message) (
      lib.filter (assertion: !assertion.assertion) (evaluate fleet).assertions
    );
  # a machine runs at most one service of each name: sync on n1 and on n2 pass, a
  # second sync on n2 fails
  twice = fleet // {
    services = fleet.services ++ [
      {
        name = "sync";
        machine = "n2";
      }
    ];
  };
  refused = lib.subtractLists (failed fleet) (failed twice);
  registry =
    assert lib.assertMsg (
      refused == [
        "phoebe: services.phoebe.fleet.services lists sync on n2 2 times, but a machine runs at most one service of each name."
      ]
    ) "a service registered twice on one machine fails ${toJSON refused}";
    "the registry's assertions hold";

  # promtool refuses two jobs of one name, so each service's job is its own
  serviceJobs = map (job: job.job_name) (
    lib.filter (job: lib.hasPrefix "service-" job.job_name) (generated.prometheus rules).scrape_configs
  );
  jobs =
    assert lib.assertMsg (
      serviceJobs == [
        "service-n1-web"
        "service-n2-web"
        "service-n2-api"
      ]
    ) "the service jobs are ${toJSON serviceJobs}";
    "each service scrapes under a job of its machine and name";

  inherit (builtins) toJSON;

  generated = import ./modules/config.nix {
    inherit lib cfg;
    listen = "0.0.0.0";
    dirs = {
      loki = "/var/lib/loki";
      vector = "/var/lib/vector";
    };
  };

  json = name: value: pkgs.writeText name (toJSON value);

  # alerts as a node's prometheus posts them to its alertmanager
  posted =
    map
      (labels: {
        inherit labels;
        annotations.summary = "";
      })
      [
        {
          alertname = "MachineDown";
          machine = "n1";
          severity = "page";
        }
        {
          alertname = "ServiceStale";
          service = "sync";
          machine = "n1";
          severity = "page";
        }
        {
          alertname = "ServiceStale";
          service = "sync";
          machine = "n2";
          severity = "page";
        }
      ];
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
  probe = service: machine: kind: target: prober: {
    job = "probe";
    inherit
      service
      machine
      kind
      target
      prober
      ;
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
        # ServiceUnreachable needs two of the three nodes to see a probe fail,
        # each service apart: web on n1 fails from one node, web on n2 at the
        # same target from two
        interval = "15s";
        input_series =
          let
            web = machine: probe "web" machine "http" "https://web.example.com";
            api = probe "api" "n2" "tcp" "api.example.com:443";
          in
          [
            (series "probe_success" (web "n1" "n1") "0x40")
            (series "probe_success" (web "n1" "n2") "1x40")
            (series "probe_success" (web "n1" "n3") "1x40")
            (series "probe_success" (web "n2" "n1") "0x40")
            (series "probe_success" (web "n2" "n2") "0x40")
            (series "probe_success" (web "n2" "n3") "1x40")
            (series "probe_success" (api "n1") "0x40")
            (series "probe_success" (api "n2") "0x40")
            (series "probe_success" (api "n3") "1x40")
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
                exp_annotations.summary = "api on n2 is unreachable at api.example.com:443 from 2 nodes";
              }
              {
                exp_labels = {
                  service = "web";
                  machine = "n2";
                  kind = "http";
                  target = "https://web.example.com";
                  severity = "page";
                };
                exp_annotations.summary = "web on n2 is unreachable at https://web.example.com from 2 nodes";
              }
            ];
          }
        ];
      }
      {
        # a fleet service's unit pages on either platform, on its own machine
        # alone; any other failed unit only informs
        interval = "15s";
        input_series = [
          (series "node_systemd_unit_state" (
            node "n2"
            // {
              name = "web.service";
              state = "failed";
            }
          ) "0x20")
          (series "node_systemd_unit_state" (
            node "n1"
            // {
              name = "sync.service";
              state = "failed";
            }
          ) "0x20")
          (series "node_systemd_unit_state" (
            node "n2"
            // {
              name = "sync.service";
              state = "failed";
            }
          ) "1x20")
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
              {
                exp_labels = node "n2" // {
                  name = "sync.service";
                  state = "failed";
                  service = "sync";
                  severity = "page";
                };
                exp_annotations.summary = "sync failed on n2";
              }
            ];
          }
        ];
      }
      {
        # ServiceStale pages for a heartbeat that falls behind for 5 minutes,
        # or one that is missing as long; a fresh one stays quiet, though a
        # service of its name on another machine pages
        interval = "1m";
        input_series = [
          # last healthy at 0, an hour behind from 60m on
          (series "backup_healthy_timestamp_seconds" (node "n1") "0x70")
          # healthy every minute on n2, never written on n1
          (series "sync_healthy_timestamp_seconds" (node "n2") "0+60x70")
          # and report_healthy_timestamp_seconds, never written
        ];
        alert_rule_test =
          let
            late = {
              exp_labels = {
                service = "backup";
                machine = "n1";
                severity = "page";
              };
              exp_annotations.summary = "backup on n1 last reported healthy 1h 6m 0s ago";
            };
            missing = service: machine: metric: {
              exp_labels = {
                inherit service machine;
                severity = "page";
              };
              exp_annotations.summary = "${service} on ${machine} is missing ${metric}";
            };
            report = missing "report" "n3" "report_healthy_timestamp_seconds";
            sync = missing "sync" "n1" "sync_healthy_timestamp_seconds";
          in
          [
            {
              eval_time = "4m";
              alertname = "ServiceStale";
              exp_alerts = [ ];
            }
            {
              eval_time = "6m";
              alertname = "ServiceStale";
              exp_alerts = [
                report
                sync
              ];
            }
            {
              eval_time = "65m";
              alertname = "ServiceStale";
              exp_alerts = [
                report
                sync
              ];
            }
            {
              eval_time = "66m";
              alertname = "ServiceStale";
              exp_alerts = [
                late
                report
                sync
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

  # vector's own unit tests, over the transforms every journald machine runs
  logs = event: {
    insert_at = "labelled";
    type = "log";
    log_fields = event;
  };
  vectorTests = generated.vector // {
    tests = [
      {
        name = "a canceled query loki logs stays out of loki";
        inputs = [
          (logs {
            _SYSTEMD_UNIT = "loki.service";
            PRIORITY = "3";
            message = ''level=error caller=scheduler_processor.go:111 component=querier msg="error processing requests from scheduler" err="rpc error: code = Canceled desc = context canceled"'';
          })
        ];
        no_outputs_from = [ "quiet" ];
      }
      {
        name = "every other line reaches loki, loki's own included";
        inputs = [
          (logs {
            _SYSTEMD_UNIT = "loki.service";
            PRIORITY = "3";
            message = "level=error msg=\"failed to flush chunks\"";
          })
          (logs {
            _SYSTEMD_UNIT = "web.service";
            PRIORITY = "6";
            message = "context canceled";
          })
        ];
        outputs = [
          {
            extract_from = "quiet";
            conditions = [
              {
                type = "vrl";
                source = ''.machine == "n1" && includes(["loki.service", "web.service"], .unit)'';
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
    # what nix checks as it evaluates, held here so the check evaluates it
    evaluated = [
      registry
      jobs
    ];
    # the alertmanager listens on loopback
    __darwinAllowLocalNetworking = true;
    nativeBuildInputs = [
      pkgs.curl
      pkgs.jq
      pkgs.prometheus.cli
      pkgs.prometheus-alertmanager
      pkgs.prometheus-blackbox-exporter
      pkgs.vector
    ];
  }
  ''
    promtool check config ${json "prometheus.yml" (generated.prometheus rules)}
    promtool test rules ${json "tests.yml" tests}
    amtool check-config ${json "alertmanager.yml" generated.alertmanager}

    # a machine that is down holds back the alerts of its own services alone,
    # not those of a service of the same name on another machine
    alertmanager --config.file ${json "alertmanager.yml" generated.alertmanager} \
      --storage.path "$TMPDIR/alertmanager" --web.listen-address 127.0.0.1:19093 \
      --cluster.listen-address "" --log.level warn &
    trap 'kill %1' EXIT
    for _ in $(seq 50); do curl -sf 127.0.0.1:19093/-/ready > /dev/null && break; sleep 0.2; done
    curl -sf -H 'Content-Type: application/json' -d @${json "posted.json" posted} 127.0.0.1:19093/api/v2/alerts
    held=$(curl -sf 127.0.0.1:19093/api/v2/alerts \
      | jq -c 'map(select(.status.inhibitedBy != []) | .labels | {alertname, machine}) | sort')
    if [ "$held" != '[{"alertname":"ServiceStale","machine":"n1"}]' ]; then
      echo "MachineDown on n1 holds back $held" >&2
      exit 1
    fi
    vector test ${json "vector.json" vectorTests}
    blackbox_exporter --config.check --config.file ${json "blackbox.yml" generated.blackbox}
    touch $out
  ''
