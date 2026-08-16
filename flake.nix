{
  description = "PMD Scraper Rust environment with cross compilation to Windows";

  inputs = {
    nixpkgs.url = "github:Nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        # Rust toolchain with all necessary components
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
          ];
          targets = [
            "x86_64-pc-windows-gnu"
            "x86_64-unknown-linux-musl"
          ];
        };

        # The MinGW cross-compilation package set
        mingwPkgs = pkgs.pkgsCross.mingwW64;

      in
      {
        devShells.default = pkgs.mkShell {
          name = "pmd-scraper";

          buildInputs = with pkgs; [
            # Rust toolchain
            rustToolchain

            # Native dependencies for Linux builds
            pkg-config
            openssl

            # Windows cross-compilation toolchain and C libraries
            mingwPkgs.stdenv.cc
            mingwPkgs.windows.pthreads
            mingwPkgs.libdeflate
          ];

          shellHook = ''
            echo "--- PMD Scraper Environment ---"
            echo "Rust 1.90.0 with all development tools"
            echo ""
            rustc --version
            cargo --version
            rust-analyzer --version
            echo ""

            export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="${mingwPkgs.stdenv.cc.targetPrefix}gcc"
            export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_AR="${mingwPkgs.stdenv.cc.bintools.targetPrefix}ar"

            # Force rustc to link against the Nix-provided libdeflate for the Windows target.
            # The libdeflate-sys build.rs script fails to find the library via pkg-config,
            # so we bypass it and pass the linker flags directly.
            export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="-L${mingwPkgs.libdeflate}/lib -ldeflate"
            exec fish
          '';
        };

        # Default package builds the native (Linux) binary
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "pmd_scraper";
          version = "0.1.0";
          src = ./.;
          cargoLock = {
            lockFile = ./Cargo.lock;
          };
        };
      }
    );
}
