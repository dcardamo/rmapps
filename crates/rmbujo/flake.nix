{
  description = "rmbujo — dot-grid bullet journal PDF generator for reMarkable";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.flake-utils.url = "github:numtide/flake-utils";
  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        # flake.nix is at the repo root; the overlay lives at nix/overlays/rmapi.nix.
        overlays = [ (import ./nix/overlays/rmapi.nix) ];
        pkgs = import nixpkgs { inherit system overlays; };
      in {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [ pkgs.rustc pkgs.cargo pkgs.clippy pkgs.rustfmt pkgs.pkg-config ];
          # poppler-utils: pdftoppm for the visual-regression tests. rmapi:
          # reMarkable cloud client, shelled out to by the rmapi deploy backend
          # (v4-patched via overlays/rmapi.nix). Typst renders with vendored fonts,
          # so no system font/CSS toolchain is needed.
          buildInputs = [ pkgs.libiconv pkgs.poppler-utils pkgs.rmapi ];
        };
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "rmbujo";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          # poppler-utils: pdftoppm for the visual-regression tests that
          # buildRustPackage runs in its check phase.
          nativeBuildInputs = [ pkgs.pkg-config pkgs.poppler-utils ];
          buildInputs = [ pkgs.libiconv ];
        };
      });
}
