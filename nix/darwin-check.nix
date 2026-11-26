# evaluates nix-darwin systems running phoebe, a node and a machine that only
# sends, and checks their launchd daemons without building either system. the
# node's plists land in $out/plists, to read.
{
  pkgs,
  self,
  nix-darwin,
}:
let
  inherit (pkgs) lib;

  fleet = {
    nodes = [
      "oracle-1"
      "altar"
    ];
    machines = {
      oracle-1 = {
        os = "nixos";
        alwaysOn = true;
        address = "100.64.0.1";
      };
      altar = {
        os = "darwin";
        alwaysOn = true;
        address = "100.64.0.2";
      };
      laptop = {
        os = "darwin";
        alwaysOn = false;
        address = "100.64.0.3";
      };
    };
    services.coeus = {
      machine = "altar";
      unit = "org.nixos.coeusd";
    };
  };

  system =
    hostName: extra:
    (nix-darwin.lib.darwinSystem {
      modules = [
        self.darwinModules.default
        extra
        {
          nixpkgs.hostPlatform = pkgs.stdenv.hostPlatform.system;
          system.stateVersion = 6;
          networking.hostName = hostName;
          services.phoebe = {
            enable = true;
            inherit fleet;
          };
        }
      ];
    }).config;

  facts =
    config:
    let
      daemons = lib.filterAttrs (
        name: _: lib.hasPrefix "phoebe-" name || name == "prometheus-node-exporter"
      ) config.launchd.daemons;
    in
    {
      # forces the whole system, every daemon's plist and /etc included
      system = config.system.build.toplevel.drvPath;
      failed = map (assertion: assertion.message) (lib.filter (a: !a.assertion) config.assertions);
      daemons = lib.mapAttrs (_: daemon: daemon.serviceConfig) daemons;
      plists = lib.mapAttrs' (
        name: daemon:
        lib.nameValuePair name config.environment.launchDaemons."${daemon.serviceConfig.Label}.plist".text
      ) daemons;
      user = if config.users.users ? _phoebe then config.users.users._phoebe.uid else null;
      fleet = config.environment.etc."phoebe/fleet.json".text;
      inherit (config.services.phoebe.generated) vector loki alertmanager;
      # nix-darwin's own module runs it from a script
      exporter = { inherit (config.services.prometheus.exporters.node) listenAddress port extraFlags; };
    };

  json = value: builtins.unsafeDiscardStringContext (builtins.toJSON value);
