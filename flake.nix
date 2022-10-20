{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    let
      inherit (builtins) fromTOML readFile substring;

      cargoToml = fromTOML (readFile ./Cargo.toml);
      version = "${cargoToml.package.version}+${substring 0 8 self.lastModifiedDate}_${self.shortRev or "dirty"}";

      mkSonicServer = { lib, rustPlatform, llvmPackages, clang, ... }:
        rustPlatform.buildRustPackage {
          pname = cargoToml.package.name;
          inherit version;

          src = lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          # TODO: fix and enable tests
          doCheck = false;

          nativeBuildInputs = [
            llvmPackages.libclang
            llvmPackages.libcxxClang
            clang
          ];
          # Needed so bindgen can find libclang.so
          LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
          BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${llvmPackages.libclang.lib}/lib/clang/${lib.getVersion clang}/include";

          postInstall = ''
            mkdir -p $out/etc/
            mkdir -p $out/usr/lib/systemd/system/

            install -Dm444 -t $out/etc/sonic config.cfg 
            substitute \
              ./examples/config/systemd.service $out/usr/lib/systemd/system/sonic-server.service \
              --replace /bin/sonic $out/bin/sonic \
              --replace /etc/sonic.cfg $out/etc/sonic/config.cfg
          '';

          meta = {
            homepage = "https://github.com/valeriansaliou/sonic";
            downloadPage = "https://github.com/valeriansaliou/sonic/releases";
            license = lib.licenses.mpl20;
          };
        };

    in
    {
      overlays.sonic-server = final: prev: {
        sonic-server = prev.callPackage mkSonicServer { };
      };
      overlays.default = self.overlays.sonic-server;
    }
    //
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        sonic-server = pkgs.callPackage mkSonicServer { };
      in
      {
        packages = {
          inherit sonic-server;
          default = sonic-server;
        };

        apps.sonic-server = {
          type = "app";
          program = "${sonic-server}/bin/sonic";
        };
        apps.default = self.apps.${system}.sonic-server;

        devShells.default = pkgs.mkShell {
          inherit (sonic-server) nativeBuildInputs LIBCLANG_PATH BINDGEN_EXTRA_CLANG_ARGS;
          packages = with pkgs; [ cargo rustc rustfmt clippy rust-analyzer ];
          RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
        };
      });
}
