# tiny-dfr
The most basic dynamic function row daemon possible

## Features

### Image Caching System
tiny-dfr now includes a sophisticated image caching system that significantly improves performance:

- **LRU Cache**: Least Recently Used caching algorithm for optimal memory management
- **Configurable Limits**: Adjustable cache size and memory limits
- **Performance Metrics**: Real-time cache hit rate and memory usage monitoring
- **Automatic Cleanup**: Intelligent cache cleanup to prevent memory bloat
- **Debug Tools**: Built-in cache debugging and management commands

#### Cache Configuration
- **Max Entries**: 128 cached images (configurable)
- **Memory Limit**: 50MB (configurable)
- **Cleanup Interval**: 5 minutes (configurable)
- **Debug Keys**: Enable/disable cache management shortcuts

#### Cache Management Commands
When debug keys are enabled, you can use these keyboard shortcuts:
- **C Key**: Display current cache state
- **X Key**: Force cache cleanup
- **V Key**: Show detailed cache information

#### Performance Benefits
- **Reduced Disk I/O**: Icons are loaded once and cached in memory
- **Faster Rendering**: No repeated image loading during redraws
- **Lower Latency**: Instant icon display for cached items
- **Memory Efficient**: Automatic cleanup prevents memory leaks

## Dependencies
cairo, libinput, freetype, fontconfig, uinput enabled in kernel config

## License

tiny-dfr is licensed under the MIT license, as included in the [LICENSE](LICENSE) file.

* Copyright The Asahi Linux Contributors

Please see the Git history for authorship information.

tiny-dfr embeds Google's [material-design-icons](https://github.com/google/material-design-icons)
which are licensed under [Apache License Version 2.0](LICENSE.material)
Some icons are derivatives of material-icons, with edits made by kekrby.
