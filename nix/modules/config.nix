# every program's configuration, as attribute sets, generated from the fleet
# registry in `services.phoebe`. both platform modules and the checks call it,
# so a machine runs exactly what the checks check.
{
  lib,
  # config.services.phoebe
  cfg,
  # the host every program binds to
  listen,
  # where loki and vector keep their state
  dirs,
  # darwin only: the log files vector reads, and the ones it leaves alone
  logFiles ? [ ],
  logExclude ? [ ],
}:
let
  inherit (lib)
    attrNames
    concatLists
    concatMap
    concatStringsSep
    filter
    filterAttrs
    length
    listToAttrs
    mapAttrsToList
    min
    nameValuePair
    optional
    optionalAttrs
    unique
    ;

  inherit (cfg.fleet) machines nodes services;

  ports = {
    prometheus = 9090;
    alertmanager = 9093;
    cluster = 9094;
    loki = 3100;
    node = 9100;
    blackbox = 9115;
    vector = 9598;
  };

  me = machines.${cfg.machine};
  at = name: port: "${machines.${name}.address}:${toString port}";
  bind = port: "${listen}:${toString port}";

  # one static target per machine, labelled with the machine's name
  each =
    names: port:
    map (name: {
      targets = [ (at name port) ];
      labels.machine = name;
    }) names;
  job = name: names: port: {
    job_name = name;
    static_configs = each names port;
  };

  # PromQL's `name=~"a|b"`, for host names, which hold no other regex syntax
  oneOf = concatStringsSep "|";

  probes = concatLists (
    mapAttrsToList (
      service: declared:
      map (
        probe:
        probe
        // {
          inherit service;
          inherit (declared) machine;
        }
      ) declared.probes
    ) services
  );

  queryName = query: "dns_" + lib.replaceStrings [ "." "-" ] [ "_" "_" ] query;
  moduleOf =
    probe:
    if probe.kind == "dns" then
      queryName probe.query
    else if probe.kind == "http" then
      "http"
    else
      probe.kind + lib.optionalString probe.tls "_tls";

  tcp = tls: queries: {
    prober = "tcp";
    timeout = "5s";
    tcp = {
      preferred_ip_protocol = "ip4";
      tls = tls;
    }
    // optionalAttrs (queries != [ ]) { query_response = queries; };
  };
  conversations = {
    tcp = [ ];
    smtp = [
      { expect = "^220"; }
      { send = "EHLO phoebe"; }
      { expect = "^250 "; }
      { send = "QUIT"; }
    ];
    imap = [
      { expect = "^\\* OK"; }
      { send = ". LOGOUT"; }
      { expect = "^\\* BYE"; }
    ];
  };

  blackbox.modules = {
    http = {
      prober = "http";
      timeout = "5s";
      http.preferred_ip_protocol = "ip4";
    };
  }
  // lib.concatMapAttrs (kind: queries: {
    ${kind} = tcp false queries;
    "${kind}_tls" = tcp true queries;
  }) conversations
  // listToAttrs (
    map (
      query:
      nameValuePair (queryName query) {
        prober = "dns";
        timeout = "5s";
        dns = {
          query_name = query;
          query_type = "A";
          preferred_ip_protocol = "ip4";
          valid_rcodes = [ "NOERROR" ];
        };
      }
    ) (unique (map (probe: probe.query) (filter (probe: probe.kind == "dns") probes)))
  );

  # every node probes every target, so every prometheus holds every node's view.
  # every prometheus asks every node's blackbox exporter, so a target sees the
  # square of the node count in probes per interval, hence the longer interval
  probeJob = {
    job_name = "probe";
    scrape_interval = "60s";
    metrics_path = "/probe";
    static_configs = concatMap (
      prober:
      map (probe: {
        targets = [ probe.target ];
        labels = {
          inherit (probe) service kind machine;
          inherit prober;
          __param_module = moduleOf probe;
          __prober = at prober ports.blackbox;
        };
      }) probes
    ) nodes;
    relabel_configs = [
      {
        source_labels = [ "__address__" ];
        target_label = "__param_target";
      }
      {
        source_labels = [ "__address__" ];
        target_label = "target";
      }
      {
        source_labels = [ "__prober" ];
        target_label = "__address__";
      }
    ];
  };

  serviceJobs = mapAttrsToList (name: service: {
    job_name = "service-${name}";
    metrics_path = service.metrics.path;
    static_configs = [
      {
        targets = [ (at service.machine service.metrics.port) ];
        labels = {
          inherit (service) machine;
          service = name;
        };
      }
    ];
  }) (filterAttrs (_: service: service.metrics != null) services);

  # the systemd unit or launchd job behind a service, as a series selector
  unitSeries =
    service:
    let
      machine = machines.${service.machine};
      labels = ''machine="${service.machine}", name="${service.unit}"'';
    in
    if machine.os == "nixos" then
      ''node_systemd_unit_state{${labels}, state="failed"}''
    else
      "phoebe_launchd_job_failed{${labels}}";

  units = filterAttrs (_: service: service.unit != null) services;
  heartbeats = filterAttrs (_: service: service.heartbeat != null) services;
  alwaysOn = attrNames (filterAttrs (_: machine: machine.alwaysOn) machines);
  agree = min 2 (length nodes);

  filesystems = ''fstype!~"tmpfs|ramfs|devtmpfs|overlay|squashfs|autofs|nullfs|devfs", mountpoint!~"/nix/store|/boot.*|/System/Volumes/(Preboot|Update|VM|xarts|iSCPreboot|Hardware|Recovery)"'';
  writable = "node_filesystem_readonly{${filesystems}} == 0";
  free = "node_filesystem_avail_bytes{${filesystems}} / node_filesystem_size_bytes{${filesystems}}";

  rule = alert: severity: expr: duration: summary: {
    inherit alert expr;
    "for" = duration;
    labels.severity = severity;
    annotations.summary = summary;
  };

  rules.groups = [
    {
      name = "phoebe";
      rules =
        optional (alwaysOn != [ ]) (
          rule "MachineDown" "page" ''up{job="node", machine=~"${oneOf alwaysOn}"} == 0'' "5m"
            "{{ $labels.machine }} is down"
        )
        ++ mapAttrsToList (
          name: service:
          (rule "UnitFailed" "page" "${unitSeries service} == 1" "1m"
            "${name} failed on {{ $labels.machine }}"
          )
          // {
            labels = {
              severity = "page";
              service = name;
            };
          }
        ) units
        ++ concatLists (
          mapAttrsToList (
            name: service:
            let
              inherit (service.heartbeat) metric within;
              gauge = ''${metric}{machine="${service.machine}"}'';
              # two rules of one name and labels, a gauge behind and a gauge
              # missing, which never hold at once. promtool lints them as
              # duplicates, a warning meant for recording rules. they wait as
              # long as MachineDown, so a machine going down, whose gauges go
              # missing with it, fires MachineDown first
              stale =
                expr: summary:
                (rule "ServiceStale" "page" expr "5m" summary)
                // {
                  labels = {
                    severity = "page";
                    service = name;
                  };
                };
            in
            [
              (stale "time() - max by (machine) (${gauge}) > ${toString within}" "${name} last reported healthy {{ $value | humanizeDuration }} ago on {{ $labels.machine }}")
              # absent() keeps the machine label of the gauge's matcher
              (stale "absent(${gauge})" "${metric} of ${name} is missing on {{ $labels.machine }}")
            ]
          ) heartbeats
        )
        ++ [
          (rule "UnitFailed" "info" (
            ''(node_systemd_unit_state{state="failed"} == 1 or phoebe_launchd_job_failed == 1)''
            + lib.optionalString (units != { }) (
              " unless on(machine, name) (${concatStringsSep " or " (mapAttrsToList (_: unitSeries) units)})"
            )
          ) "1m" "{{ $labels.name }} failed on {{ $labels.machine }}")
          (rule "ServiceUnreachable" "page"
            "count by (service, machine, kind, target) (probe_success == 0) >= ${toString agree}"
            # as long as MachineDown, whose scrapes fail sooner than probes do,
            # so a machine going down fires MachineDown first and the
            # alertmanager holds back the unreachable services behind it
            "5m"
            "{{ $labels.service }} is unreachable at {{ $labels.target }} from {{ $value }} nodes"
          )
          (rule "CertExpiring" "info"
            "min by (service, machine, target) (probe_ssl_earliest_cert_expiry) - time() < 14 * 86400"
            "10m"
            "the certificate of {{ $labels.target }} expires in {{ $value | humanizeDuration }}"
          )
          (rule "DiskAlmostFull" "page" "(${free} < 0.10) and ${writable}" "15m"
            "{{ $labels.mountpoint }} on {{ $labels.machine }} is {{ $value | humanizePercentage }} free"
          )
          (rule "DiskFillingFast" "info"
            "(predict_linear(node_filesystem_avail_bytes{${filesystems}}[6h], 86400) < 0) and (${free} < 0.25) and ${writable}"
            "1h"
            "{{ $labels.mountpoint }} on {{ $labels.machine }} fills up within a day"
          )
          (rule "NodeUnhealthy" "page"
            ''(up{job=~"prometheus|loki|alertmanager|blackbox"} == 0) unless on(machine) (up{job="node"} == 0)''
            "5m"
            "{{ $labels.job }} on {{ $labels.machine }} is down"
          )
          (rule "LogsBacklog" "info"
            (
              ''vector_buffer_size_bytes{component_id=~"loki_.+"}''
              + ''/ on(machine, component_id) vector_buffer_max_size_bytes{component_id=~"loki_.+"} > 0.5''
            )
            "15m"
            "{{ $labels.machine }} filled {{ $value | humanizePercentage }} of its buffer for {{ $labels.component_id }}"
          )
        ];
    }
  ];

  prometheus = rulesFile: {
    global = {
      scrape_interval = "15s";
      evaluation_interval = "15s";
    };
    rule_files = [ rulesFile ];
    # no external labels: every node's alerts must carry the same labels, so
    # the alertmanager cluster sends each of them once
    alerting.alertmanagers = [
      { static_configs = [ { targets = map (node: at node ports.alertmanager) nodes; } ]; }
    ];
    scrape_configs = [
      (job "node" (attrNames machines) ports.node)
      (job "vector" (attrNames machines) ports.vector)
      (job "prometheus" nodes ports.prometheus)
      (job "loki" nodes ports.loki)
      (job "alertmanager" nodes ports.alertmanager)
      (job "blackbox" nodes ports.blackbox)
    ]
    ++ optional (probes != [ ]) probeJob
    ++ serviceJobs;
  };

  alertmanager = {
    route = {
      receiver = "phoebe";
      group_by = [
        "alertname"
        "machine"
        "service"
      ];
      group_wait = "30s";
      group_interval = "5m";
      repeat_interval = "4h";
    };
    # a machine that is down says so once, instead of once for each of its
    # services and stores
    inhibit_rules = [
      {
        source_matchers = [ ''alertname="MachineDown"'' ];
        target_matchers = [
          ''alertname=~"ServiceUnreachable|ServiceStale|UnitFailed|NodeUnhealthy|CertExpiring"''
        ];
        equal = [ "machine" ];
      }
    ];
    receivers = [
      {
        name = "phoebe";
        webhook_configs = optional (cfg.alerting.webhook != null) {
          url = cfg.alerting.webhook;
          send_resolved = true;
        };
      }
    ];
  };

  loki = {
    auth_enabled = false;
    server = {
      http_listen_address = listen;
      http_listen_port = ports.loki;
      grpc_listen_address = "127.0.0.1";
      grpc_listen_port = 9095;
      log_level = "warn";
    };
    common = {
      path_prefix = dirs.loki;
      replication_factor = 1;
      # loki's parts talk to each other over grpc on loopback. left to
      # itself, each advertises the first private address it finds, which on
      # a machine with several interfaces need not be one it listens on
      instance_addr = "127.0.0.1";
      ring.kvstore.store = "inmemory";
      storage.filesystem = {
        chunks_directory = "${dirs.loki}/chunks";
        rules_directory = "${dirs.loki}/rules";
      };
    };
    schema_config.configs = [
      {
        from = "2026-01-01";
        store = "tsdb";
        object_store = "filesystem";
        schema = "v13";
        index = {
          prefix = "index_";
          period = "24h";
        };
      }
    ];
    frontend.address = "127.0.0.1";
    compactor = {
      working_directory = "${dirs.loki}/compactor";
      retention_enabled = true;
      delete_request_store = "filesystem";
    };
    limits_config = {
      retention_period = cfg.node.retention.logs;
      # a machine's backlog reaches a node that was down, however old it is
      reject_old_samples = false;
      # every machine pushes as the same tenant, so the limits are the
      # fleet's: room for six machines draining their buffers at once after
      # a node comes back
      ingestion_rate_mb = 16;
      ingestion_burst_size_mb = 32;
      per_stream_rate_limit = "8MB";
      per_stream_rate_limit_burst = "16MB";
      # streams are a machine, a unit and a level, a few hundred per machine
      max_global_streams_per_user = 20000;
    };
    analytics.reporting_enabled = false;
  };

  journald = me.os == "nixos";

  vector = {
    data_dir = dirs.vector;
    sources = {
      logs =
        if journald then
          { type = "journald"; }
        else
          {
            type = "file";
            include = logFiles;
            exclude = logExclude;
            # a file vector has no checkpoint for is read from its end, and a
            # file untouched for a day is left alone, so a mac joining the fleet
            # sends what its daemons write from then on, not years of history
            read_from = "end";
            ignore_older_secs = 86400;
          };
      vector_metrics.type = "internal_metrics";
    };
    transforms.labelled = {
      type = "remap";
      inputs = [ "logs" ];
      source = ''
        .machine = ${builtins.toJSON cfg.machine}
      ''
      + (
        if journald then
          ''
            # one stream per kind of unit rather than per instance: every
            # login is a session-<n>.scope and every deploy a coeus-job-<n>.service
            unit = string(._SYSTEMD_UNIT) ?? string(.SYSLOG_IDENTIFIER) ?? "unknown"
            unit = replace(unit, r'^run-[a-z]?[0-9a-f]+\.(?P<kind>service|scope)$', "run.$kind")
            unit = replace(unit, r'-[0-9]+\.(?P<kind>service|scope)$', ".$kind")
            unit = replace(unit, r'@[^.]*\.(?P<kind>service|scope)$', "@.$kind")
            .unit = unit
            priority = to_int(.PRIORITY) ?? 6
            .level = to_syslog_level(priority) ?? "info"
          ''
        else
          ''
            .file = string(.file) ?? "unknown"
          ''
      );
    };
    # loki logs an error for every query a client gives up on, a browser
    # closing a log tail among them. shipped back into loki, those lines
    # would swamp the very tail being read
    transforms.quiet = {
      type = "filter";
      inputs = [ "labelled" ];
      condition =
        if journald then
          ''!(.unit == "loki.service" && contains(string(.message) ?? "", "context canceled"))''
        else
          ''!(.file == "/var/log/phoebe-loki.log" && contains(string(.message) ?? "", "context canceled"))'';
    };
    sinks = {
      metrics = {
        type = "prometheus_exporter";
        inputs = [ "vector_metrics" ];
        address = bind ports.vector;
      };
    }
    # one sink per node, each with a disk buffer of its own, so a node that is
    # down holds up nothing but its own backlog
    // listToAttrs (
      map (
        node:
        nameValuePair "loki_${node}" {
          type = "loki";
          inputs = [ "quiet" ];
          endpoint = "http://${at node ports.loki}";
          encoding.codec = "text";
          labels = {
            machine = "{{ machine }}";
          }
          // (
            if journald then
              {
                unit = "{{ unit }}";
                level = "{{ level }}";
              }
            else
              { file = "{{ file }}"; }
          );
          out_of_order_action = "accept";
          healthcheck.enabled = false;
          buffer = {
            type = "disk";
            max_size = cfg.logs.bufferSize;
            when_full = "drop_newest";
          };
        }
      ) nodes
    );
  };

  # the registry, for the app, which also learns from it which machine it
  # runs on
  fleet = {
    inherit nodes ports;
    inherit (cfg) machine;
    machines = lib.mapAttrs (_: machine: { inherit (machine) os alwaysOn address; }) machines;
    services = lib.mapAttrs (_: service: {
      inherit (service)
        machine
        unit
        metrics
        heartbeat
        ;
      probes = map (probe: {
        inherit (probe)
          kind
          target
          tls
          query
          ;
      }) service.probes;
    }) services;
  };
in
{
  inherit
    ports
    me
    rules
    prometheus
    alertmanager
    loki
    vector
    blackbox
    fleet
    ;
  isNode = lib.elem cfg.machine nodes;
  # the other nodes' alertmanagers, by address
  peers = map (node: machines.${node}.address) (filter (node: node != cfg.machine) nodes);
}