in
pkgs.runCommandLocal "phoebe-darwin-module"
  {
    node = json (
      facts (
        system "altar" {
          services.phoebe.textfileDirectories = [
            "/var/lib/backup/metrics"
            "/var/lib/jobs/*/metrics"
          ];
          services.phoebe.web = {
            enable = true;
            publicOrigin = "https://phoebe.example.com";
            sessionKeyFile = "/run/agenix/phoebe-session-key";
            signin.domain = "tail0000.ts.net";
            tunnel = {
              id = "6f1c2a9e-3b7d-4e58-9a0c-d2e4f6a8b1c3";
              credentialsFile = "/run/agenix/phoebe-tunnel";
            };
            push = {
              vapidKeyFile = "/run/agenix/phoebe-vapid";
              subscriptionsFile = "/run/agenix/phoebe-push-subscriptions";
              edit = "agenix -e secrets/phoebe-push-subscriptions.age";
            };
          };
        }
      )
    );
    agent = json (facts (system "laptop" { }));
    passAsFile = [
      "node"
      "agent"
    ];
    nativeBuildInputs = [ pkgs.jq ];
  }
  ''
    node=$nodePath agent=$agentPath

    jq -e '.failed == []' $node $agent

    # the node runs every store and the app, the agent none
    jq -e '.daemons | keys == [
      "phoebe-alertmanager", "phoebe-blackbox", "phoebe-launchd-state", "phoebe-loki",
      "phoebe-prometheus", "phoebe-vector", "phoebe-web", "phoebe-web-cert", "prometheus-node-exporter"
    ]' $node
    jq -e '.daemons | keys == ["phoebe-launchd-state", "phoebe-vector", "prometheus-node-exporter"]' $agent
    jq -e '.user == 472' $node
    jq -e '.user == null' $agent

    # the stores and the app run as _phoebe; vector, the job reporter and the
    # certificate's fetcher as root
    jq -e '[.daemons | to_entries[] | select(.key | test("prometheus$|alertmanager|loki|blackbox|web$"))
      | select(.key != "prometheus-node-exporter") | .value.UserName] | unique == ["_phoebe"]' $node
    jq -e '[.daemons["phoebe-vector", "phoebe-launchd-state", "phoebe-web-cert"].UserName] == [null, null, null]' $node

    # every listener binds the tailnet address, never every interface
    args() { jq -r --arg d "$2" '.daemons[$d].ProgramArguments | join(" ")' "$1"; }
    for daemon in $(jq -r '.daemons | keys[]' $node); do
      if args $node $daemon | grep -E '0\.0\.0\.0|\[::\]|listen-address=:'; then
        echo "$daemon listens on every interface" >&2
        exit 1
      fi
    done
    args $node phoebe-prometheus | grep -qF -- '--web.listen-address=100.64.0.2:9090'
    args $node phoebe-alertmanager | grep -qF -- '--web.listen-address=100.64.0.2:9093'
    args $node phoebe-alertmanager | grep -qF -- '--cluster.listen-address=100.64.0.2:9094'
    args $node phoebe-alertmanager | grep -qF -- '--cluster.advertise-address=100.64.0.2:9094'
    args $node phoebe-alertmanager | grep -qF -- '--cluster.peer=100.64.0.1:9094'
    ! args $node phoebe-alertmanager | grep -qF -- '--cluster.peer=100.64.0.2'
    args $node phoebe-blackbox | grep -qF -- '--web.listen-address=100.64.0.2:9115'
    # the app and its hook stay on loopback, and alertmanager posts to the hook
    # the app on a socket of its own directory, for cloudflared, which stays
    # in its process group
    args $node phoebe-web | grep -qF -- '--socket /var/lib/phoebe-web/origin.sock --hook 127.0.0.1:7981'
    jq -e '.daemons["phoebe-web"].AbandonProcessGroup == false' $node
    # launchd hands the app its secrets where they are, for _phoebe to read
    args $node phoebe-web | grep -qF -- '--origin https://phoebe.example.com --session-key-file /run/agenix/phoebe-session-key'
    args $node phoebe-web | grep -qF -- '--signin-port 45731 --signin-cert /var/lib/phoebe-web/cert.pem --signin-key /var/lib/phoebe-web/key.pem --capability r3dlust.com/cap/phoebe'
    args $node phoebe-web | grep -qF -- '--signin-url https://oracle-1.tail0000.ts.net:45731 --signin-url https://altar.tail0000.ts.net:45731'
    args $node phoebe-web | grep -qF -- '--tunnel-id 6f1c2a9e-3b7d-4e58-9a0c-d2e4f6a8b1c3 --tunnel-credentials-file /run/agenix/phoebe-tunnel --cloudflared /nix/store/'
    args $node phoebe-web | grep -qF -- "--push-key-file /run/agenix/phoebe-vapid --push-subscriptions-file /run/agenix/phoebe-push-subscriptions --push-edit 'agenix -e secrets/phoebe-push-subscriptions.age'"
    jq -e '.daemons["phoebe-web-cert"].StartInterval == 3600' $node
    jq -e '.alertmanager.receivers[0].webhook_configs == [{url: "http://127.0.0.1:7981/hook", send_resolved: true}]' $node
    jq -e '.daemons["phoebe-web"].StandardOutPath == "/var/log/phoebe-web.log"' $node
    # the node exporter serves phoebe's own textfiles, then the machine's
    jq -e '.exporter == {listenAddress: "100.64.0.2", port: 9100,
      extraFlags: ["--collector.textfile.directory=/var/lib/phoebe/textfile",
        "--collector.textfile.directory=/var/lib/backup/metrics",
        "--collector.textfile.directory=/var/lib/jobs/*/metrics"]}' $node
    jq -e '.exporter == {listenAddress: "100.64.0.3", port: 9100,
      extraFlags: ["--collector.textfile.directory=/var/lib/phoebe/textfile"]}' $agent
    jq -e '.loki.server.http_listen_address == "100.64.0.2" and .loki.server.grpc_listen_address == "127.0.0.1"' $node
    jq -e '.vector.sinks.metrics.address == "100.64.0.3:9598"' $agent

    # the agent sends its log files to both nodes, each through its own buffer
    jq -e '.vector.sources.logs.type == "file" and .vector.sources.logs.include == ["/var/log/*.log"]' $agent
    jq -e '[.vector.sinks | to_entries[] | select(.value.type == "loki") | {key, endpoint: .value.endpoint, buffer: .value.buffer.type}]
      == [{key: "loki_altar", endpoint: "http://100.64.0.2:3100", buffer: "disk"},
          {key: "loki_oracle-1", endpoint: "http://100.64.0.1:3100", buffer: "disk"}]' $agent

    jq -e '.fleet | fromjson | .machines.laptop == {os: "darwin", alwaysOn: false, address: "100.64.0.3"}' $node
    jq -e '.fleet | fromjson | .machine == "altar"' $node

    mkdir -p $out/plists
    for daemon in $(jq -r '.plists | keys[]' $node); do
      jq -r --arg d "$daemon" '.plists[$d]' $node > $out/plists/$daemon.plist
    done
  ''
