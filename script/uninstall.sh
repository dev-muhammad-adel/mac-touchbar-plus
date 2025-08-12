#!/bin/bash

# Tiny DFR Uninstall Script
# This script removes tiny-dfr and all its components

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

print_status "Uninstalling tiny-dfr..."

# Stop and disable the service first
print_status "Stopping and disabling tiny-dfr service..."
if systemctl is-active --quiet tiny-dfr.service; then
    systemctl stop tiny-dfr.service
fi

if systemctl is-enabled --quiet tiny-dfr.service; then
    systemctl disable tiny-dfr.service
fi

# Remove systemd service file
print_status "Removing systemd service..."
rm -f /etc/systemd/system/tiny-dfr.service
systemctl daemon-reload

# Remove udev rules
print_status "Removing udev rules..."
rm -f /etc/udev/rules.d/99-touchbar-tiny-dfr.rules
rm -f /etc/udev/rules.d/99-touchbar-seat.rules

# Reload udev rules
print_status "Reloading udev rules..."
udevadm control --reload-rules
udevadm trigger

# Remove binaries
print_status "Removing binaries..."
rm -f /usr/bin/tiny-dfr
rm -f /usr/bin/tiny-dfr-helper
rm -f /usr/bin/tiny-dfr-vlc-helper
rm -f /usr/bin/tiny-dfr-browser-helper

# Remove share files
print_status "Removing share files..."
rm -f /usr/share/tiny-dfr/config.json
rm -rf /usr/share/tiny-dfr/icons

# Remove empty directories
print_status "Cleaning up empty directories..."
if [ -d /usr/share/tiny-dfr ] && [ -z "$(ls -A /usr/share/tiny-dfr)" ]; then
    rmdir /usr/share/tiny-dfr
fi

print_status "Uninstallation completed successfully!"
print_status "All tiny-dfr components have been removed from the system." 