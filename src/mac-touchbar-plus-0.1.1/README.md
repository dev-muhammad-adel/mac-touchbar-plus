# mac-touchbar-plus

A dynamic function row daemon for Linux systems with touchbar support, providing intelligent context-aware controls and media management.

## Features

### 🎛️ Multi-Layer System

mac-touchbar-plus features a sophisticated multi-layer system that adapts to your current application and context:

#### **Main Layer (Media Layer)**

- **Split Layout Design**: Combines app modules (left 69%) with media controls (right 31%)
- **Smart Media Shortcuts**: Context-aware media controls that adapt to your current application
- **Volume & Brightness Controls**: Quick access to system volume and display brightness
- **Media Playback**: Play, pause, next, previous, and seek controls
- **Microphone Controls**: Quick mute/unmute functionality

#### **Function Key Layer (Fn Layer)**

- **F1-F12 Keys**: Full function key support for system shortcuts
- **Customizable Actions**: Each F-key can be mapped to specific system functions
- **System Integration**: Seamless integration with desktop environments

#### **App Module Layer 2**

- **System Controls**: Brightness, keyboard backlight, volume controls
- **Blank Spaces**: Configurable spacing for custom layouts
- **Modular Design**: Easy to customize and extend

#### **App Module Layer 3**

- **Advanced Controls**: Close, search, microphone, and application grid
- **Media Integration**: Full media playback controls
- **System Management**: Comprehensive system control options

### 🎵 Advanced Media Management

#### **App Modules with Dual Modes**

**Focus Window Mode**

- **Window Detection**: Automatically detects focused applications
- **Context-Aware Controls**: Media controls adapt to the active window
- **Real-time Updates**: Instant response to window focus changes
- **Multi-Window Support**: Handles multiple instances of the same application

**Background Service Mode**

- **MPRIS Integration**: Full MPRIS (Media Player Remote Interfacing Specification) support
- **Service Discovery**: Automatically detects available media services
- **Background Control**: Control media without focusing the application
- **Service Selection**: Choose from available MPRIS services

#### **Supported Media Players**

- **Spotify**: Full integration with native MPRIS support
- **VLC**: Complete media control and status monitoring
- **Dragon Player**: KDE's media player support
- **SMPlayer**: Advanced media player integration
- **Generic MPRIS**: Support for any MPRIS-compatible application

#### **Browser Integration**

- **Multi-Browser Support**: Firefox, Chrome, Chromium, Brave, Edge, Safari, Opera
- **Media Control**: Control web-based media playback
- **Navigation**: Back, forward, and refresh controls
- **Tab Management**: Quick access to browser functions

### 🖥️ Window Manager Support

mac-touchbar-plus supports multiple window managers and desktop environments:

- **X11**: Full X11 window manager support
- **Wayland Compositors**:
  - **Sway**: Complete Sway integration
  - **Hyprland**: Native Hyprland support
  - **GNOME**: GNOME Wayland with WindowMonitorPro extension
  - **Niri**: Niri compositor support
