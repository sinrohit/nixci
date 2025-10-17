{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-25.05";
    flake-parts.url = "github:hercules-ci/flake-parts";
    systems.url = "github:nix-systems/default";

    # Rust
    rust-flake.url = "github:juspay/rust-flake";
    rust-flake.inputs.nixpkgs.follows = "nixpkgs";
    cargo-doc-live.url = "github:srid/cargo-doc-live";
    process-compose-flake.url = "github:Platonic-Systems/process-compose-flake";
    git-hooks-nix = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.nixpkgs-stable.follows = "nixpkgs";
    };

    # App dependenciues
    devour-flake.url = "github:srid/devour-flake";
    devour-flake.flake = false;
  };

  outputs =
    inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
      ];
      imports = [
        inputs.flake-parts.flakeModules.easyOverlay
        inputs.rust-flake.flakeModules.default
        inputs.rust-flake.flakeModules.nixpkgs
        # inputs.cargo-doc-live.flakeModule
        # inputs.process-compose-flake.flakeModule
        inputs.git-hooks-nix.flakeModule
      ];

      perSystem =
        {
          config,
          self',
          pkgs,
          lib,
          system,
          ...
        }:
        {
          rust-project.crates."nixci".crane.args = {
            nativeBuildInputs = with pkgs; [
              libiconv
              pkg-config
            ];
            buildInputs = lib.optionals pkgs.stdenv.isLinux [
              pkgs.openssl
            ];
            DEVOUR_FLAKE = inputs.devour-flake;
            NIX_EVAL_JOBS = lib.getExe pkgs.nix-eval-jobs;
          };

          pre-commit = {
            check.enable = true;
            settings = {
              hooks = {
                nixfmt-rfc-style.enable = true;
                rustfmt.enable = true;
              };
            };
          };

          # Flake outputs
          packages.default = self'.packages.nixci.overrideAttrs (oa: {
            nativeBuildInputs = (oa.nativeBuildInputs or [ ]) ++ [
              pkgs.installShellFiles
              pkgs.nix
            ];
            postInstall = ''
              installShellCompletion --cmd nixci \
                --bash <($out/bin/nixci completion bash) \
                --zsh <($out/bin/nixci completion zsh) \
                --fish <($out/bin/nixci completion fish)
            '';
          });
          overlayAttrs.nixci = self'.packages.default;

          devShells.default = pkgs.mkShell {
            name = "nixci";
            inputsFrom = [
              self'.devShells.rust
              config.pre-commit.devShell
            ];
            shellHook = ''
              export DEVOUR_FLAKE=${inputs.devour-flake}
              export NIX_EVAL_JOBS=${lib.getExe pkgs.nix-eval-jobs}
            '';
            packages = [
              pkgs.cargo-watch
              #config.process-compose.cargo-doc-live.outputs.package
            ];
          };
        };
    };
}
