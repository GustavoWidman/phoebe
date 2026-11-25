# phoebe

a lamp for a nix fleet, so i can see what's on fire.

phoebe is the observability app for my nix fleet, at its very first step: a
flake that builds a `phoebe` binary, which prints its version and exits, and
modules for nixos and nix-darwin that put that binary on a machine.

named after phoebe, the titaness of the bright moon, who married coeus. coeus
deploys the fleet, so it seemed only fair that his wife keep an eye on it.

## installing it

the flake exports `nixosModules.default` and `darwinModules.default`. both
declare the same two options:

```nix
# in any machine's configuration, nixos or nix-darwin
services.phoebe = {
  enable = true; # puts phoebe on the path
  package = phoebe.packages.${pkgs.stdenv.hostPlatform.system}.default; # the default
};
```

```bash
$ phoebe
phoebe 0.1.0
```

## file structure

```text
.
├── .github/              # checks for every pull request
├── flake.nix             # the package, the modules, the checks and a dev shell
├── nix/
│   ├── package.nix       # the crane build, with the tests and clippy as checks
│   ├── checks.nix        # every check that isn't the package's own
│   ├── test.nix          # the nixos module, booted in a vm
│   ├── darwin-check.nix  # the nix-darwin module, evaluated on macos
│   └── modules/
│       └── common.nix    # services.phoebe, for both platforms
└── src/
    └── main.rs           # the binary
```

## hacking on it

```bash
nix develop        # the toolchain
nix flake check    # the package, its tests and clippy, the nix-darwin module,
                   # plus a vm test of the nixos module on linux
```

every pull request runs `nix flake check` on x86_64-linux, kvm and vm test
included, and builds every other check on aarch64-linux and aarch64-darwin.
on macos that includes evaluating a whole nix-darwin system and reading what
it would install. commits follow conventional commits, and a pull request raises the version in
`Cargo.toml` unless it's labeled `no-release`.

---

a binary that prints its own version is not much of an observability stack.
it is, however, very observable.
