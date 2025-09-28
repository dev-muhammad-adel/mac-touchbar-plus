# Use Arch Linux as base image
FROM archlinux:latest

# Update system and install only required packages
RUN pacman -Syu --noconfirm && \
    pacman -S --noconfirm \
    rust \
    cargo \
    pkg-config \
    fontconfig \
    freetype2 \
    cairo \
    librsvg \
    libdrm \
    libinput \
    libx11 \
    libxcb \
    libxkbcommon \
    libxkbcommon-x11 \
    wayland \
    dbus \
    systemd \
    udev \
    ttf-dejavu \
    ttf-liberation \
    noto-fonts \
    sudo \
    git \
    base-devel \
    && pacman -Scc --noconfirm

# Create a non-root user for running the application
RUN useradd -m -s /bin/bash -G video,input,audio,render tiny-dfr && \
    echo "tiny-dfr ALL=(ALL) NOPASSWD: ALL" >> /etc/sudoers

# Set working directory
WORKDIR /app

# Copy the project files
COPY . .

# Install systemd service files and udev rules
RUN sudo mkdir -p /etc/systemd/system && \
    sudo cp etc/systemd/system/*.service /etc/systemd/system/ && \
    sudo mkdir -p /etc/udev/rules.d && \
    sudo cp etc/udev/rules.d/*.rules /etc/udev/rules.d/

# Install share files (config and icons)
RUN sudo mkdir -p /usr/share/tiny-dfr && \
    sudo cp -r share/tiny-dfr/* /usr/share/tiny-dfr/ && \
    sudo chmod -R 644 /usr/share/tiny-dfr

# Change ownership of the project directory
RUN chown -R tiny-dfr:tiny-dfr /app

# Switch to the non-root user
USER tiny-dfr

# Build the project
RUN cargo build --release

# Install the binaries to system locations
RUN sudo install -D -m 755 target/release/tiny-dfr /usr/bin/tiny-dfr && \
    sudo install -D -m 755 target/release/tiny-dfr-focus-window-helper /usr/bin/tiny-dfr-focus-window-helper && \
    sudo install -D -m 755 target/release/tiny-dfr-media-helper /usr/bin/tiny-dfr-media-helper && \
    sudo install -D -m 755 target/release/tiny-dfr-browser-helper /usr/bin/tiny-dfr-browser-helper && \
    sudo install -D -m 755 target/release/tiny-dfr-background-service-helper /usr/bin/tiny-dfr-background-service-helper

# Create a script to run the application with proper device access
RUN echo '#!/bin/bash' > run.sh && \
    echo 'if [ -f /.dockerenv ]; then' >> run.sh && \
    echo '    echo "Running in Docker container"' >> run.sh && \
    echo '    sudo chmod 666 /dev/dri/card* 2>/dev/null || true' >> run.sh && \
    echo '    sudo chmod 666 /dev/input/event* 2>/dev/null || true' >> run.sh && \
    echo '    sudo chmod 666 /dev/uinput 2>/dev/null || true' >> run.sh && \
    echo '    if ! pgrep -x "dbus-daemon" > /dev/null; then' >> run.sh && \
    echo '        sudo dbus-daemon --system --fork' >> run.sh && \
    echo '    fi' >> run.sh && \
    echo 'fi' >> run.sh && \
    echo 'exec /usr/bin/tiny-dfr "$@"' >> run.sh && \
    chmod +x run.sh

# Expose any necessary ports (if the application uses any)
# EXPOSE 8080

# Set the default command
CMD ["./run.sh"]
