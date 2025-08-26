# NixOS Setup Guide for tiny-dfr

This guide explains how to build and launch not-so-tiny-dfr on NixOS systems.

## Option 1: Build with nix-build command from default.nix

For development or testing, you can use the provided `shell.nix`:

```bash
# Build the project (this will put `not-so-tiny-dfr` in your `/nix/store/`)
nix-shell


# Run the daemon
sudo .result/bin/tiny-dfr
```

## Option 2: Development Environment with shell.nix (not recommended)

For development or testing, you can use the provided `shell.nix`:

```bash
# Enter the development environment
nix-shell

# Build the project
cargo build --release

# you can also run:
# `cargo vendor`
# `cargo generate-lockfile`
# then build multible times with
# `cargo build --release --offline`
# which will speed up the build, but
# only do this for development.

# Install helpers to /usr/bin
sudo mkdir /usr/bin/
sudo cp target/release/tiny-dfr-* /usr/bin/

# Copy share files (icons, config) to /usr/share
sudo mkdir -p /usr/share/tiny-dfr/
sudo cp share/tiny-dfr /usr/share

# Run the daemon
sudo ./target/release/tiny-dfr
```

