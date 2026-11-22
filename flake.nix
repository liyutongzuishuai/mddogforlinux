{
  description = "Render Markdown in the terminal as a browser shows them, styled like GitHub by default";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    # nixpkgs-unstable (26.11) dropped x86_64-darwin; 26.05 is the last branch
    # supporting Intel Macs so we pin to that.
    nixpkgs-26_05-darwin.url = "github:NixOS/nixpkgs/nixpkgs-26.05-darwin";
  };

  outputs =
    {
      self,
      nixpkgs,
      nixpkgs-26_05-darwin,
    }:
    let
      inherit (nixpkgs) lib;
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "x86_64-linux"
        "aarch64-linux"
      ];

      mkPkgs =
        system:
        import (if system == "x86_64-darwin" then nixpkgs-26_05-darwin else nixpkgs) {
          inherit system;
          config = {
            allowUnfree = true;
            allowBroken = system == "x86_64-darwin";
          };
        };

      forAllSystems = f: lib.genAttrs systems (system: f (mkPkgs system) system);

      cargoToml = fromTOML (builtins.readFile ./Cargo.toml);
      linuxFonts = pkgs: with pkgs; [ dejavu_fonts liberation_ttf noto-fonts stix-two ];
      fontConfig = pkgs: pkgs.makeFontsConf { fontDirectories = linuxFonts pkgs; };
    in
    {
      packages = forAllSystems (
        pkgs: system: {
          mddog = pkgs.rustPlatform.buildRustPackage {
            pname = cargoToml.package.name;
            version = cargoToml.package.version;

            src = pkgs.lib.cleanSource ./.;

            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            nativeBuildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.makeWrapper ];

            # fontdb reads this file in Rust; no native Fontconfig library is required.
            FONTCONFIG_FILE = if pkgs.stdenv.hostPlatform.isLinux then fontConfig pkgs else null;
            postInstall = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
              wrapProgram $out/bin/mdd --set-default FONTCONFIG_FILE ${fontConfig pkgs}
            '';

            checkFlags = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
              # Requires macOS /usr/bin/defaults (unavailable in the Nix sandbox).
              "--skip=help_follows_an_appearance_change"
            ];

            meta = {
              description = cargoToml.package.description;
              homepage = "https://github.com/qu8n/mddog";
              license = pkgs.lib.licenses.mit;
              platforms = systems;
              mainProgram = "mdd";
            };
          };

          default = self.packages.${system}.mddog;
        }
      );

      devShells = forAllSystems (
        pkgs: system: {
          default = pkgs.mkShell {
            inputsFrom = [ self.packages.${system}.mddog ];
            packages = with pkgs; [
              cargo
              clippy
              rustc
              rustfmt
            ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux (linuxFonts pkgs);
            FONTCONFIG_FILE = if pkgs.stdenv.hostPlatform.isLinux then fontConfig pkgs else null;
          };
        }
      );
    };
}
