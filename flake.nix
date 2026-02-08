{
  description = "hyprgrd - grid workspace switcher";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs";
  };

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };

      rustLibSrc = pkgs.rust.packages.stable.rustPlatform.rustLibSrc;

      #  Hyprland plugin (.so) 
      # Must be compiled against the exact Hyprland that will load it
      # (PLUGIN_API_VERSION check is strict).
      mkPlugin = { hyprland ? pkgs.hyprland }:
        pkgs.hyprlandPlugins.mkHyprlandPlugin (finalAttrs: {
          pluginName = "hyprgrd";
          version = "0.1.0";
          src = ./plugin;

          nativeBuildInputs = [ pkgs.cmake pkgs.pkg-config ];

          cmakeFlags = [
            "-DHYPRLAND_HEADERS=${hyprland.dev}/include"
            "-DHYPRGRD_BUILD_TESTS=ON"
          ];

          doCheck = true;
          checkPhase = ''
            runHook preCheck
            ./test_plugin
            ./test_plugin_symbols ./libhyprgrd.so
            runHook postCheck
          '';

          meta = {
            homepage = "https://github.com/adrian-kriegel/hyprgrd";
            description = "hyprgrd";
            license = pkgs.lib.licenses.mit;
            platforms = pkgs.lib.platforms.linux;
            maintainers = with pkgs.lib.maintainers; [adrian-kriegel];
          };
        }
      );

      hyprgrd-plugin = mkPlugin {};

      #  Rust daemon + visualizer 
      hyprgrd = pkgs.rustPlatform.buildRustPackage {
        pname = "hyprgrd";
        version = "0.1.0";
        src = ./.;

        cargoHash = "sha256-r31Bghhieie2VsPsH6gFfnKe8E7QhvZYwYHWHmEvYPE=";

        nativeBuildInputs = with pkgs; [ pkg-config ];

        buildInputs = with pkgs; [
          gtk4
          gtk4-layer-shell
        ];

        # default feature = visualizer-gtk
        buildFeatures = [ "visualizer-gtk" ];
      };

    in {
      lib.mkPlugin = mkPlugin;

      packages.${system} = {
        plugin = hyprgrd-plugin;
        default = hyprgrd;
      };

      devShells.${system}.default = pkgs.mkShell {
        name = "hyprgrd";

        nativeBuildInputs = with pkgs; [
          # Rust
          cargo
          rustc
          pkg-config
          # C++ plugin
          cmake
        ];

        buildInputs = with pkgs; [
          # Hyprland (for the plugin)
          hyprland
          # GTK (for the visualizer)
          gtk4
          gtk4-layer-shell
        ] ++ pkgs.hyprland.buildInputs;

        packages = with pkgs; [
          rustfmt
          clippy
          rustLibSrc
        ];

        shellHook = ''
          export RUSTC=$(which rustc)
          export CARGO=$(which cargo)
          export RUST_SRC_PATH="${rustLibSrc}";
        '';
      };
    };
}
