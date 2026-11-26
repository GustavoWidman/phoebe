# the agent and the stores as nixpkgs' own services, fed phoebe's generated
# configuration. every program listens on every interface, and the firewall
# lets other machines in on the tailnet interface only.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.phoebe;
  generated = import ./config.nix {
    inherit lib cfg;
    listen = "0.0.0.0";
    dirs = {
      loki = config.services.loki.dataDir;
      vector = "/var/lib/vector";
    };
  };
  inherit (generated) ports isNode;
  json = name: value: pkgs.writeText name (builtins.toJSON value);
in
{
  options.services.phoebe.interface = lib.mkOption {
    type = lib.types.str;
    default = config.services.tailscale.interfaceName;
    defaultText = lib.literalExpression "config.services.tailscale.interfaceName";
    description = ''
      The network interface other machines reach this one on. The firewall
      opens phoebe's ports on it alone.
    '';
  };

  config = lib.mkIf cfg.enable (
    lib.mkMerge [
      {
        services.phoebe.generated = generated;
        environment.etc."phoebe/fleet.json".text = builtins.toJSON generated.fleet;

        services.prometheus.exporters.node = {
          enable = true;
          port = ports.node;
          enabledCollectors = [
            "systemd"
            "textfile"
          ];
          extraFlags = map (dir: "--collector.textfile.directory=${dir}") cfg.textfileDirectories;
        };

        services.vector = {
          enable = true;
          journaldAccess = true;
          settings = generated.vector;
        };

        networking.firewall.interfaces.${cfg.interface}.allowedTCPPorts = [
          ports.node
          ports.vector
        ];
      }

      (lib.mkIf isNode {
        services.prometheus = {
          enable = true;
          port = ports.prometheus;
          configText = builtins.toJSON (generated.prometheus (json "phoebe-rules.yml" generated.rules));
          retentionTime = cfg.node.retention.metrics;
          extraFlags = lib.optional (
            cfg.node.retention.metricsSize != null
          ) "--storage.tsdb.retention.size=${cfg.node.retention.metricsSize}";
        };

        services.prometheus.alertmanager = {
          enable = true;
          port = ports.alertmanager;
          configText = builtins.toJSON generated.alertmanager;
          clusterPeers = generated.peers;
          extraFlags = [
            "--cluster.listen-address=0.0.0.0:${toString ports.cluster}"
            # gossip names the address the other nodes reach, which on a
            # machine with several interfaces alertmanager can't guess
            "--cluster.advertise-address=${generated.me.address}:${toString ports.cluster}"
          ];
        };

        services.loki = {
          enable = true;
          configuration = generated.loki;
        };

        services.prometheus.exporters.blackbox = {
          enable = true;
          port = ports.blackbox;
          configFile = json "blackbox.yml" generated.blackbox;
        };

        networking.firewall.interfaces.${cfg.interface} = {
          allowedTCPPorts = [
            ports.prometheus
            ports.alertmanager
            ports.cluster
            ports.loki
            ports.blackbox
          ];
          allowedUDPPorts = [ ports.cluster ];
        };
      })
    ]
  );
}
