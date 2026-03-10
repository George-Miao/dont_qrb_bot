{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    {
      nixpkgs,
      rust-overlay,
      flake-utils,
      crane,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        rust = pkgs.rust-bin.selectLatestNightlyWith (
          toolchain:
          toolchain.default.override {
            extensions = [ "rust-src" ];
          }
        );
        craneLib = (crane.mkLib pkgs).overrideToolchain (p: rust);
        buildInputs = with pkgs; [
          openssl
          pkg-config
          rust
        ];

        dont_qrb_bot = craneLib.buildPackage {
          src = ./.;
          pname = "dont_qrb_bot";
          strictDeps = true;
          doCheck = false;
          nativeBuildInputs = buildInputs;
        };

        docker = pkgs.dockerTools.buildLayeredImage {
          name = "dont_qrb_bot";
          created = "now";
          tag = "latest";
          config = {
            Cmd = [ "${dont_qrb_bot}/bin/dont_qrb_bot" ];
          };
        };
      in
      with pkgs;
      {
        packages = {
          inherit docker dont_qrb_bot;
          default = dont_qrb_bot;
        };

        apps.default = flake-utils.lib.mkApp {
          drv = dont_qrb_bot;
        };

        devShells.default = mkShell {
          inherit buildInputs;
        };
      }
    );
}
