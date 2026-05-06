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

        packages.default = pkgs.stdenv.mkDerivation {
          pname = "flynt";
          version = "0.6.2";
          src = ./.;

          nativeBuildInputs = [
            rust
            pkgs.pkg-config
            pkgs.cmake
            pkgs.dioxus-cli
          ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            pkgs.wrapGAppsHook3
          ];

          buildInputs = [
            pkgs.openssl
            pkgs.sqlite
          ] ++ linuxBuildInputs ++ pkgs.lib.optionals pkgs.stdenv.isDarwin (with pkgs.darwin.apple_sdk.frameworks; [
            WebKit
            AppKit
            CoreServices
            Security
          ]);

          buildPhase = ''
            # dx build handles asset hashing + bundling
            cd crates/codex-app
            dx build --platform desktop --release
            cd ../..
          '';

          # Pass DIOXUS_PRODUCT_NAME through the GApps wrapper so
          # get_asset_root() resolves to lib/flynt/ at runtime
          preFixup = pkgs.lib.optionalString pkgs.stdenv.isLinux ''
            gappsWrapperArgs+=(--set DIOXUS_PRODUCT_NAME flynt)
          '';

          installPhase = ''
            mkdir -p $out/bin $out/lib/flynt

            # Find the dx output directory
            DX_OUT=""
            for candidate in \
              target/dx/flynt/release/linux/app \
              target/dx/codex-app/release/linux/app; do
              if [ -d "$candidate" ]; then
                DX_OUT="$candidate"
                break
              fi
            done

            if [ -z "$DX_OUT" ]; then
              echo "ERROR: dx build output not found"
              find target/dx/ -type f \( -name "flynt" -o -name "codex-app" \) 2>/dev/null
              exit 1
            fi

            # Copy binary
            BIN="$DX_OUT/flynt"
            [ -f "$BIN" ] || BIN="$DX_OUT/codex-app"
            cp "$BIN" $out/bin/flynt
            chmod +x $out/bin/flynt

            # Copy hashed assets to lib/flynt/ — Dioxus get_asset_root() on Linux
            # checks bin/../lib/$DIOXUS_PRODUCT_NAME/ which survives wrapGAppsHook
            if [ -d "$DX_OUT/assets" ]; then
              cp -r "$DX_OUT/assets" $out/lib/flynt/assets
            fi
          '' + pkgs.lib.optionalString pkgs.stdenv.isLinux ''
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
            cp crates/codex-app/assets/icon.png $out/share/icons/hicolor/512x512/apps/flynt.png
          '';

          meta = with pkgs.lib; {
            description = "Markdown-native notes, kanban, and knowledge graph";
            homepage = "https://github.com/styrene-lab/flynt";
            license = licenses.unfree;
            platforms = platforms.linux ++ platforms.darwin;
          };
        };
      }
    );
}
