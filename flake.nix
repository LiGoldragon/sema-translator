{
  description = "sema-translator — sole-writer nested naming authority";

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
        bootstrapArguments = commonArguments // {
          cargoExtraArgs = "--no-default-features --features bootstrap";
        };
        cargoArtifacts = craneLib.buildDepsOnly commonArguments;
        bootstrapCargoArtifacts = craneLib.buildDepsOnly bootstrapArguments;
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
          process = craneLib.cargoTest (commonArguments // {
            inherit cargoArtifacts;
            cargoTestExtraArgs = "--test process";
          });
          bootstrap = craneLib.cargoTest (bootstrapArguments // {
            cargoArtifacts = bootstrapCargoArtifacts;
            cargoTestExtraArgs = "--test bootstrap --test dependency_boundary";
          });
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
