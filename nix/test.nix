# two nodes and a machine that only sends, on one network standing in for
# the tailnet. the agent runs a demo unit and a web server every node
# probes. each node runs the app, which its alertmanager posts to, and a
# push service on loopback that the app pushes to.
{ self }:
let
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
    { pkgs, ... }:
    let
      fakePush = pkgs.writers.writePython3Bin "fake-push" {
        libraries = [ pkgs.python3Packages.cryptography ];
        flakeIgnore = [ "E501" ];
      } (builtins.readFile ./fake-push.py);
    in
    {
      environment.systemPackages = [ pkgs.jq ];

      services.phoebe.web = {
        enable = true;
        origin = "https://phoebe.test";
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
        environment.systemPackages = [ pkgs.jq ];
      }
    );
  };

  testScript = ''
    import json
    import shlex

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

    def snapshot(node):
        """the first snapshot the node's app streams"""
        out = node.succeed(
            "curl -sN --max-time 3 localhost:7980/api/stream | sed -n 's/^data: //p' | head -n 1"
        )
        return json.loads(out) if out.strip() else None

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
            "curl -sf localhost:7980/api/alerts | jq -c 'map(select(.service == \"demo\"))[0]'"
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
                f"curl -sf localhost:7980/api/alerts/{alert['id']} | jq -e {shlex.quote(notified)}",
                timeout=120,
            )

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
