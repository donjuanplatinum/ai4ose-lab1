{
  description = "AI4OSE OS Kernel Development Environment";

  inputs = {
    nixpkgs-qemu.url = "github:nixos/nixpkgs/nixos-22.05";
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, nixpkgs-qemu, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        pkgs-old = import nixpkgs-qemu { inherit system; };

        rustToolchain = pkgs.rust-bin.nightly."2024-05-02".default.override {
          targets = [ "riscv64gc-unknown-none-elf" ];
          extensions = [ "rust-src" "llvm-tools-preview" "rust-analyzer" ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            rustToolchain
            pkgs.cargo-binutils
            
            pkgs-old.qemu

            pkgs.gdb
            pkgs.python3
            pkgs.gnumake
            pkgs.dtc
          ];

          shellHook = ''
            echo "--- AI4OSE Environment Loaded ---"
            echo "Rust: $(rustc --version)"
            echo "QEMU: $(qemu-system-riscv64 --version | head -n 1)"
            echo "Target: riscv64gc-unknown-none-elf"
          '';
        };
      }
    );
}
