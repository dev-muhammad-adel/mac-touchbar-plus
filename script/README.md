# Tiny DFR Installation Scripts

This directory contains installation and uninstallation scripts for the tiny-dfr project.

## Scripts

### `install.sh`
Installs tiny-dfr and all its components:
- Builds the project using `cargo build --release`
- Installs main binary (`tiny-dfr`) and helper binaries
- Installs udev rules for device access
- Installs systemd service for automatic startup
- Installs configuration files and icons
- Removes `config.toml` as requested
- Enables and starts the service

### `uninstall.sh`
Removes tiny-dfr and all its components:
- Stops and disables the systemd service
- Removes all installed binaries
- Removes udev rules
- Removes configuration files and icons
- Cleans up empty directories

## Usage

### Installation
```bash
sudo ./script/install.sh
```

### Uninstallation
```bash
sudo ./script/uninstall.sh
```

## Requirements

- Root privileges (use `sudo`)
- Systemd-based Linux distribution
- Internet connection for downloading dependencies

## Dependencies

The install script will automatically detect your package manager and install all required dependencies:

### Build Dependencies
- Rust and Cargo
- pkg-config
- cairo, libinput, freetype, fontconfig, glib2, pango, gdk-pixbuf2, libxml2, librsvg

### Runtime Dependencies
- **Input simulation tools**: wtype (Wayland), ydotool (Wayland alternative), xdotool (X11 fallback)
- **Window management tools**: wlrctl
- **System tools**: procps/procps-ng (pgrep)

### Supported Distributions
- **Arch Linux / Manjaro**: Uses `pacman`
- **Ubuntu / Debian**: Uses `apt`
- **Fedora / RHEL / CentOS**: Uses `dnf`/`yum`
- **openSUSE**: Uses `zypper`
- **Other**: Manual dependency installation required

## What Gets Installed

### Binaries (to `/usr/bin/`)
- `tiny-dfr` - Main application
- `tiny-dfr-helper` - Helper binary
- `tiny-dfr-vlc-helper` - VLC helper binary
- `tiny-dfr-browser-helper` - Browser helper binary

### Configuration Files
- `/usr/share/tiny-dfr/config.json` - Main configuration
- `/usr/share/tiny-dfr/icons/` - Icon files

### System Files
- `/etc/udev/rules.d/99-touchbar-tiny-dfr.rules` - Udev rules for touchbar
- `/etc/udev/rules.d/99-touchbar-seat.rules` - Udev rules for seat
- `/etc/systemd/system/tiny-dfr.service` - Systemd service

## Service Management

After installation, the service will be automatically enabled and started. You can manage it with:

```bash
# Check status
systemctl status tiny-dfr

# Start/stop/restart
systemctl start tiny-dfr
systemctl stop tiny-dfr
systemctl restart tiny-dfr

# Enable/disable auto-start
systemctl enable tiny-dfr
systemctl disable tiny-dfr
```

## Troubleshooting

If you encounter issues:

1. Check the service status: `systemctl status tiny-dfr`
2. View service logs: `journalctl -u tiny-dfr -f`
3. Check udev rules: `udevadm test /sys/class/input/event*`
4. Verify device permissions: `ls -la /dev/input/event*` 