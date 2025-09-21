#!/bin/bash

# Tiny DFR Install Script
# This script installs tiny-dfr and its components

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if running as root
if [[ $EUID -ne 0 ]]; then
   print_error "This script must be run as root (use sudo)"
   exit 1
fi

# Get the directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

print_status "Installing tiny-dfr from: $PROJECT_ROOT"

# Function to detect package manager
detect_package_manager() {
    if command -v pacman >/dev/null 2>&1; then
        echo "pacman"
    elif command -v apt >/dev/null 2>&1; then
        echo "apt"
    elif command -v dnf >/dev/null 2>&1; then
        echo "dnf"
    elif command -v yum >/dev/null 2>&1; then
        echo "yum"
    elif command -v zypper >/dev/null 2>&1; then
        echo "zypper"
    elif [ -f /etc/os-release ] && grep -q "ID=nixos" /etc/os-release; then
        echo "nixos"
    else
        echo "unknown"
    fi
}

# Function to install dependencies based on package manager
install_dependencies() {
    local pkg_manager=$(detect_package_manager)
    
    print_status "Detected package manager: $pkg_manager"
    print_status "Installing dependencies..."
    
    case $pkg_manager in
        "pacman")
            # Arch Linux / Manjaro
            pacman -S --needed --noconfirm \
                rust \
                cargo \
                pkg-config \
                cairo \
                libinput \
                freetype2 \
                fontconfig \
                glib2 \
                pango \
                gdk-pixbuf2 \
                libxml2 \
                librsvg \
                xdotool \
                libdrm \
                libx11 \
                libxcb \
                libxcb-render \
                libxcb-xfixes \
                libxcb-shape \
                libxcb-keysyms \
                libxcb-util \
                libxcb-icccm \
                libxcb-image \
                libxcb-shm \
                libxcb-randr \
                libxcb-xkb \
                libxkbcommon \
                libxkbcommon-x11 \
                libxss \
                libxtst \
                libxi \
                libxrandr \
                libxinerama \
                libxcursor \
                libxcomposite \
                libxdamage \
                libxext \
                libxfixes \
                libxrender >/dev/null 2>&1
            ;;
        "apt")
            # Ubuntu / Debian
            apt update
            apt install -y \
                rustc \
                cargo \
                pkg-config \
                libcairo2-dev \
                libinput-dev \
                libfreetype6-dev \
                libfontconfig1-dev \
                libglib2.0-dev \
                libpango1.0-dev \
                libgdk-pixbuf2.0-dev \
                libxml2-dev \
                librsvg2-dev \
                xdotool \
                libdrm-dev \
                libx11-dev \
                libxcb1-dev \
                libxcb-render0-dev \
                libxcb-xfixes0-dev \
                libxcb-shape0-dev \
                libxcb-keysyms1-dev \
                libxcb-util0-dev \
                libxcb-icccm4-dev \
                libxcb-image0-dev \
                libxcb-shm0-dev \
                libxcb-randr0-dev \
                libxcb-xkb1-dev \
                libxkbcommon-dev \
                libxkbcommon-x11-dev \
                libxss-dev \
                libxtst-dev \
                libxi-dev \
                libxrandr-dev \
                libxinerama-dev \
                libxcursor-dev \
                libxcomposite-dev \
                libxdamage-dev \
                libxext-dev \
                libxfixes-dev \
                libxrender-dev
            ;;
        "dnf"|"yum")
            # Fedora / RHEL / CentOS
            dnf install -y \
                rust \
                cargo \
                pkg-config \
                cairo-devel \
                libinput-devel \
                freetype-devel \
                fontconfig-devel \
                glib2-devel \
                pango-devel \
                gdk-pixbuf2-devel \
                libxml2-devel \
                librsvg2-devel \
                xdotool \
                libdrm-devel \
                libX11-devel \
                libxcb-devel \
                libxcb-render0-devel \
                libxcb-xfixes0-devel \
                libxcb-shape0-devel \
                libxcb-keysyms1-devel \
                libxcb-util0-devel \
                libxcb-icccm4-devel \
                libxcb-image0-devel \
                libxcb-shm0-devel \
                libxcb-randr0-devel \
                libxcb-xkb1-devel \
                libxkbcommon-devel \
                libxkbcommon-x11-devel \
                libXScrnSaver-devel \
                libXtst-devel \
                libXi-devel \
                libXrandr-devel \
                libXinerama-devel \
                libXcursor-devel \
                libXcomposite-devel \
                libXdamage-devel \
                libXext-devel \
                libXfixes-devel \
                libXrender-devel
            ;;
        "zypper")
            # openSUSE
            zypper install -y \
                rust \
                cargo \
                pkg-config \
                cairo-devel \
                libinput-devel \
                freetype-devel \
                fontconfig-devel \
                glib2-devel \
                pango-devel \
                gdk-pixbuf2-devel \
                libxml2-devel \
                librsvg2-devel \
                xdotool \
                libdrm-devel \
                libX11-devel \
                libxcb-devel \
                libxcb-render0-devel \
                libxcb-xfixes0-devel \
                libxcb-shape0-devel \
                libxcb-keysyms1-devel \
                libxcb-util0-devel \
                libxcb-icccm4-devel \
                libxcb-image0-devel \
                libxcb-shm0-devel \
                libxcb-randr0-devel \
                libxcb-xkb1-devel \
                libxkbcommon-devel \
                libxkbcommon-x11-devel \
                libXScrnSaver-devel \
                libXtst-devel \
                libXi-devel \
                libXrandr-devel \
                libXinerama-devel \
                libXcursor-devel \
                libXcomposite-devel \
                libXdamage-devel \
                libXext-devel \
                libXfixes-devel \
                libXrender-devel
            ;;
        "nixos")
            # NixOS
            print_status "NixOS detected. Installing dependencies via nix-env..."
            print_warning "Note: On NixOS, you may need to add these packages to your configuration.nix instead of using nix-env"
            print_warning "Consider adding them to your systemPackages or using a development shell with nix-shell"
            
            # Try to install via nix-env, but warn if it fails
            if command -v nix-env >/dev/null 2>&1; then
                nix-env -iA nixos.rustc nixos.cargo nixos.pkg-config nixos.cairo nixos.libinput nixos.freetype nixos.fontconfig nixos.glib nixos.pango nixos.gdk-pixbuf nixos.libxml2 nixos.librsvg nixos.xdotool nixos.libdrm nixos.libX11 nixos.libxcb nixos.libxcb-render nixos.libxcb-xfixes nixos.libxcb-shape nixos.libxcb-keysyms nixos.libxcb-util nixos.libxcb-icccm nixos.libxcb-image nixos.libxcb-shm nixos.libxcb-randr nixos.libxcb-xkb nixos.libxkbcommon nixos.libxkbcommon-x11 nixos.libxss nixos.libxtst nixos.libxi nixos.libxrandr nixos.libxinerama nixos.libxcursor nixos.libxcomposite nixos.libxdamage nixos.libxext nixos.libxfixes nixos.libxrender || {
                    print_warning "nix-env installation failed. Please add the required packages to your configuration.nix"
                    print_warning "Required packages: rustc cargo pkg-config cairo libinput freetype fontconfig glib pango gdk-pixbuf libxml2 librsvg xdotool libdrm libX11 libxcb and related X11 libraries"
                }
            else
                print_warning "nix-env not found. Please add the required packages to your configuration.nix"
                print_warning "Required packages: rustc cargo pkg-config cairo libinput freetype fontconfig glib pango gdk-pixbuf libxml2 librsvg xdotool libdrm libX11 libxcb and related X11 libraries"
            fi
            ;;
        *)
            print_warning "Unknown package manager. Please install dependencies manually:"
            print_warning "- Rust and Cargo"
            print_warning "- pkg-config"
            print_warning "- cairo, libinput, freetype, fontconfig, glib2, pango, gdk-pixbuf2, libxml2, librsvg"
            print_warning "- xdotool (for X11 window management and input simulation)"
            print_warning "- libdrm, libx11, libxcb and related X11 development libraries"
            read -p "Press Enter to continue anyway..."
            ;;
    esac
}

