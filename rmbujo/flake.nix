{
  description = "rmbujo — dot-grid bullet journal PDF generator for reMarkable";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.flake-utils.url = "github:numtide/flake-utils";
  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let pkgs = import nixpkgs { inherit system; };
      in {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [ pkgs.rustc pkgs.cargo pkgs.clippy pkgs.rustfmt pkgs.pkg-config ];
          buildInputs = [ pkgs.libiconv pkgs.fontconfig pkgs.poppler-utils pkgs.dejavu_fonts ];
        };
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "rmbujo";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ pkgs.libiconv pkgs.fontconfig ];
        };
      });
}
