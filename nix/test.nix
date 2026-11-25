{ self }:
{
  name = "phoebe";

  nodes.machine = {
    imports = [ self.nixosModules.default ];
    services.phoebe.enable = true;
  };

  testScript =
    { nodes, ... }:
    ''
      machine.wait_for_unit("multi-user.target")

      with subtest("the enabled module installs phoebe"):
          printed = machine.succeed("phoebe").strip()
          assert printed == "phoebe ${nodes.machine.services.phoebe.package.version}", printed
    '';
}
