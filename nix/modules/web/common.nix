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
  isDarwin = pkgs.stdenv.hostPlatform.isDarwin;
  # systemd hands the service its secrets as credentials; launchd reads them where they are
  secret =
    name: file: if isDarwin then file else "/run/credentials/phoebe-web.service/phoebe-web.${name}";
  state = "/var/lib/phoebe-web";
  # /var/run on a Mac empties at boot, and only root could make the directory again
  runtime = if isDarwin then state else "/run/phoebe-web";
in
{
  options.services.phoebe.web = {
    enable = lib.mkEnableOption ''
      `phoebe web`, the fleet's app, on this node. It reads this node's
      Prometheus, Loki and Alertmanager, signs browsers in on the tailnet,
      connects to the cloudflare tunnel while this node can serve, and takes
      the Alertmanager's notices, pushing a page alert to every subscribed
      device. Enable it on every node: any one of them up serves the app
    '';

    package = mkOption {
      type = types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.phoebe-web;
      defaultText = lib.literalExpression "phoebe.packages.\${system}.phoebe-web";
      description = "The built web app. phoebe itself comes from `services.phoebe.package`.";
    };

    publicOrigin = mkOption {
      type = types.str;
      example = "https://phoebe.example.com";
      description = ''
        Where browsers load the app from, the tunnel's public hostname with
        its scheme: the one origin signing in lets in, where a tap on a
        notification leads, and the VAPID subject.
      '';
    };

    sessionKeyFile = mkOption {
      type = types.str;
      example = "/run/agenix/phoebe-session-key";
      description = ''
        File holding the fleet's session keys, 64 hex digits a line, the
        same file on every node so a session from one works on all of them.
        The first key signs new sessions and every key checks them: rotate
        by putting a new key first and dropping the old one a week later, or
        at once to end every session. `openssl rand -hex 32` makes a key.
      '';
    };

    signin = {
      port = mkOption {
        type = types.port;
        default = 45731;
        description = ''
          The port signing in is served on, on this node's tailnet addresses
          alone. coeus web signs in on 45730, so phoebe's sits beside it.
        '';
      };

      domain = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "tail0000.ts.net";
        description = ''
          The tailnet's MagicDNS domain. With it, `urls` names every node of
          the registry as `https://<node>.<domain>:<port>`.
        '';
      };

      urls = mkOption {
        type = types.listOf types.str;
        default = lib.optionals (web.signin.domain != null) (
          map (
            node: "https://${node}.${web.signin.domain}:${toString web.signin.port}"
          ) cfg.fleet.nodes
        );
        defaultText = lib.literalExpression ''map (node: "https://''${node}.''${domain}:''${toString port}") fleet.nodes'';
        example = [
          "https://lab.tail0000.ts.net:45731"
          "https://oracle-1.tail0000.ts.net:45731"
        ];
        description = ''
          Every node's sign-in address. The app asks them all and signs in
          through the first that answers, so list the same ones on every
          node.
        '';
      };

      capability = mkOption {
        type = types.str;
        default = "r3dlust.com/cap/phoebe";
        description = ''
          The app capability a tailnet policy grant gives the nodes that may
          sign in, toward the nodes that serve the app.
        '';
      };

      certFile = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "/var/lib/tailscale-cert/cert.pem";
        description = ''
          This node's `*.ts.net` certificate, with its chain, as PEM. When
          null, phoebe asks tailscaled for one (`tailscale cert`, which needs
          HTTPS enabled for the tailnet) into its state directory, at start
          and every hour after, so it renews itself. A file given here must
          be readable by the app's user, and the app picks up a renewed one
          without restarting.
        '';
      };

      keyFile = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "/var/lib/tailscale-cert/key.pem";
        description = "The private key of `certFile`, as PEM; fetched with it when null.";
      };
    };

    tunnel = mkOption {
      default = null;
      description = ''
        The cloudflare tunnel this node connects to while it can serve:
        tailscale running and in touch with its coordination server and
        relays, and this node's Prometheus, Loki and Alertmanager ready.
        cloudflared runs as the app's child, and nothing reaches the app when
        null. Give every node the same tunnel.
      '';
      type = types.nullOr (
        types.submodule {
          options = {
            id = mkOption {
              type = types.str;
              example = "6f1c2a9e-3b7d-4e58-9a0c-d2e4f6a8b1c3";
              description = "The tunnel's id, from `cloudflared tunnel create`.";
            };
            credentialsFile = mkOption {
              type = types.str;
              example = "/run/agenix/phoebe-tunnel";
              description = "The credentials JSON `cloudflared tunnel create` wrote for the tunnel.";
            };
            package = lib.mkPackageOption pkgs "cloudflared" { };
          };
        }
      );
    };

    hookPort = mkOption {
      type = types.port;
      default = 7981;
      description = ''
        The port Alertmanager posts its notices to, on loopback alone, apart
        from the app. `alerting.webhook` points at it.
      '';
    };

    push = mkOption {
      default = null;
      description = ''
        What a page alert is pushed with, and to whom. The app offers the
        public half of the key to a device turning notifications on, and
        checks its subscription against the list. Both files are read at
        every send and every request, so changing them needs no restart, and
        both must be readable by the app's user: `phoebe-web` on NixOS,
        `_phoebe` on nix-darwin. When null, nothing is pushed and the app
        offers no notifications.
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
            edit = mkOption {
              type = types.nullOr types.str;
              default = null;
              example = "agenix -e secrets/phoebe-push-subscriptions.age";
              description = "The command that edits the subscription list, which the app shows beside a subscription to add.";
            };
          };
        }
      );
    };

    tailscalePackage = mkOption {
      type = types.nullOr types.package;
      default =
        if config.services ? tailscale && config.services.tailscale.enable then
          config.services.tailscale.package
        else
          null;
      defaultText = lib.literalExpression "if config.services.tailscale.enable then config.services.tailscale.package else null";
      description = ''
        The tailscale CLI the app asks who is calling, whether this node is
        in the tailnet, and for its certificate. When null, the `tailscale`
        on the system's PATH, or the Mac app's.
      '';
    };

    command = mkOption {
      type = types.listOf types.str;
      internal = true;
      readOnly = true;
      description = "`phoebe web` as each platform's module runs it.";
    };

    secrets = mkOption {
      type = types.attrsOf types.str;
      internal = true;
      readOnly = true;
      description = "The credentials systemd hands the app, each to the file it holds.";
    };
  };

  config = lib.mkIf (cfg.enable && web.enable) {
    assertions = [
      {
        assertion = lib.elem cfg.machine cfg.fleet.nodes;
        message = "phoebe: services.phoebe.web reads this machine's stores, so ${cfg.machine} must be one of services.phoebe.fleet.nodes.";
      }
      {
        assertion = (web.signin.certFile == null) == (web.signin.keyFile == null);
        message = "phoebe: services.phoebe.web.signin takes certFile and keyFile together, or neither to fetch them.";
      }
      {
        assertion = web.signin.urls != [ ];
        message = "phoebe: services.phoebe.web.signin needs the nodes' sign-in addresses: set domain, or urls.";
      }
    ];

    services.phoebe.alerting.webhook = lib.mkDefault "http://127.0.0.1:${toString web.hookPort}/hook";

    services.phoebe.web.secrets = {
      "phoebe-web.session-key" = web.sessionKeyFile;
    }
    // lib.optionalAttrs (web.tunnel != null) { "phoebe-web.tunnel" = web.tunnel.credentialsFile; };

    services.phoebe.web.command = [
      (lib.getExe cfg.package)
      "web"
      "--fleet"
      "/etc/phoebe/fleet.json"
      "--socket"
      "${runtime}/origin.sock"
      "--hook"
      "127.0.0.1:${toString web.hookPort}"
      "--web-dir"
      "${web.package}"
      "--origin"
      web.publicOrigin
      "--session-key-file"
      (secret "session-key" web.sessionKeyFile)
      "--signin-port"
      (toString web.signin.port)
      "--signin-cert"
      (if web.signin.certFile == null then "${state}/cert.pem" else web.signin.certFile)
      "--signin-key"
      (if web.signin.keyFile == null then "${state}/key.pem" else web.signin.keyFile)
      "--capability"
      web.signin.capability
    ]
    ++ lib.concatMap (url: [
      "--signin-url"
      url
    ]) web.signin.urls
    ++ lib.optionals (web.tunnel != null) [
      "--tunnel-id"
      web.tunnel.id
      "--tunnel-credentials-file"
      (secret "tunnel" web.tunnel.credentialsFile)
      "--cloudflared"
      (lib.getExe web.tunnel.package)
    ]
    ++ lib.optionals (web.push != null) (
      [
        "--push-key-file"
        web.push.vapidKeyFile
        "--push-subscriptions-file"
        web.push.subscriptionsFile
      ]
      ++ lib.optionals (web.push.edit != null) [
        "--push-edit"
        web.push.edit
      ]
    );
  };
}