# Install dependencies
install_dependencies

# Build the project first
print_status "Building tiny-dfr..."
cd "$PROJECT_ROOT"
cargo build --release

# Install the main binary
print_status "Installing main binary..."
install -D -m 755 target/release/tiny-dfr /usr/bin/tiny-dfr

# Install helper binaries with correct names
print_status "Installing helper binaries..."
install -D -m 755 target/release/tiny-dfr-focus-window-helper /usr/bin/tiny-dfr-focus-window-helper
install -D -m 755 target/release/tiny-dfr-media-helper /usr/bin/tiny-dfr-media-helper
install -D -m 755 target/release/tiny-dfr-browser-helper /usr/bin/tiny-dfr-browser-helper

# Install udev rules
print_status "Installing udev rules..."
install -D -m 644 "$PROJECT_ROOT/etc/udev/rules.d/99-touchbar-tiny-dfr.rules" /etc/udev/rules.d/99-touchbar-tiny-dfr.rules
install -D -m 644 "$PROJECT_ROOT/etc/udev/rules.d/99-touchbar-seat.rules" /etc/udev/rules.d/99-touchbar-seat.rules

# Install systemd service
print_status "Installing systemd service..."
install -D -m 644 "$PROJECT_ROOT/etc/systemd/system/tiny-dfr.service" /etc/systemd/system/tiny-dfr.service

# Install share files (icons and config)
print_status "Installing share files..."
install -D -m 644 "$PROJECT_ROOT/share/tiny-dfr/config.json" /usr/share/tiny-dfr/config.json

# Copy icons directory
if [ -d "$PROJECT_ROOT/share/tiny-dfr/icons" ]; then
    cp -r "$PROJECT_ROOT/share/tiny-dfr/icons" /usr/share/tiny-dfr/
    print_status "Icons installed to /usr/share/tiny-dfr/icons/"
fi

# Remove config.toml if it exists (as requested)
if [ -f "$PROJECT_ROOT/share/tiny-dfr/config.toml" ]; then
    print_warning "Removing config.toml as requested..."
    rm -f "$PROJECT_ROOT/share/tiny-dfr/config.toml"
fi

# Reload udev rules
print_status "Reloading udev rules..."
udevadm control --reload-rules
udevadm trigger

# Enable and start the service
print_status "Enabling and starting tiny-dfr service..."
systemctl daemon-reload
systemctl enable tiny-dfr.service

# Check if the required devices exist before starting
print_status "Checking for required devices..."
if [ -e "/dev/tiny_dfr_display" ] && [ -e "/dev/tiny_dfr_backlight" ]; then
    print_status "Required devices found, starting service..."
    systemctl start tiny-dfr.service
    print_status "Installation completed successfully!"
    print_status "tiny-dfr service is now running and enabled to start on boot."
    print_status "You can check the service status with: systemctl status tiny-dfr"
else
    print_warning "Required devices not found. Service will start automatically when devices become available."
    print_status "Installation completed successfully!"
    print_status "tiny-dfr service is enabled and will start when the required devices are detected."
    print_status "You can check the service status with: systemctl status tiny-dfr"
fi 