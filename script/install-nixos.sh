#!/bin/bash

# Tiny DFR NixOS Install Script
# This script installs tiny-dfr on NixOS systems

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
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

print_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

# Check if running on NixOS
check_nixos() {
    if [ ! -f /etc/os-release ] || ! grep -q "ID=nixos" /etc/os-release; then
        print_error "This script is designed for NixOS systems only."
        print_error "For other distributions, use the main install.sh script."
        exit 1
    fi
    
    print_status "NixOS detected"
}

# Check if running as root
check_root() {
    if [[ $EUID -ne 0 ]]; then
        print_error "This script must be run as root (use sudo)"
        exit 1
    fi
}

# Get the directory where this script is located
get_script_dir() {
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
    print_status "Installing tiny-dfr from: $PROJECT_ROOT"
}

# Check for required tools
check_tools() {
    print_status "Checking for required tools..."
    
    if ! command -v cargo >/dev/null 2>&1; then
        print_error "Cargo not found. Please install Rust first:"
        print_error "  nix-env -iA nixos.rustc nixos.cargo"
        print_error "  or add to your configuration.nix:"
        print_error "    environment.systemPackages = with pkgs; [ rustc cargo ];"
        exit 1
    fi
    
    if ! command -v pkg-config >/dev/null 2>&1; then
        print_error "pkg-config not found. Please install it first:"
        print_error "  nix-env -iA nixos.pkg-config"
        print_error "  or add to your configuration.nix:"
        print_error "    environment.systemPackages = with pkgs; [ pkg-config ];"
        exit 1
    fi
    
    print_status "Required tools found"
}

# Build the project
build_project() {
    print_status "Building tiny-dfr..."
    cd "$PROJECT_ROOT"
    
    # Check if we're in a nix-shell environment
    if [ -n "$IN_NIX_SHELL" ]; then
        print_status "Building in nix-shell environment..."
        cargo build --release
    else
        print_warning "Not in nix-shell environment. Dependencies may be missing."
        print_info "Consider running: nix-shell"
        print_info "Then run this script again."
        
        # Try to build anyway
        if ! cargo build --release; then
            print_error "Build failed. Please run 'nix-shell' first to get all dependencies."
            exit 1
        fi
    fi
}

# Install binaries to user's home directory
install_binaries() {
    print_status "Installing binaries to ~/.local/bin..."
    
    # Create local bin directory
    LOCAL_BIN="$HOME/.local/bin"
    mkdir -p "$LOCAL_BIN"
    
    # Install main binary
    install -D -m 755 target/release/tiny-dfr "$LOCAL_BIN/tiny-dfr"
    
    # Install helper binaries
    install -D -m 755 target/release/tiny-dfr-focus-window-helper "$LOCAL_BIN/tiny-dfr-focus-window-helper"
    install -D -m 755 target/release/tiny-dfr-vlc-helper "$LOCAL_BIN/tiny-dfr-vlc-helper"
    install -D -m 755 target/release/tiny-dfr-browser-helper "$LOCAL_BIN/tiny-dfr-browser-helper"
    
    print_status "Binaries installed to $LOCAL_BIN"
    print_info "Add this to your ~/.bashrc or ~/.zshrc:"
    print_info "  export PATH=\"\$HOME/.local/bin:\$PATH\""
}

# Install configuration files
install_config() {
    print_status "Installing configuration files..."
    
    # Create config directory
    CONFIG_DIR="$HOME/.config/tiny-dfr"
    mkdir -p "$CONFIG_DIR"
    
    # Install config.json
    if [ -f "$PROJECT_ROOT/share/tiny-dfr/config.json" ]; then
        install -D -m 644 "$PROJECT_ROOT/share/tiny-dfr/config.json" "$CONFIG_DIR/config.json"
        print_status "Configuration installed to $CONFIG_DIR/config.json"
    fi
    
    # Copy icons directory
    if [ -d "$PROJECT_ROOT/share/tiny-dfr/icons" ]; then
        cp -r "$PROJECT_ROOT/share/tiny-dfr/icons" "$CONFIG_DIR/"
        print_status "Icons installed to $CONFIG_DIR/icons/"
    fi
}

# Create user systemd service
create_user_service() {
    print_status "Creating user systemd service..."
    
    # Create user systemd directory
    USER_SYSTEMD="$HOME/.config/systemd/user"
    mkdir -p "$USER_SYSTEMD"
    
    # Create the service file
    cat > "$USER_SYSTEMD/tiny-dfr.service" << 'EOF'
[Unit]
Description=Tiny Apple silicon touch bar daemon
After=graphical-session.target
Wants=graphical-session.target

[Service]
Type=simple
ExecStart=%h/.local/bin/tiny-dfr
Restart=always
RestartSec=1
Environment=DISPLAY=%E{DISPLAY}
Environment=WAYLAND_DISPLAY=%E{WAYLAND_DISPLAY}
Environment=XDG_RUNTIME_DIR=%E{XDG_RUNTIME_DIR}

# Security settings
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=%h/.config/tiny-dfr

[Install]
WantedBy=default.target
EOF

    print_status "User systemd service created at $USER_SYSTEMD/tiny-dfr.service"
}

# Setup udev rules for user
setup_udev_rules() {
    print_status "Setting up udev rules for user..."
    
    print_warning "Udev rules need to be set up at the system level."
    print_info "You have two options:"
    print_info ""
    print_info "Option 1: Use the NixOS module (recommended)"
    print_info "  - Copy nixos-module.nix to your system"
    print_info "  - Import it in your configuration.nix"
    print_info "  - Rebuild your system"
    print_info ""
    print_info "Option 2: Manual udev rules setup"
    print_info "  - Copy the udev rules to /etc/udev/rules.d/"
    print_info "  - Reload udev rules"
    print_info ""
    print_info "For now, the service will start but may not detect devices properly."
}

# Enable and start the service
enable_service() {
    print_status "Enabling user systemd service..."
    
    # Reload user systemd
    systemctl --user daemon-reload
    
    # Enable the service
    systemctl --user enable tiny-dfr.service
    
    print_status "Service enabled. To start it, run:"
    print_info "  systemctl --user start tiny-dfr"
    print_info ""
    print_info "To start automatically on login, run:"
    print_info "  systemctl --user enable tiny-dfr"
}

# Show next steps
show_next_steps() {
    print_status "Installation completed!"
    print_info ""
    print_info "Next steps:"
    print_info "1. Add ~/.local/bin to your PATH:"
    print_info "   echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.bashrc"
    print_info "   source ~/.bashrc"
    print_info ""
    print_info "2. Start the service:"
    print_info "   systemctl --user start tiny-dfr"
    print_info ""
    print_info "3. For automatic startup on login:"
    print_info "   systemctl --user enable tiny-dfr"
    print_info ""
    print_info "4. Check service status:"
    print_info "   systemctl --user status tiny-dfr"
    print_info ""
    print_warning "Note: Device detection requires proper udev rules setup."
    print_warning "Consider using the NixOS module for full system integration."
}

# Main installation flow
main() {
    print_status "Starting tiny-dfr NixOS installation..."
    
    check_nixos
    check_root
    get_script_dir
    check_tools
    build_project
    install_binaries
    install_config
    create_user_service
    setup_udev_rules
    enable_service
    show_next_steps
}

# Run main function
main "$@" 