{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        # Read version from Cargo.toml
        cargoToml = builtins.fromTOML (builtins.readFile ./crates/tattoy/Cargo.toml);
        version = cargoToml.package.version;

        nativeBuildInputs = with pkgs; [
          rustc
          cargo
          pkg-config
        ];

        buildInputs = with pkgs; [
          dbus
          xorg.libxcb
        ];

        tattoy = pkgs.rustPlatform.buildRustPackage {
          pname = "tattoy";
          inherit version;

          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
            allowBuiltinFetchGit = true;
          };

          inherit nativeBuildInputs buildInputs;

          meta = with pkgs.lib; {
            description = "Text-based compositor for modern terminals";
            homepage = "https://tattoy.sh";
            license = licenses.mit;
            maintainers = with maintainers; [ vincentbernat ];
            platforms = platforms.linux;
          };
        };
      in
      {
        packages = {
          default = tattoy;
          tattoy = tattoy;
        };

        devShells.default = pkgs.mkShell {
          inherit buildInputs nativeBuildInputs;
        };

        apps.default = {
          type = "app";
          program = "${tattoy}/bin/tattoy";
        };
      });
}
