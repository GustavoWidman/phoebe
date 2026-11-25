# two nodes and a machine that only sends, on one network standing in for
# the tailnet. the agent runs a demo unit, a web server every node probes,
# and the webhook receiver both alertmanagers post to.
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
        alerting.webhook = "http://agent:8080/";
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
    {
      environment.systemPackages = [ pkgs.jq ];
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

        # writes every request body it gets on a line of /var/lib/receiver/posts
        systemd.services.receiver = {
          wantedBy = [ "multi-user.target" ];
          serviceConfig.StateDirectory = "receiver";
          script = ''
            exec ${pkgs.python3}/bin/python3 ${pkgs.writeText "receiver.py" ''
              import http.server, json

              class Receiver(http.server.BaseHTTPRequestHandler):
                  def do_POST(self):
                      body = self.rfile.read(int(self.headers["Content-Length"]))
                      with open("/var/lib/receiver/posts", "a") as posts:
                          posts.write(json.dumps(json.loads(body)) + "\n")
                      self.send_response(200)
                      self.end_headers()

              http.server.HTTPServer(("", 8080), Receiver).serve_forever()
            ''}
          '';
        };

        networking.firewall.allowedTCPPorts = [
          8000
          8080
        ];
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

    def posts(alertname, status="firing"):
        """the webhook posts about an alert the receiver got"""
        lines = agent.succeed("cat /var/lib/receiver/posts 2>/dev/null || true").splitlines()
        return [
            post for post in map(json.loads, lines)
            if post["status"] == status and post["commonLabels"].get("alertname") == alertname
        ]

    start_all()
    for node in nodes:
        for unit in ["prometheus", "loki", "alertmanager", "prometheus-blackbox-exporter"]:
            node.wait_for_unit(f"{unit}.service")
    for unit in ["vector", "prometheus-node-exporter", "demo", "web", "receiver"]:
        agent.wait_for_unit(f"{unit}.service")

    with subtest("both prometheus scrape every machine, and see both nodes' probes"):
        for node in nodes:
            prometheus(node, 'up{job="node"} == 1', "map(.metric.machine) | sort == " + json.dumps(sorted(["agent", "node1", "node2"])))
            prometheus(node, 'up{job="vector"} == 1', "length == 3")
            prometheus(node, 'probe_success{service="web"} == 1', 'map(.metric.prober) | sort == ["node1", "node2"]')

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

    with subtest("a failed unit reaches the webhook once, from two alertmanagers"):
        for node in nodes:
            node.wait_until_succeeds(
                "curl -sf localhost:9093/api/v2/status | jq -e '.cluster.peers | length == 2'"
            )
        agent.succeed("systemctl kill --signal=KILL demo.service")
        agent.wait_until_succeeds("systemctl is-failed demo.service")
        agent.wait_until_succeeds(
            "jq -e 'select(.status == \"firing\" and .commonLabels.alertname == \"UnitFailed\")' /var/lib/receiver/posts",
            timeout=300,
        )
        # the second alertmanager in the cluster waits 15 seconds for the
        # first one's notice before it would send its own
        agent.sleep(60)
        sent = posts("UnitFailed")
        assert len(sent) == 1, sent
        labels = sent[0]["commonLabels"]
        assert (labels["service"], labels["severity"], labels["machine"]) == ("demo", "page", "agent"), labels

    with subtest("with one node off, the other fires MachineDown for it"):
        node2.crash()
        prometheus(
            node1,
            'ALERTS{alertname="MachineDown", alertstate="firing"}',
            'map(.metric.machine) == ["node2"]',
            timeout=600,
        )
        agent.wait_until_succeeds(
            "jq -e 'select(.status == \"firing\" and .commonLabels.alertname == \"MachineDown\")' /var/lib/receiver/posts",
            timeout=120,
        )
  '';
}
