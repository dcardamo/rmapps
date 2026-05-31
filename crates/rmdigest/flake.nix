{
  description = "rmdigest — annotation digests for reMarkable PDFs";
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
          # python3: the `stylo` build script (pulled in transitively via
          # fulgur/blitz) generates CSS-property code from .mako.rs templates and
          # shells out to python3. Declared here so the dev shell is self-contained
          # rather than relying on a system Python being on PATH.
          nativeBuildInputs = [ pkgs.rustc pkgs.cargo pkgs.clippy pkgs.rustfmt pkgs.pkg-config pkgs.python3 ];
          # rmapi: reMarkable cloud client, shelled out to by the rmapi deploy
          # backend (v4-patched via overlays/rmapi.nix).
          buildInputs = [ pkgs.libiconv pkgs.fontconfig pkgs.poppler-utils pkgs.dejavu_fonts pkgs.rmapi ];
        };
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "rmdigest";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          # python3: stylo build script (see dev shell note). poppler-utils:
          # provides pdftoppm for the visual-regression tests that buildRustPackage
          # runs in its check phase.
          nativeBuildInputs = [ pkgs.pkg-config pkgs.python3 pkgs.poppler-utils ];
          buildInputs = [ pkgs.libiconv pkgs.fontconfig ];
        };
      });
}
