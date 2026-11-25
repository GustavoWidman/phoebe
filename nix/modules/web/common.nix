# `phoebe web`'s options and its command line, the same on NixOS and
# nix-darwin; ./nixos.nix and ./darwin.nix run it
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
  web = cfg.web;
in
{
  options.services.phoebe.web = {
    enable = lib.mkEnableOption ''
      `phoebe web`, the fleet's app, on this node. It reads this node's
      Prometheus, Loki and Alertmanager, and takes the Alertmanager's
      notices, pushing a page alert to every subscribed device. Enable it on
      every node: any one of them up serves the app
    '';

    package = mkOption {
      type = types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.phoebe-web;
      defaultText = lib.literalExpression "phoebe.packages.\${system}.phoebe-web";
      description = "The built web app. phoebe itself comes from `services.phoebe.package`.";
    };

    port = mkOption {
      type = types.port;
      default = 7980;
      description = "The port the app is served on, on loopback alone.";
    };

    hookPort = mkOption {
      type = types.port;
      default = 7981;
      description = ''
        The port Alertmanager posts its notices to, on loopback alone, apart
        from the app. `alerting.webhook` points at it.
      '';
    };

    origin = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "https://phoebe.example.com";
      description = ''
        The app's https origin: where a tap on a notification leads, and the
        VAPID subject. Pushes go out only with it and `push`.
      '';
    };

    push = mkOption {
      default = null;
      description = ''
        What a page alert is pushed with, and to whom. Both files are read
        at every send, so changing them needs no restart, and both must be
        readable by the app's user: `phoebe-web` on NixOS, `_phoebe` on
        nix-darwin. When null, nothing is pushed.
      '';
      type = types.nullOr (
        types.submodule {
          options = {
            vapidKeyFile = mkOption {
              type = types.str;
              example = "/run/agenix/phoebe-vapid";
              description = "File holding the VAPID private key, the base64url of a P-256 scalar.";
            };
            subscriptionsFile = mkOption {
              type = types.str;
              example = "/run/agenix/phoebe-push-subscriptions";
              description = ''
                File holding a JSON array of the devices to notify, each a
                browser's `PushSubscription.toJSON()` with a `name` beside
                its `endpoint` and `keys`.
              '';
            };
          };
        }
      );
    };

    command = mkOption {
      type = types.listOf types.str;
      internal = true;
      readOnly = true;
      description = "`phoebe web` as each platform's module runs it.";
    };
  };

  config = lib.mkIf (cfg.enable && web.enable) {
    assertions = [
      {
        assertion = lib.elem cfg.machine cfg.fleet.nodes;
        message = "phoebe: services.phoebe.web reads this machine's stores, so ${cfg.machine} must be one of services.phoebe.fleet.nodes.";
      }
      {
        assertion = (web.origin == null) == (web.push == null);
        message = "phoebe: services.phoebe.web takes origin and push together, or neither to push nothing.";
      }
    ];

    services.phoebe.alerting.webhook = lib.mkDefault "http://127.0.0.1:${toString web.hookPort}/hook";

    services.phoebe.web.command = [
      (lib.getExe cfg.package)
      "web"
      "--fleet"
      "/etc/phoebe/fleet.json"
      "--listen"
      "127.0.0.1:${toString web.port}"
      "--hook"
      "127.0.0.1:${toString web.hookPort}"
      "--web-dir"
      "${web.package}"
    ]
    ++ lib.optionals (web.push != null) [
      "--origin"
      web.origin
      "--push-key-file"
      web.push.vapidKeyFile
      "--push-subscriptions-file"
      web.push.subscriptionsFile
    ];
  };
}
