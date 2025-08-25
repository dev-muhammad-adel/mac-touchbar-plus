{
  pkgs ? import <nixpkgs> { system = "x86_64-linux"; }
}:

let
  # default rust version was too old for the Cargo.toml
  newRustPlatform.buildRustPackage = pkgs.rustPlatform.buildRustPackage.override {
    rustc = pkgs.rustc;
    cargo = pkgs.cargo;
  };
in
newRustPlatform.buildRustPackage {
  pname = "not-so-tiny-dfr";
  version = "1.0";

  src = ./.;

  # cargoHash = pkgs.lib.fakeHash;
  cargoHash = "sha256-oVOEtyLS+F+t/VlMjGsteiH16NvOgbpoqGBbcIBj2aw=";
  
  nativeBuildInputs = with pkgs; [
    pkg-config
    cargo
    rustc
    # freetype
  ];
  buildInputs = with pkgs; [
    udev
    cairo
    gdk-pixbuf
    glib
    libinput
    librsvg
    libxml2
    pango
    freetype
  ];
  # CARGO_HOME = "./.cargo";
  PKG_CONFIG_PATH = with pkgs; lib.makeSearchPath "lib/pkgconfig" [
    udev
    cairo
    gdk-pixbuf
    glib
    libinput
    librsvg
    libxml2
    pango
    freetype
  ];  
  # build phase handled already
  installPhase = ''
        mkdir -p $out/bin
        cp target/release/tiny-dfr $out/bin/tiny-dfr
        cp target/release/tiny-dfr-focus-window-helper $out/bin/tiny-dfr-focus-window-helper
        cp target/release/tiny-dfr-vlc-helper $out/bin/tiny-dfr-vlc-helper
        cp target/release/tiny-dfr-browser-helper $out/bin/tiny-dfr-browser-helper
        mkdir -p $out/share/tiny-dfr
        cp -r $src/share/tiny-dfr/* $out/share/tiny-dfr/

        # wrapProgram $out/bin/tiny-dfr \
        #   --set TINY_DFR_SHARE "$out/share/tiny-dfr"
    '';
}
