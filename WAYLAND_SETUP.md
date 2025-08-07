# Wayland Setup for Browser Helper

The browser helper has been updated to work on Wayland without requiring DBus. It uses Wayland-compatible tools for window management and input simulation.

## Required Dependencies

### For Wayland Input Simulation
Install one or more of these tools for sending key sequences:

```bash
# wtype (recommended for Wayland)
sudo pacman -S wtype

# ydotool (alternative for Wayland)
sudo pacman -S ydotool

# xdotool (X11 fallback)
sudo pacman -S xdotool
```

### For Window Management
Install one or more of these tools for detecting active windows:

```bash
# For Sway/Wayland
sudo pacman -S sway

# For i3/X11
sudo pacman -S i3-wm

# For wlroots-based compositors
sudo pacman -S wlrctl
```

## How It Works

The browser helper now uses a multi-layered approach:

1. **Browser Detection**: Uses `pgrep` to detect running browser processes
2. **Window Management**: Tries multiple tools in order:
   - `wlrctl` (Wayland/Sway)
   - `swaymsg` (Sway)
   - `i3-msg` (i3/X11)
   - `xdotool` + `xprop` (X11 fallback)
3. **Input Simulation**: Tries multiple tools in order:
   - `wtype` (Wayland)
   - `ydotool` (Wayland alternative)
   - `xdotool` (X11 fallback)

## Supported Browsers

The helper detects and works with:
- Firefox
- Chrome/Chromium
- Brave Browser
- Microsoft Edge
- LibreWolf
- Waterfox
- Pale Moon
- SeaMonkey
- Epiphany
- Falkon
- Qutebrowser
- Surf

## Troubleshooting

### No browser detected
- Make sure a browser is running
- Check if the browser process name matches the supported list
- Try running `pgrep -f firefox` (or other browser name) to verify detection

### Key sequences not working
- Install `wtype` for Wayland: `sudo pacman -S wtype`
- Install `ydotool` as alternative: `sudo pacman -S ydotool`
- For X11, install `xdotool`: `sudo pacman -S xdotool`

### Window information not available
- For Sway: Install `sway` package
- For i3: Install `i3-wm` package
- For other Wayland compositors: Install `wlrctl`

## Testing

You can test the browser helper manually:

```bash
# Test browser detection
pgrep -f firefox

# Test window management (Sway)
swaymsg -t get_tree

# Test input simulation (Wayland)
wtype ctrl+t
```

The helper will automatically detect which tools are available and use the best ones for your environment. 