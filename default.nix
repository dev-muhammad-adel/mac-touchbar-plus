{
  pkgs ? import <nixpkgs> { system = "x86_64-linux"; }
}:

pkgs.stdenv.mkDerivation {
  name = "not-so-tiny-dfr";
  src = ./.;
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
  CARGO_HOME = "./.cargo";
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
  buildPhase = ''
      cp -r $src/* .
      cargo build --release --offline
    '';
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
