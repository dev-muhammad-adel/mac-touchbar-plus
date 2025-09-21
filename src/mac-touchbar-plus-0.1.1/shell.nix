{ pkgs ? import <nixpkgs> { system = "x86_64-linux"; } }:

pkgs.mkShell {
  nativeBuildInputs = with pkgs; [
    pkg-config
    cargo
    rustc
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
  ];

  # Ensure PKG_CONFIG_PATH includes all dependencies
  PKG_CONFIG_PATH = with pkgs; lib.makeSearchPath "lib/pkgconfig" [
    udev
    cairo
    gdk-pixbuf
    glib
    libinput
    librsvg
    libxml2
    pango
  ];
}  
