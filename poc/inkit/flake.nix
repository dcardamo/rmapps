{
  description = "inkapp — a framework for building apps for pen-based document devices";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.flake-utils.url = "github:numtide/flake-utils";
  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import ./nix/overlays/rmapi.nix) ];
        pkgs = import nixpkgs { inherit system overlays; };
      in {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [
            pkgs.rustc pkgs.cargo pkgs.clippy pkgs.rustfmt pkgs.pkg-config
          ];
          # fontconfig + fonts: Typst (via typst-kit) loads system fonts at render time.
          # poppler-utils: pdftoppm/pdftotext render spike PDFs for verification.
          # rmapi: reMarkable cloud client (v4-patched), for the spike's on-device steps.
          buildInputs = [
            pkgs.libiconv pkgs.fontconfig pkgs.dejavu_fonts pkgs.noto-fonts
            pkgs.poppler-utils pkgs.rmapi
          ];
        };
      });
}
