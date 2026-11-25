# two nodes and a machine that only sends, on one network standing in for
# the tailnet. the agent runs a demo unit and a web server every node
# probes. each node runs the app, which its alertmanager posts to, and a
# push service on loopback that the app pushes to. a tailscale CLI of the
# test's own stands in for the tailnet's: it says each machine is in it, and
# whois grants phoebe's capability to every address but the agent's. a
# cloudflared of the test's own does nothing but run, so the tunnel gate
# shows as a process coming and going.
{ self }:
let
  browser =
    pkgs:
    pkgs.writers.writePython3Bin "browser" {
      libraries = [ pkgs.python3Packages.cryptography ];
      flakeIgnore = [ "E501" ];
    } (builtins.readFile ./browser.py);

  names = [
    "node1"
    "node2"
    "agent"
  ];

  machine =
    extra:
    { lib, nodes, ... }:
    {
      imports = [
        self.nixosModules.default
        extra
      ];

      services.phoebe = {
        enable = true;
        interface = "eth1";
        fleet = {
          nodes = [
            "node1"
            "node2"
          ];
          machines = lib.genAttrs names (name: {
            os = "nixos";
            alwaysOn = true;
            address = nodes.${name}.networking.primaryIPAddress;
          });
          services = {
            demo = {
              machine = "agent";
              unit = "demo.service";
            };
            web = {
              machine = "agent";
              unit = "web.service";
              probes = [
                {
                  kind = "http";
                  target = "http://agent:8000/";
                }
              ];
            };
          };
        };
      };

      virtualisation.memorySize = 2048;
      # vector refuses disk buffers that could outgrow their disk
      virtualisation.diskSize = 4096;
    };

  node = machine (
    {
      config,
      pkgs,
      nodes,
      ...
    }:
    let
      fakePush = pkgs.writers.writePython3Bin "fake-push" {
        libraries = [ pkgs.python3Packages.cryptography ];
        flakeIgnore = [ "E501" ];
      } (builtins.readFile ./fake-push.py);
      address = config.networking.primaryIPAddress;
      tailscale = pkgs.writeShellScriptBin "tailscale" ''
        case "$1" in
          status)
            echo '{"BackendState": "Running", "Self": {"TailscaleIPs": ["${address}"], "Online": true}, "Health": null}'
            ;;
          whois)
            if [ "$3" = "${nodes.agent.networking.primaryIPAddress}" ]; then
              echo '{"Node": {"Name": "agent.test.ts.net."}, "UserProfile": {"LoginName": "tagged-devices"}, "CapMap": null}'
            else
              echo '{"Node": {"Name": "phone.test.ts.net."}, "UserProfile": {"LoginName": "operator@test"}, "CapMap": {"r3dlust.com/cap/phoebe": [{}]}}'
            fi
            ;;
          *) exit 1 ;;
        esac
      '';
      cloudflared = pkgs.writeShellScriptBin "cloudflared" ''
        trap 'exit 0' TERM
        while :; do ${pkgs.coreutils}/bin/sleep 1; done
      '';
      # a certificate for this test alone, on the address the node signs in on
      cert = pkgs.runCommand "signin-cert" { nativeBuildInputs = [ pkgs.openssl ]; } ''
        mkdir $out
        openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 3650 \
          -subj /CN=phoebe-test -addext subjectAltName=IP:${address} \
          -keyout $out/key.pem -out $out/cert.pem
      '';
    in
    {
      environment.systemPackages = [
        pkgs.jq
        (browser pkgs)
      ];

      services.phoebe.web = {
        enable = true;
        publicOrigin = "https://phoebe.test";
        # a key for this test alone
        sessionKeyFile = "${pkgs.writeText "session-key" "a3f1c0de9b8e7d6c5b4a39281706f5e4d3c2b1a0918273645546372819a0b1c2"}";
        signin = {
          urls = map (node: "https://${nodes.${node}.networking.primaryIPAddress}:45731") [
            "node1"
            "node2"
          ];
          certFile = "${cert}/cert.pem";
          keyFile = "${cert}/key.pem";
        };
        tailscalePackage = tailscale;
        tunnel = {
          id = "6f1c2a9e-3b7d-4e58-9a0c-d2e4f6a8b1c3";
          credentialsFile = "${pkgs.writeText "tunnel.json" "{}"}";
          package = cloudflared;
        };
        push = {
          # a key for this test alone
          vapidKeyFile = "${pkgs.writeText "vapid" "E-gWtH0VMhoWtOEVcR7eZFJ_04w6zsdfgTUNex6ZxLM"}";
          subscriptionsFile = "${pkgs.writeText "subscriptions.json" (
            builtins.toJSON [
              {
                name = "phone";
                endpoint = "http://127.0.0.1:8088/push/phone";
                # the browser of RFC 8291's appendix A, whose private key
                # the fake push service holds
                keys = {
                  p256dh = "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4";
                  auth = "BTBZMqHH6r4Tts7J_aSIgg";
                };
              }
            ]
          )}";
        };
      };

      systemd.services.fake-push = {
        wantedBy = [ "multi-user.target" ];
        serviceConfig = {
          ExecStart = "${fakePush}/bin/fake-push /var/lib/fake-push/seen.jsonl";
          StateDirectory = "fake-push";
        };
      };
    }
  );
