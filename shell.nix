{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    # Rust toolchain
    rustc
    cargo
    
    # Build tools
    pkg-config
    
    # Graphics and UI libraries
    cairo
    libinput
    freetype
    fontconfig
    glib
    pango
    gdk-pixbuf
    libxml2
    librsvg
    
    # X11 and DRM libraries
    libdrm
    libX11
    libxcb
    libxcb-render
    libxcb-xfixes
    libxcb-shape
    libxcb-keysyms
    libxcb-util
    libxcb-icccm
    libxcb-image
    libxcb-shm
    libxcb-randr
    libxcb-xkb
    libxkbcommon
    libxkbcommon-x11
    libXScrnSaver
    libXtst
    libXi
    libXrandr
    libXinerama
    libXcursor
    libXcomposite
    libXdamage
    libXext
    libXfixes
    libXrender
    
    # Utilities
    xdotool
  ];
  
  shellHook = ''
    echo "tiny-dfr development environment loaded"
    echo "Available packages:"
    echo "  - Rust toolchain: rustc, cargo"
    echo "  - Graphics libraries: cairo, libinput, freetype, etc."
    echo "  - X11 libraries: libX11, libxcb, etc."
    echo "  - Build tools: pkg-config"
    echo ""
    echo "You can now run: cargo build --release"
  '';
} 