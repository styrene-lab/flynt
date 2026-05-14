{
  description = "Flynt — markdown-native notes, kanban, and knowledge graph";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        version = cargoToml.workspace.package.version;

        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

        rust = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        # System libraries needed by wry/tao on Linux
        linuxBuildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux (with pkgs; [
          webkitgtk_4_1
          gtk3
          glib
          xdotool
          libayatana-appindicator
          libsoup_3
          cairo
          pango
          gdk-pixbuf
          atk
          openssl
        ]);

        linuxNativeBuildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux (with pkgs; [
          pkg-config
          cmake
          wrapGAppsHook3
        ]);

        # Environment variables for pkg-config discovery on Linux
        linuxShellHook = pkgs.lib.optionalString pkgs.stdenv.isLinux ''
          export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath linuxBuildInputs}:$LD_LIBRARY_PATH"
          export GIO_MODULE_DIR="${pkgs.glib-networking}/lib/gio/modules"
        '';

        commonNativeBuildInputs = [
          rust
          pkgs.pkg-config
          pkgs.cmake
        ];

        commonBuildInputs = [
          pkgs.openssl
          pkgs.sqlite
        ];

        darwinFrameworks = pkgs.lib.optionals pkgs.stdenv.isDarwin (with pkgs.darwin.apple_sdk.frameworks; [
          WebKit
          AppKit
          CoreServices
          Security
        ]);

        flynt = pkgs.stdenv.mkDerivation {
          pname = "flynt";
          inherit version;
          src = ./.;

          nativeBuildInputs = commonNativeBuildInputs ++ [
            pkgs.dioxus-cli
          ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            pkgs.wrapGAppsHook3
          ];

          buildInputs = commonBuildInputs ++ linuxBuildInputs ++ darwinFrameworks;

          buildPhase = ''
            # dx build handles asset hashing + bundling.
            cd crates/flynt-app
            dx build --platform desktop --release
            cd ../..
          '';

          # Pass DIOXUS_PRODUCT_NAME through the GApps wrapper so
          # get_asset_root() resolves to lib/flynt/ at runtime.
          preFixup = pkgs.lib.optionalString pkgs.stdenv.isLinux ''
            gappsWrapperArgs+=(--set DIOXUS_PRODUCT_NAME flynt)
          '';

          installPhase = pkgs.lib.optionalString pkgs.stdenv.isLinux ''
            mkdir -p $out/bin $out/lib/flynt

            DX_OUT="target/dx/flynt/release/linux/app"
            if [ ! -d "$DX_OUT" ]; then
              echo "ERROR: dx build output not found at $DX_OUT"
              find target/dx/ -type f -name "flynt" 2>/dev/null
              exit 1
            fi

            cp "$DX_OUT/flynt" $out/bin/flynt
            chmod +x $out/bin/flynt

            if [ -d "$DX_OUT/assets" ]; then
              cp -r "$DX_OUT/assets" $out/lib/flynt/assets
            fi

            mkdir -p $out/share/applications
            cat > $out/share/applications/flynt.desktop <<DESKTOP
            [Desktop Entry]
            Name=Flynt
            Comment=Markdown notes, kanban, and knowledge graph
            Exec=$out/bin/flynt
            Icon=$out/share/icons/hicolor/512x512/apps/flynt.png
            Type=Application
            Categories=Office;TextEditor;
            DESKTOP

            mkdir -p $out/share/icons/hicolor/512x512/apps
            cp crates/flynt-app/assets/icon.png $out/share/icons/hicolor/512x512/apps/flynt.png
          '' + pkgs.lib.optionalString pkgs.stdenv.isDarwin ''
            APP="target/dx/flynt/release/macos/Flynt.app"
            if [ ! -d "$APP" ]; then
              echo "ERROR: dx build output not found at $APP"
              find target/dx/ -maxdepth 5 -type d -name "*.app" 2>/dev/null
              exit 1
            fi

            mkdir -p $out/Applications $out/bin
            cp -R "$APP" $out/Applications/Flynt.app
            BIN=$(find "$out/Applications/Flynt.app/Contents/MacOS" -type f -perm -111 | head -1)
            ln -s "$BIN" $out/bin/flynt
          '';

          meta = with pkgs.lib; {
            description = "Markdown-native notes, kanban, and knowledge graph";
            homepage = "https://github.com/styrene-lab/flynt";
            license = licenses.unfree;
            mainProgram = "flynt";
            platforms = platforms.linux ++ platforms.darwin;
          };
        };

        flynt-agent = pkgs.stdenv.mkDerivation {
          pname = "flynt-agent";
          inherit version;
          src = ./.;

          nativeBuildInputs = commonNativeBuildInputs;
          buildInputs = commonBuildInputs;

          buildPhase = ''
            cargo build --release -p flynt-agent
          '';

          installPhase = ''
            mkdir -p $out/bin $out/share/flynt-agent
            cp target/release/flynt-agent $out/bin/flynt-agent
            cp crates/flynt-agent/manifest.toml $out/share/flynt-agent/manifest.toml
          '';

          meta = with pkgs.lib; {
            description = "Flynt project tools agent for Omegon and MCP clients";
            homepage = "https://github.com/styrene-lab/flynt";
            license = licenses.unfree;
            mainProgram = "flynt-agent";
            platforms = platforms.linux ++ platforms.darwin;
          };
        };
      in
      {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [
            rust
            pkgs.dioxus-cli
            pkgs.cargo-watch
          ] ++ linuxNativeBuildInputs;

          buildInputs = [
            pkgs.openssl
            pkgs.sqlite
          ] ++ linuxBuildInputs;

          shellHook = ''
            echo "Flynt dev environment ready ($(rustc --version))"
            ${linuxShellHook}
          '';
        };

        packages = {
          inherit flynt flynt-agent;
          default = flynt;
        };

        apps = {
          flynt = {
            type = "app";
            program = "${flynt}/bin/flynt";
          };
          flynt-agent = {
            type = "app";
            program = "${flynt-agent}/bin/flynt-agent";
          };
          default = {
            type = "app";
            program = "${flynt}/bin/flynt";
          };
        };
      }
    );
}
