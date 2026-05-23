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
          # fontconfig + fonts: inkapp-core embeds fonts via typst-assets (deterministic, no
          # system fonts needed for rendering). The dejavu/noto packages and poppler-utils
          # below are kept for the legacy spike only (typst-readback), which still uses
          # system-font rendering and pdftoppm for verification.
          # rmapi: reMarkable cloud client (v4-patched), for the spike's on-device steps.
          buildInputs = [
            pkgs.libiconv pkgs.fontconfig pkgs.dejavu_fonts pkgs.noto-fonts
            pkgs.poppler-utils pkgs.rmapi
          ];
        };
      });
}
