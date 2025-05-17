{
  description = "A Rust CLI tool with Nix Config";

  
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";

    systems.url = "github:nix-systems/default";

    flake-utils.url = "github:numtide/flake-utils";
    flake-utils.inputs.systems.follows = "systems";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils,... } :
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
        };
      in
      {
        # 定义包
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "nix-config-tools";
          version = "0.1.0";
          src = ./.;

          cargoLock.lockFile = ./Cargo.lock;

          # 构建依赖 (编译时)
          nativeBuildInputs = [ 
            rustToolchain
          ];

          # 运行时依赖
          buildInputs = with pkgs; [
          ];
        };

        # 定义可执行入口
        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/nix-config-tools";
        };

        # 开发环境
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            rustToolchain
            cargo
          ];
          
          # 环境变量
          HELLO = "FLAKE-UTILS";
        };
      }
    );
}