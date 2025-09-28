#!/bin/bash

# Script to build and/or run tiny-dfr Docker container
# Usage: ./docker-run.sh [build|run|both]
#   build - only build the Docker image
#   run   - only run the existing Docker image  
#   both  - build and run (default)

set -e

# Parse command line arguments
ACTION=${1:-both}

echo "=== tiny-dfr Docker Script ==="
echo "Action: $ACTION"
echo

# Check if Docker is installed
if ! command -v docker &> /dev/null; then
    echo "Error: Docker is not installed. Please install Docker first."
    exit 1
fi

# Build the Docker image (if requested)
if [[ "$ACTION" == "build" || "$ACTION" == "both" ]]; then
    echo "Building Docker image..."
    sudo docker build -t tiny-dfr .

    echo
    echo "=== Build completed successfully! ==="
    echo

    # Check if the image was created
    if sudo docker images | grep -q "tiny-dfr"; then
        echo "✅ Docker image 'tiny-dfr' created successfully"
    else
        echo "❌ Failed to create Docker image"
        exit 1
    fi
fi

# Run the container (if requested)
if [[ "$ACTION" == "run" || "$ACTION" == "both" ]]; then
    # Check if image exists before trying to run
    if ! sudo docker images | grep -q "tiny-dfr"; then
        echo "❌ Docker image 'tiny-dfr' not found. Please build it first with: ./docker-run.sh build"
        exit 1
    fi

    echo
    echo "=== Running Container ==="
    echo

# Detect display type
if [ -n "$WAYLAND_DISPLAY" ]; then
    echo "Detected Wayland display: $WAYLAND_DISPLAY"
    DISPLAY_ARGS="-e WAYLAND_DISPLAY=$WAYLAND_DISPLAY -e XDG_RUNTIME_DIR=/run/user/1000 -v /run/user/1000/wayland-0:/run/user/1000/wayland-0:rw"
elif [ -n "$DISPLAY" ]; then
    echo "Detected X11 display: $DISPLAY"
    DISPLAY_ARGS="-e DISPLAY=$DISPLAY -v /tmp/.X11-unix:/tmp/.X11-unix:rw"
else
    echo "Warning: No display detected. Container may not work properly."
    DISPLAY_ARGS=""
fi

# Run the container
echo "Starting container with device access..."
docker run -it --rm \
    --name tiny-dfr-container \
    --privileged \
    --device /dev/dri:/dev/dri \
    --device /dev/input:/dev/input \
    --device /dev/uinput:/dev/uinput \
    -v /sys:/sys:ro \
    -v /dev:/dev:ro \
    -v /run/dbus:/run/dbus:rw \
    -v /run/systemd:/run/systemd:ro \
    -v /run/udev:/run/udev:ro \
    $DISPLAY_ARGS \
    -e XDG_SESSION_TYPE=${XDG_SESSION_TYPE:-wayland} \
    -e DBUS_SESSION_BUS_ADDRESS=unix:path=/run/dbus/system_bus_socket \
    -e USER=tiny-dfr \
    -e HOME=/home/tiny-dfr \
    -e RUST_LOG=info \
    -e RUST_BACKTRACE=1 \
    --network host \
    --cap-add SYS_ADMIN \
    --cap-add SYS_TTY_CONFIG \
    --cap-add DAC_OVERRIDE \
    --cap-add SETUID \
    --cap-add SETGID \
    --user "1000:1000" \
    --workdir /app \
    tiny-dfr

    echo
    echo "=== Container stopped ==="
fi

# Show usage if invalid action
if [[ "$ACTION" != "build" && "$ACTION" != "run" && "$ACTION" != "both" ]]; then
    echo "❌ Invalid action: $ACTION"
    echo "Usage: ./docker-run.sh [build|run|both]"
    echo "  build - only build the Docker image"
    echo "  run   - only run the existing Docker image"
    echo "  both  - build and run (default)"
    exit 1
fi
