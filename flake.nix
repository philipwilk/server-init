{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    fenix.url = "github:nix-community/fenix";
  };

  outputs = {
    self,
    flake-utils,
    naersk,
    nixpkgs,
    fenix,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = (import nixpkgs) {
          inherit system;
          overlays = [fenix.overlays.default ];
        };

        naersk' = pkgs.callPackage naersk {};
      in rec {
        defaultPackage = naersk'.buildPackage {
          src = ./.;
        };

        devShell = pkgs.mkShell {
          buildInputs = with pkgs; [
            openssl
          ];
          nativeBuildInputs = with pkgs; [
            nixfmt-rfc-style
            rust-analyzer
            sccache
            pkg-config
            sqlite
            (pkgs.fenix.stable.withComponents [
              "cargo"
              "clippy"
              "rust-src"
              "rustc"
              "rustfmt"
            ])
          ];
        };
      }
    );
}
