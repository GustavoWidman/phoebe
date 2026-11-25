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
    hostName:
    (nix-darwin.lib.darwinSystem {
      modules = [
        self.darwinModules.default
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
      inherit (config.services.phoebe.generated) vector loki;
      # nix-darwin's own module runs it from a script
      exporter = { inherit (config.services.prometheus.exporters.node) listenAddress port extraFlags; };
    };

  json = value: builtins.unsafeDiscardStringContext (builtins.toJSON value);
in
pkgs.runCommandLocal "phoebe-darwin-module"
  {
    node = json (facts (system "altar"));
    agent = json (facts (system "laptop"));
    passAsFile = [
      "node"
      "agent"
    ];
    nativeBuildInputs = [ pkgs.jq ];
  }
  ''
    node=$nodePath agent=$agentPath

    jq -e '.failed == []' $node $agent

    # the node runs every store, the agent none
    jq -e '.daemons | keys == [
      "phoebe-alertmanager", "phoebe-blackbox", "phoebe-launchd-state", "phoebe-loki",
      "phoebe-prometheus", "phoebe-vector", "prometheus-node-exporter"
    ]' $node
    jq -e '.daemons | keys == ["phoebe-launchd-state", "phoebe-vector", "prometheus-node-exporter"]' $agent
    jq -e '.user == 472' $node
    jq -e '.user == null' $agent

    # the stores run as _phoebe, vector and the job reporter as root
    jq -e '[.daemons | to_entries[] | select(.key | test("prometheus$|alertmanager|loki|blackbox"))
      | select(.key != "prometheus-node-exporter") | .value.UserName] | unique == ["_phoebe"]' $node
    jq -e '[.daemons["phoebe-vector", "phoebe-launchd-state"].UserName] == [null, null]' $node

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
    jq -e '.exporter == {listenAddress: "100.64.0.2", port: 9100,
      extraFlags: ["--collector.textfile.directory=/var/lib/phoebe/textfile"]}' $node
    jq -e '.exporter.listenAddress == "100.64.0.3"' $agent
    jq -e '.loki.server.http_listen_address == "100.64.0.2" and .loki.server.grpc_listen_address == "127.0.0.1"' $node
    jq -e '.vector.sinks.metrics.address == "100.64.0.3:9598"' $agent

    # the agent sends its log files to both nodes, each through its own buffer
    jq -e '.vector.sources.logs.type == "file" and .vector.sources.logs.include == ["/var/log/*.log"]' $agent
    jq -e '[.vector.sinks | to_entries[] | select(.value.type == "loki") | {key, endpoint: .value.endpoint, buffer: .value.buffer.type}]
      == [{key: "loki_altar", endpoint: "http://100.64.0.2:3100", buffer: "disk"},
          {key: "loki_oracle-1", endpoint: "http://100.64.0.1:3100", buffer: "disk"}]' $agent

    jq -e '.fleet | fromjson | .machines.laptop == {os: "darwin", alwaysOn: false, address: "100.64.0.3"}' $node

    mkdir -p $out/plists
    for daemon in $(jq -r '.plists | keys[]' $node); do
      jq -r --arg d "$daemon" '.plists[$d]' $node > $out/plists/$daemon.plist
    done
  ''
