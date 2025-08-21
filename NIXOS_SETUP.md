# NixOS Setup Guide for tiny-dfr

This guide explains how to set up tiny-dfr on NixOS systems.

## Option 1: Using the NixOS Module (Recommended)

The easiest way to set up tiny-dfr on NixOS is to use the provided NixOS module.

### 1. Import the module

Add this to your `configuration.nix`:

```nix
{ config, pkgs, ... }:

{
  imports = [
    ./nixos-module.nix  # Adjust path as needed
  ];
  
  services.tiny-dfr = {
    enable = true;
    # Optional: customize user/group
    # user = "custom-user";
    # group = "custom-group";
  };
}
```

### 2. Rebuild your system

```bash
sudo nixos-rebuild switch
```

The module will automatically:
- Create a dedicated user and group for tiny-dfr
- Set up the systemd service with proper security settings
- Configure udev rules for device detection
- Install the tiny-dfr package

## Option 2: Development Environment with shell.nix

For development or testing, you can use the provided `shell.nix`:

```bash
# Enter the development environment
nix-shell

# Build the project
cargo build --release

# Run the daemon
sudo ./target/release/tiny-dfr
```

## Option 3: Manual Installation

If you prefer manual installation, you can use the install script:

```bash
# Clone the repository
git clone <repository-url>
cd tiny-dfr

# Run the install script as root
sudo ./script/install.sh
```

**Note**: The install script will attempt to use `nix-env` to install dependencies, but this approach is not recommended for NixOS systems.

## Required Packages

The following packages are needed for tiny-dfr to work:

### Core Dependencies
- `rustc` and `cargo` - Rust toolchain
- `pkg-config` - Build system
- `cairo`, `libinput`, `freetype`, `fontconfig` - Graphics libraries
- `glib`, `pango`, `gdk-pixbuf` - UI libraries
- `libxml2`, `librsvg` - XML and SVG support

### X11 and DRM Libraries
- `libdrm` - Direct Rendering Manager
- `libX11`, `libxcb` - X11 libraries
- Various `libxcb-*` packages for X11 extensions
- `libxkbcommon` - Keyboard handling

### Utilities
- `xdotool` - X11 automation and window management

## Troubleshooting

### Service won't start
Check if the required devices exist:
```bash
ls -la /dev/tiny_dfr*
```

### Permission denied errors
Ensure the tiny-dfr user has access to the required devices:
```bash
# Check device permissions
ls -la /dev/tiny_dfr*

# Check user groups
groups tiny-dfr
```

### Missing libraries
If you encounter missing library errors, ensure all dependencies are properly installed:
```bash
# Check if a library is available
nix-env -qaP | grep <library-name>
```

### Udev rules not working
Reload udev rules after configuration changes:
```bash
sudo udevadm control --reload-rules
sudo udevadm trigger
```

## Security Considerations

The NixOS module includes several security features:
- Runs as a dedicated user with minimal privileges
- Uses systemd security features (PrivateTmp, ProtectSystem, etc.)
- Restricts system calls and file system access
- Only allows access to necessary device files

## Updating

To update tiny-dfr:

1. Pull the latest changes from the repository
2. Rebuild the package: `cargo build --release`
3. Restart the service: `sudo systemctl restart tiny-dfr`

Or if using the NixOS module, update your configuration and rebuild the system.

## Support

For NixOS-specific issues:
- Check the NixOS documentation
- Review the module configuration
- Ensure all required packages are available in your nixpkgs version

For general tiny-dfr issues, refer to the main README.md file. 