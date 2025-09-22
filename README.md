# mac-touchbar-plus

A dynamic function row daemon for Linux systems with touchbar support, providing intelligent context-aware controls and media management.

![Touchbar Overview](docs/images/features/main-layer-not-supported-focused-app.png)

![background service two players active is chromium](docs/images/features/main-layer-available-two-mpris-background-service-active-chromium.png.png)

![Touchbar Overview](docs/images/features/full-shortcut-media-layer.png)

![Touchbar Overview](docs/images/features/fn-keys-layer.png)
## Features

### 🎛️ Multi-Layer System

mac-touchbar-plus features a sophisticated multi-layer system that adapts to your current application and context:

#### **Main Layer (Media Layer)**

![Main Layer Layout](docs/images/features/main-layer-not-supported-focused-app.png)




- **Split Layout Design**: Combines app modules (left 69%) with media controls (right 31%)
- **Smart Media Shortcuts**: Context-aware media controls that adapt to your current application
- **Volume & Brightness Controls**: Quick access to system volume and display brightness
- **Media Playback**: Play, pause, next, previous, and seek controls
- **Microphone Controls**: Quick mute/unmute functionality

**App Modules with Dual Modes**

![Main Layer Layout](docs/images/features/main-layer-browser-focused-app.png)

![Main Layer Layout](docs/images/features/main-layer-vlc_dragonplayer_smplayer-focused-app.png)

**Focus Window Mode**
- **Window Detection**: Automatically detects focused applications
- **Context-Aware Controls**: Media controls adapt to the active window
- **Real-time Updates**: Instant response to window focus changes
- **Multi-Window Support**: Handles multiple instances of the same application

*Supported modules*
- *Spotify*: Full integration with native MPRIS support
- *VLC*: Complete media control and status monitoring
- *Dragon Player**: KDE's media player support
- *SMPlayer**: Advanced media player integration
- *Multi-Browser Support*: Firefox, Chrome, Chromium, Brave, Edge, Safari, Opera
- *Media Control*: Control web-based media playback
- *Navigation*: Back, forward, and refresh controls
- *Tab Management*: Quick access to browser functions


**Background Service Mode**
- **MPRIS Integration**: Full MPRIS (Media Player Remote Interfacing Specification) support
- **Service Discovery**: Automatically detects available media services
- **Background Control**: Control media without focusing the application
- **Service Selection**: Choose from available MPRIS services


**Context-Aware Examples:**
- ![background service one players active is spotify](docs/images/features/main-layer-available-one-mpris-background-service-spotify.png) - Spotify with MPRIS background service
- ![background service two players active is chromium](docs/images/features/main-layer-available-two-mpris-background-service-active-chromium.png.png) - Chromium with dual MPRIS services

#### **Media Control Layer**
![Media Control Layer](docs/images/features/full-shortcut-media-layer.png)
- **Advanced Controls**: Close, search, microphone, and application grid
- **Media Integration**: Full media playback controls
- **System Management**: Comprehensive system control options



#### **Function Key Layer (Fn Layer)**

![Function Keys Layer](docs/images/features/fn-keys-layer.png)

- **F1-F12 Keys**: Full function key support for system shortcuts
- **Customizable Actions**: Each F-key can be mapped to specific system functions
- **System Integration**: Seamless integration with desktop environments



### 📸 Screenshot Functionality

mac-touchbar-plus includes built-in screenshot capabilities with intelligent user session management:


#### **Screenshot Capture**
- **Keyboard Shortcut**: Press `Ctrl+Shift+6` to capture a screenshot
- **Automatic Naming**: Files are automatically named with timestamp (e.g., `tiny-dfr_2024-01-15_14-30-25.png`)

### 🖥️ Window Manager Support

mac-touchbar-plus supports multiple window managers and desktop environments:

- **X11**: Full X11 window manager support
- **Wayland Compositors**:
  - **Sway**: Complete Sway integration
  - **Hyprland**: Native Hyprland support
  - **GNOME**: GNOME Wayland with WindowMonitorPro extension
  - **Niri**: Niri compositor support
