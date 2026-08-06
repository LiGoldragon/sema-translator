{
  description = "sema-translator — authority-approved bootstrap translation";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-build = {
      url = "github:LiGoldragon/rust-build";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-build }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        rust = rust-build.lib.${system}.fromPkgs pkgs;
        inherit (rust) craneLib toolchain;
        src = rust.cleanSource { root = ./.; };
        commonArguments = { inherit src; strictDeps = true; };
        cargoArtifacts = craneLib.buildDepsOnly commonArguments;
      in
      {
        packages.default = craneLib.buildPackage (commonArguments // {
          inherit cargoArtifacts;
        });
        checks = {
          build = craneLib.cargoBuild (commonArguments // {
            inherit cargoArtifacts;
          });
          test = craneLib.cargoTest (commonArguments // {
            inherit cargoArtifacts;
          });
          sole-bootstrap-surface = pkgs.runCommand "sema-translator-sole-bootstrap-surface" { } ''
            test "$(find ${src}/src -maxdepth 1 -type f -name '*.rs' -printf '%f\n' | sort | tr '\n' ' ')" = "bootstrap.rs lib.rs "
            test "$(find ${src}/tests -maxdepth 1 -type f -name '*.rs' -printf '%f\n' | sort | tr '\n' ' ')" = "bootstrap.rs dependency_boundary.rs "
            ! grep -R -E 'mod (authorization|runtime|store|wire)|sema_engine|tokio::|AUTHORITY_ROUTE|DAEMON_BINARY_NAME' ${src}/src
            touch $out
          '';
          doc = craneLib.cargoDoc (commonArguments // {
            inherit cargoArtifacts;
            RUSTDOCFLAGS = "-D warnings";
          });
          fmt = craneLib.cargoFmt { inherit src; };
          clippy = craneLib.cargoClippy (commonArguments // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- -D warnings";
          });
        };
        devShells.default = pkgs.mkShell {
          name = "sema-translator";
          packages = [ pkgs.jujutsu toolchain ];
        };
      });
}
