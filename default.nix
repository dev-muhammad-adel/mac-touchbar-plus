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
   postPatch = ''
      substituteInPlace  src/config.rs --replace-fail "/usr/share/tiny-dfr" "${placeholder "out"}/share/tiny-dfr"
      substituteInPlace  src/helper/manager.rs --replace-fail "/usr/bin/tiny-dfr" "${placeholder "out"}/bin/tiny-dfr"
      substituteInPlace  src/utils/button_images.rs --replace-fail "/usr/share/tiny-dfr" "${placeholder "out"}/share/tiny-dfr"
      substituteInPlace  src/view/browser_screen.rs --replace-fail "/usr/share/tiny-dfr" "${placeholder "out"}/share/tiny-dfr"
    '';
  preBuild = ''
    cargo clean
    '';
  postInstall = ''
        # cp -R $src/share/tiny-dfr/* $out/share/tiny-dfr/
        mkdir -p $out/share/tiny-dfr
        cp -R $src/share/tiny-dfr/* $out/share/tiny-dfr/
  '';
}
