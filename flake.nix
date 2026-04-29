{
  description = "Codyx — markdown-native notes, kanban, and knowledge graph";

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
            echo "Codyx dev environment ready ($(rustc --version))"
            ${linuxShellHook}
          '';
        };

        packages.default = pkgs.stdenv.mkDerivation {
          pname = "codyx";
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

          installPhase = ''
            mkdir -p $out/bin

            # Find the dx output directory
            DX_OUT=""
            for candidate in \
              target/dx/codyx/release/linux/app \
              target/dx/codex-app/release/linux/app; do
              if [ -d "$candidate" ]; then
                DX_OUT="$candidate"
                break
              fi
            done

            if [ -z "$DX_OUT" ]; then
              echo "ERROR: dx build output not found"
              find target/dx/ -type f -name "codyx" -o -name "codex-app" 2>/dev/null
              exit 1
            fi

            # Copy binary
            BIN="$DX_OUT/codyx"
            [ -f "$BIN" ] || BIN="$DX_OUT/codex-app"
            cp "$BIN" $out/bin/codyx
            chmod +x $out/bin/codyx

            # Copy assets alongside binary (Dioxus resolves from exe parent dir)
            if [ -d "$DX_OUT/assets" ]; then
              cp -r "$DX_OUT/assets" $out/bin/assets
            fi
          '' + pkgs.lib.optionalString pkgs.stdenv.isLinux ''
            mkdir -p $out/share/applications
            cat > $out/share/applications/codyx.desktop <<DESKTOP
            [Desktop Entry]
            Name=Codyx
            Comment=Markdown notes, kanban, and knowledge graph
            Exec=$out/bin/codyx
            Icon=$out/share/icons/hicolor/512x512/apps/codyx.png
            Type=Application
            Categories=Office;TextEditor;
            DESKTOP

            mkdir -p $out/share/icons/hicolor/512x512/apps
            cp crates/codex-app/assets/icon.png $out/share/icons/hicolor/512x512/apps/codyx.png
          '';

          meta = with pkgs.lib; {
            description = "Markdown-native notes, kanban, and knowledge graph";
            homepage = "https://github.com/styrene-lab/codex";
            license = licenses.unfree;
            platforms = platforms.linux ++ platforms.darwin;
          };
        };
      }
    );
}
