{
  description = "rmapps — reMarkable tools workspace (rmfiles, rm-cloud, rmbujo, rmreader, rmdigest, rmapps)";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.flake-utils.url = "github:numtide/flake-utils";
  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in {
        devShells.default = pkgs.mkShell {
          # python3: the `stylo` build script (pulled in transitively via
          # fulgur/blitz, used by rmreader/rmdigest) generates CSS-property code
          # from .mako.rs templates and shells out to python3. Declared here so the
          # dev shell is self-contained rather than relying on a system Python.
          # reqwest uses rustls-tls (pure Rust), so no OpenSSL/pkg-config needed.
          # typst is pure Rust with fonts vendored via typst-assets, so no system
          # fonts/CSS toolchain are required for building.
          nativeBuildInputs = [ pkgs.rustc pkgs.cargo pkgs.clippy pkgs.rustfmt pkgs.python3 ];
          # Cloud sync is native (rm-cloud); no external rmapi binary is needed.
          # poppler-utils: pdftoppm for the visual-regression tests.
          # fontconfig + dejavu_fonts: needed by rmreader/rmdigest test rendering.
          buildInputs = [ pkgs.libiconv pkgs.fontconfig pkgs.poppler-utils pkgs.dejavu_fonts ];
        };
        packages.rmapps = pkgs.rustPlatform.buildRustPackage {
          pname = "rmapps";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          # Build only the rmapps binary from the workspace.
          cargoBuildFlags = [ "-p" "rmapps" ];
          # python3: stylo build script (see dev shell note), needed to compile
          # rmreader/rmdigest. poppler-utils: pdftoppm for the visual-regression
          # tests that buildRustPackage runs in its check phase.
          nativeBuildInputs = [ pkgs.python3 pkgs.poppler-utils ];
          buildInputs = [ pkgs.libiconv pkgs.fontconfig ];
        };
        packages.default = self.packages.${system}.rmapps;
      });
}