in
{
  name = "phoebe";

  nodes = {
    node1 = node;
    node2 = node;
    agent = machine (
      { pkgs, ... }:
      {
        systemd.services.demo = {
          wantedBy = [ "multi-user.target" ];
          serviceConfig.ExecStart = "${pkgs.coreutils}/bin/sleep infinity";
        };

        systemd.services.web = {
          wantedBy = [ "multi-user.target" ];
          serviceConfig = {
            ExecStart = "${pkgs.python3}/bin/python3 -m http.server 8000";
            WorkingDirectory = "/var/empty";
          };
        };

        networking.firewall.allowedTCPPorts = [ 8000 ];
        environment.systemPackages = [
          pkgs.jq
          (browser pkgs)
        ];
      }
    );
  };

  testScript = ''
    import json
    import shlex
    import time

    nodes = [node1, node2]

    def prometheus(node, query, test, timeout=300):
        """waits until jq's test holds for the result of a query to the node's prometheus"""
        node.wait_until_succeeds(
            "curl -sf --get localhost:9090/api/v1/query "
            + f"--data-urlencode {shlex.quote('query=' + query)} "
            + f"| jq -e {shlex.quote('.data.result | ' + test)}",
            timeout=timeout,
        )

    def logged(node, text, timeout=120):
        """waits until a line holding the text is in the node's loki"""
        query = f'{{machine="agent"}} |= "{text}"'
        node.wait_until_succeeds(
            "curl -sf --get localhost:3100/loki/api/v1/query_range --data-urlencode since=1h "
            + f"--data-urlencode {shlex.quote('query=' + query)} "
            + "| jq -e '.data.result | length > 0'",
            timeout=timeout,
        )

    def api(node, path, args=""):
        """what the node's app answers a browser signed in on it, over its socket"""
        return node.succeed(f"browser curl {shlex.quote(path)} -f {args}")

    def snapshot(node):
        """the first snapshot the node's app streams"""
        # the stream never ends, so curl gives up on it, and fails, after 3 seconds
        out = node.succeed(
            "{ browser curl /api/stream -N --max-time 3 || true; } | sed -n '/^data: /{s///p;q}'"
        )
        return json.loads(out) if out.strip() else None

    def cloudflared(node):
        """whether the node's app runs cloudflared, which it does while the node can serve"""
        return node.execute("pgrep -u phoebe-web -f 'cloudflared tunnel --config /run/phoebe-web/cloudflared.json'")[0] == 0

    def streamed(node, holds, timeout=300):
        """waits until a snapshot the node's app streams holds, and returns it"""
        for _ in range(timeout // 5):
            last = snapshot(node)
            if last is not None and holds(last):
                return last
            node.sleep(5)
        node.log(node.succeed("journalctl -u phoebe-web --no-pager | tail -n 30"))
        raise AssertionError(f"{node.name} never streamed it; the last was {json.dumps(last)}")

    def pushes():
        """every push the nodes' push services got, with the node that got it"""
        seen = []
        for node in nodes:
            lines = node.succeed("cat /var/lib/fake-push/seen.jsonl 2>/dev/null || true").splitlines()
            seen += [(node.name, json.loads(line)) for line in lines]
        return seen

    start_all()
    for node in nodes:
        for unit in ["prometheus", "loki", "alertmanager", "prometheus-blackbox-exporter", "phoebe-web", "fake-push"]:
            node.wait_for_unit(f"{unit}.service")
    for unit in ["vector", "prometheus-node-exporter", "demo", "web"]:
        agent.wait_for_unit(f"{unit}.service")

    with subtest("a node signs in a granted tailnet peer, and nobody else"):
        for node in nodes:
            address = node.succeed("ip -4 -o addr show dev eth1 | awk '{print $4}' | cut -d/ -f1").strip()
            node.wait_until_succeeds(f"browser signin https://{address}:45731 > /tmp/viewer", timeout=60)
            viewer = json.loads(node.succeed("cat /tmp/viewer"))
            assert (viewer["login"], viewer["device"]) == ("operator@test", "phone"), viewer
        # the stream and every route but hello ask for a session
        node1.succeed("curl -s --unix-socket /run/phoebe-web/origin.sock http://phoebe/api/hello | jq -e '.signin | length == 2'")
        status = node1.succeed("curl -s -o /dev/null -w '%{http_code}' --unix-socket /run/phoebe-web/origin.sock http://phoebe/api/alerts")
        assert status == "401", status
        api(node1, "/api/alerts")
        refused = agent.fail("browser signin https://node1:45731")
        assert "403 the tailnet policy grants no access" in refused, refused
        # nothing but the app's own user opens the socket
        node1.succeed("stat -c '%U %a' /run/phoebe-web/origin.sock | grep -qx 'phoebe-web 600'")

    with subtest("both prometheus scrape every machine, and see both nodes' probes"):
        for node in nodes:
            prometheus(node, 'up{job="node"} == 1', "map(.metric.machine) | sort == " + json.dumps(sorted(["agent", "node1", "node2"])))
            prometheus(node, 'up{job="vector"} == 1', "length == 3")
            prometheus(node, 'probe_success{service="web"} == 1', 'map(.metric.prober) | sort == ["node1", "node2"]')

    with subtest("each node's app streams every machine up, and every probe from both nodes"):
        for node in nodes:
            streamed(
                node,
                lambda fleet: all(m["state"]["kind"] == "up" for m in fleet["machines"])
                and all(n["prometheus"] and n["loki"] and n["alertmanager"] and n["blackbox"] for n in fleet["nodes"])
                and all(
                    [v["node"] for v in p["views"]] == ["node1", "node2"]
                    for s in fleet["services"] for p in s["probes"]
                )
                and all(s["unit"]["state"] == "active" for s in fleet["services"]),
            )

    with subtest("a line logged on the agent reaches both lokis"):
        agent.succeed("logger -t phoebe-test reaches-both")
        for node in nodes:
            logged(node, "reaches-both")

    with subtest("lines logged while a loki is down reach it once it is back"):
        node2.systemctl("stop loki.service")
        agent.succeed("logger -t phoebe-test while-node2-was-down")
        logged(node1, "while-node2-was-down")
        # the agent holds the line for node2 in node2's own buffer
        prometheus(node1, 'vector_buffer_size_events{machine="agent", component_id="loki_node2"} > 0', "length == 1")
        node2.systemctl("start loki.service")
        node2.wait_for_unit("loki.service")
        logged(node2, "while-node2-was-down")

    with subtest("a failed unit fires, seen by both nodes, and pushes once"):
        for node in nodes:
            node.wait_until_succeeds(
                "curl -sf localhost:9093/api/v2/status | jq -e '.cluster.peers | length == 2'"
            )
        agent.succeed("systemctl kill --signal=KILL demo.service")
        agent.wait_until_succeeds("systemctl is-failed demo.service")
        for node in nodes:
            streamed(
                node,
                lambda fleet: [
                    a for a in fleet["alerts"]
                    if (a["rule"], a["service"], a["state"]["kind"]) == ("UnitFailed", "demo", "firing")
                    and a["seen_by"] == ["node1", "node2"]
                ] != [],
            )
        alert = json.loads(node1.succeed(
            "browser curl /api/alerts -f | jq -c 'map(select(.service == \"demo\"))[0]'"
        ))
        assert alert["summary"] == "demo failed on agent", alert

        for _ in range(60):
            if pushes():
                break
            node1.sleep(5)
        # the second alertmanager in the cluster waits 15 seconds for the
        # first one's notice before it would send its own
        node1.sleep(60)
        sent = pushes()
        assert len(sent) == 1, sent
        by, push = sent[0]
        assert push["path"] == "/push/phone", push
        assert push["claims"]["aud"] == "http://127.0.0.1:8088", push
        assert push["claims"]["sub"] == "https://phoebe.test", push
        assert (push["ttl"], push["urgency"]) == ("86400", "high"), push
        assert push["topic"], push
        notification = push["message"]["notification"]
        assert notification["navigate"] == f"https://phoebe.test/alerts/{alert['id']}", notification
        assert notification["tag"] == alert["id"], notification
        assert notification["title"] == "demo failed on agent", notification

    with subtest("both nodes' timelines show who pushed it, from their own loki"):
        notified = (
            '.timeline | map(select(.kind == "notified" and .resolved == false'
            + f' and .by == "{by}")) | length == 1'
        )
        for node in nodes:
            node.wait_until_succeeds(
                f"browser curl /api/alerts/{alert['id']} -f | jq -e {shlex.quote(notified)}",
                timeout=120,
            )

    with subtest("a node is in the tunnel only while its stores answer"):
        node1.wait_until_succeeds("journalctl -u phoebe-web | grep -q 'ready to serve'", timeout=60)
        assert cloudflared(node1), "node1 is ready, but runs no cloudflared"
        node1.systemctl("stop loki.service")
        stopped = time.monotonic()
        # five failed probes two seconds apart, then cloudflared's way out
        node1.wait_until_succeeds("! pgrep -u phoebe-web -f 'cloudflared tunnel'", timeout=30)
        took = time.monotonic() - stopped
        assert 8 <= took <= 25, f"the tunnel closed {took:.0f}s after loki stopped"
        node1.succeed("journalctl -u phoebe-web | grep -q \"not ready to serve: this node's loki does not answer\"")
        node1.succeed("journalctl -u phoebe-web | grep -q 'leaving the tunnel'")
        assert cloudflared(node2), "node2 left the tunnel with node1"
        node1.systemctl("start loki.service")
        node1.wait_until_succeeds("pgrep -u phoebe-web -f 'cloudflared tunnel'", timeout=60)

    with subtest("with one node off, the other fires MachineDown for it"):
        node2.crash()
        prometheus(
            node1,
            'ALERTS{alertname="MachineDown", alertstate="firing"}',
            'map(.metric.machine) == ["node2"]',
            timeout=600,
        )
        node1.wait_until_succeeds(
            "jq -e 'select(.message.notification.title == \"node2 is down\")' /var/lib/fake-push/seen.jsonl",
            timeout=120,
        )
  '';
}
