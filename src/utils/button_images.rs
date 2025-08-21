//! Button image types and management for tiny-dfr
//! 
//! This module provides the ButtonImage enum and related structures for
//! handling different types of button images (SVG, PNG, text, blank).
//! Includes a simple per-thread cache for performance optimization.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use cairo::{ImageSurface, Context, Format, Antialias};
use rsvg::{SvgHandle, Loader};
use anyhow::Result;
use icon_loader::{IconLoader, IconFileType};
use lazy_static::lazy_static;
use std::path::PathBuf;
use std::fs::File;

// LRU Cache entry with timestamp for automatic cleanup
#[derive(Debug)]
struct CacheEntry {
    image: CachedImage,
    last_accessed: Instant,
    size_bytes: usize,
}

// Thread-local LRU cache with automatic cleanup
thread_local! {
    static IMAGE_CACHE: Mutex<LruCache> = Mutex::new(LruCache::new());
}

// LRU Cache implementation
struct LruCache {
    entries: HashMap<String, CacheEntry>,
    access_order: VecDeque<String>, // Most recently used first
    max_size: usize,
    max_memory_mb: f64,
    last_cleanup: Instant,
    cleanup_interval: Duration,
}

impl LruCache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            access_order: VecDeque::new(),
            max_size: MAX_CACHE_SIZE,
            max_memory_mb: CACHE_MEMORY_LIMIT_MB,
            last_cleanup: Instant::now(),
            cleanup_interval: Duration::from_secs(CACHE_CLEANUP_INTERVAL),
        }
    }

    fn get(&mut self, key: &str) -> Option<&CachedImage> {
        if let Some(entry) = self.entries.get_mut(key) {
            // Update access time
            entry.last_accessed = Instant::now();
            
            // Move to front of access order
            if let Some(pos) = self.access_order.iter().position(|k| *k == key) {
                self.access_order.remove(pos);
            }
            self.access_order.push_front(key.to_string());
            
            Some(&entry.image)
        } else {
            None
        }
    }

    fn insert(&mut self, key: String, image: CachedImage, size_bytes: usize) {
        // Check if we need to cleanup before inserting
        self.cleanup_if_needed();
        
        // Remove old entry if it exists
        if let Some(_old_entry) = self.entries.remove(&key) {
            // Remove from access order
            if let Some(pos) = self.access_order.iter().position(|k| *k == key) {
                self.access_order.remove(pos);
            }
        }
        
        // Insert new entry
        let entry = CacheEntry {
            image,
            last_accessed: Instant::now(),
            size_bytes,
        };
        
        self.entries.insert(key.clone(), entry);
        self.access_order.push_front(key);
        
        // Enforce size limits
        self.enforce_limits();
    }

    fn cleanup_if_needed(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_cleanup) >= self.cleanup_interval {
            self.cleanup_expired_entries();
            self.last_cleanup = now;
        }
    }

    fn cleanup_expired_entries(&mut self) {
        let now = Instant::now();
        let max_age = Duration::from_secs(300); // 5 minutes max age
        
        let expired_keys: Vec<String> = self.entries
            .iter()
            .filter(|(_, entry)| now.duration_since(entry.last_accessed) > max_age)
            .map(|(key, _)| key.clone())
            .collect();
        
        let expired_count = expired_keys.len();
        
        for key in &expired_keys {
            self.entries.remove(key);
            if let Some(pos) = self.access_order.iter().position(|k| *k == *key) {
                self.access_order.remove(pos);
            }
        }
        
        if expired_count > 0 {
            println!("[CacheCleanup] Removed {} expired entries", expired_count);
        }
    }

    fn enforce_limits(&mut self) {
        let current_memory_mb = self.get_total_memory_mb();
        
        // Remove oldest entries if we exceed memory or size limits
        while (self.entries.len() > self.max_size || current_memory_mb > self.max_memory_mb) 
               && !self.access_order.is_empty() {
            if let Some(oldest_key) = self.access_order.pop_back() {
                if let Some(entry) = self.entries.remove(&oldest_key) {
                    println!("[CacheEviction] Evicted entry '{}' ({} bytes)", oldest_key, entry.size_bytes);
                }
            }
        }
    }

    fn get_total_memory_mb(&self) -> f64 {
        let total_bytes: usize = self.entries.values().map(|entry| entry.size_bytes).sum();
        total_bytes as f64 / 1024.0 / 1024.0
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.access_order.clear();
        println!("[CacheClear] Cache cleared completely");
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn get_stats(&self) -> (usize, usize) {
        let entry_count = self.entries.len();
        let total_memory = self.entries.values().map(|entry| entry.size_bytes).sum();
        (entry_count, total_memory)
    }
}

// Cached image types (only cloneable ones)
#[derive(Clone, Debug)]
pub enum CachedImage {
    Text(String),
    Bitmap(ImageSurface),
    Blank,
}

// Image caching system constants
pub const MAX_CACHE_SIZE: usize = 128; // Maximum number of cached images
pub const CACHE_CLEANUP_INTERVAL: u64 = 300; // Cleanup every 5 minutes (300 seconds)
pub const CACHE_MEMORY_LIMIT_MB: f64 = 50.0; // Maximum memory usage in MB
pub const CACHE_DEBUG_KEYS: bool = true; // Enable cache debug keys

// Cache configuration structure
#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub max_size: usize,
    pub cleanup_interval: u64,
    pub memory_limit_mb: f64,
    pub debug_keys_enabled: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_size: MAX_CACHE_SIZE,
            cleanup_interval: CACHE_CLEANUP_INTERVAL,
            memory_limit_mb: CACHE_MEMORY_LIMIT_MB,
            debug_keys_enabled: CACHE_DEBUG_KEYS,
        }
    }
}

// Button image types that can be cached
pub enum ButtonImage {
    Text(String),
    Svg(SvgHandle),
    Bitmap(ImageSurface),
    Blank,
}

// Manual implementation of Debug for ButtonImage
impl std::fmt::Debug for ButtonImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ButtonImage::Text(text) => write!(f, "Text({})", text),
            ButtonImage::Svg(_) => write!(f, "Svg(<handle>)"),
            ButtonImage::Bitmap(_) => write!(f, "Bitmap(<surface>)"),
            ButtonImage::Blank => write!(f, "Blank"),
        }
    }
}

// Global cache configuration
lazy_static! {
    static ref CACHE_CONFIG: Mutex<CacheConfig> = Mutex::new(CacheConfig::default());
}

// Public functions for cache management

/// Get current cache configuration
pub fn get_cache_config() -> CacheConfig {
    CACHE_CONFIG.lock().map(|guard| guard.clone()).unwrap_or_else(|_| CacheConfig::default())
}

/// Update cache configuration
pub fn update_cache_config(new_config: CacheConfig) {
    if let Ok(mut config) = CACHE_CONFIG.lock() {
        *config = new_config;
        println!("[CacheConfig] Updated cache configuration: {:?}", config);
    }
}

/// Get image from cache
pub fn get_cached_image(key: &str) -> Option<ButtonImage> {
    IMAGE_CACHE.with(|cache| {
        if let Ok(mut cache) = cache.lock() {
            cache.get(key).map(|cached| match cached {
                CachedImage::Text(text) => ButtonImage::Text(text.clone()),
                CachedImage::Bitmap(surface) => ButtonImage::Bitmap(surface.clone()),
                CachedImage::Blank => ButtonImage::Blank,
            })
        } else {
            None
        }
    })
}

/// Store image in cache (only cloneable types)
pub fn cache_image(key: String, image: &ButtonImage) {
    let (cached_image, size_bytes) = match image {
        ButtonImage::Text(text) => (Some(CachedImage::Text(text.clone())), text.len()),
        ButtonImage::Bitmap(surface) => {
            // Estimate bitmap size (ARGB32 = 4 bytes per pixel)
            let width = surface.width() as usize;
            let height = surface.height() as usize;
            let size = width * height * 4;
            (Some(CachedImage::Bitmap(surface.clone())), size)
        },
        ButtonImage::Blank => (Some(CachedImage::Blank), 0),
        ButtonImage::Svg(_) => (None, 0), // Don't cache SVG handles
    };
    
    if let Some(cached) = cached_image {
        IMAGE_CACHE.with(|cache| {
            if let Ok(mut cache) = cache.lock() {
                cache.insert(key, cached, size_bytes);
            }
        });
    }
}

/// Clear the cache
pub fn clear_cache() {
    IMAGE_CACHE.with(|cache| {
        if let Ok(mut cache) = cache.lock() {
            cache.clear();
        }
    });
}

/// Get cache statistics
pub fn get_cache_stats() -> (usize, usize) {
    IMAGE_CACHE.with(|cache| {
        if let Ok(cache) = cache.lock() {
            cache.get_stats()
        } else {
            (0, 0)
        }
    })
}

/// Display detailed cache information
pub fn display_detailed_cache_info() {
    let config = get_cache_config();
    let (entry_count, memory_usage) = get_cache_stats();
    
    println!("[CacheInfo] ===== Button Image Cache Information =====");
    println!("  Configuration:");
    println!("    - Max entries: {}", config.max_size);
    println!("    - Memory limit: {:.2} MB", config.memory_limit_mb);
    println!("    - Cleanup interval: {} seconds", config.cleanup_interval);
    println!("    - Debug keys: {}", if config.debug_keys_enabled { "enabled" } else { "disabled" });
    println!("  Current Status:");
    println!("    - Active entries: {}", entry_count);
    println!("    - Memory usage: {:.2} MB", memory_usage as f64 / 1024.0 / 1024.0);
    println!("    - Cache utilization: {:.1}%", (entry_count as f64 / config.max_size as f64) * 100.0);
}

/// Debug cache state
pub fn debug_cache_state() {
    let (entry_count, memory_usage) = get_cache_stats();
    println!("[CacheDebug] Current cache state: {} entries, {:.2} MB", entry_count, memory_usage as f64 / 1024.0 / 1024.0);
}

/// Force cache cleanup
pub fn force_cache_cleanup() {
    clear_cache();
    println!("[CacheCleanup] Cache cleared");
}

/// Clear image cache if memory pressure is detected
pub fn clear_image_cache_if_needed() {
    let config = get_cache_config();
    let (entry_count, memory_usage) = get_cache_stats();
    let memory_mb = memory_usage as f64 / 1024.0 / 1024.0;
    
    if memory_mb > config.memory_limit_mb || entry_count > config.max_size {
        clear_cache();
        println!("[CacheManager] Cache cleared due to memory pressure: {} entries, {:.2} MB", entry_count, memory_mb);
    }
}

/// Manage image cache (called before redraws)
pub fn manage_image_cache() {
    // Trigger cleanup if needed
    IMAGE_CACHE.with(|cache| {
        if let Ok(mut cache) = cache.lock() {
            cache.cleanup_if_needed();
        }
    });
    
    // Check memory pressure
    clear_image_cache_if_needed();
}

/// Display cache performance statistics
pub fn display_cache_stats() {
    let (entry_count, memory_usage) = get_cache_stats();
    let memory_mb = memory_usage as f64 / 1024.0 / 1024.0;
    
    // Only log if there are entries or if memory usage is significant
    if entry_count > 0 || memory_mb > 1.0 {
        println!("[CacheStats] {} entries, {:.2} MB", entry_count, memory_mb);
    }
}

/// Test cache performance
pub fn test_cache_performance() {
    
    // Clear cache first
    clear_cache();
    
    // Test cache hit/miss performance
    let test_key = "test_icon:app:use_default:Adwaita";
    let test_image = ButtonImage::Text("Test".to_string());
    
    // First access (cache miss)
    let start = std::time::Instant::now();
    let _result1 = get_cached_image(test_key);
    let miss_time = start.elapsed();
    
    // Cache the image
    cache_image(test_key.to_string(), &test_image);
    
    // Second access (cache hit)
    let start = std::time::Instant::now();
    let _result2 = get_cached_image(test_key);
    let hit_time = start.elapsed();
    
    
    // Show cache stats
    let (entry_count, memory_usage) = get_cache_stats();
}

/// Load browser-specific icons from the custom directory structure
pub fn load_browser_icon(icon_name: &str) -> Result<ButtonImage> {
    let direct_path = format!("/usr/share/tiny-dfr/icons/tiny-dfr-icons/symbolic/browser/{}.svg", icon_name);
    if std::path::Path::new(&direct_path).exists() {
        println!("[load_browser_icon] Loading browser icon: {} from {}", icon_name, direct_path);
        match Loader::new().read_path(&direct_path) {
            Ok(handle) => {
                println!("[load_browser_icon] Successfully loaded browser icon: {}", icon_name);
                return Ok(ButtonImage::Svg(handle));
            }
            Err(e) => {
                println!("[load_browser_icon] Failed to load browser icon {}: {}", icon_name, e);
                return Err(anyhow::anyhow!("Failed to load browser icon {}: {}", icon_name, e));
            }
        }
    } else {
        println!("[load_browser_icon] Browser icon not found at: {}", direct_path);
        return Err(anyhow::anyhow!("Browser icon not found: {}", icon_name));
    }
}

// Constants for image loading
pub const ICON_SIZE: i32 = 48;
pub const BROWSER_ICON_SIZE: i32 = 48; // Smaller size for browser screen buttons
pub const DEBUG_LOGGING: bool = true;

/// Load image with caching support
pub fn load_image(icon_name: &str, mode: Option<String>, path: &str, theme: &str) -> Result<ButtonImage> {
    // Create cache key
    let icon_theme = match &mode {
        Some(mode_val) => {
            if mode_val == "App" { theme } else { theme }
        }
        None => {
            panic!("No mode specified")
        }
    };
    
    // Create cache key
    let cache_key = format!("{}:{}:{}:{}", icon_name, mode.as_deref().unwrap_or("none"), path, icon_theme);
    
    // Try to get from cache first
    if let Some(cached_image) = get_cached_image(&cache_key) {
        if DEBUG_LOGGING {
            println!("[load_image] Cache hit for icon: {}", icon_name);
        }
        return Ok(cached_image);
    }
    
    if DEBUG_LOGGING {
        println!("[load_image] Cache miss for icon: {}, loading from disk", icon_name);
    }
    
    // Add debug logging to see what's happening with browser icons
    if icon_name.contains("symbolic") {
        println!("[load_image] DEBUG: Loading symbolic icon: {} with path: {}", icon_name, path);
    }

    if path != "use_default" {
        return Err(anyhow::anyhow!("Custom path defined, using that"));
    }

    let mut search_paths: Vec<PathBuf> = vec![
        PathBuf::from("/etc/tiny-dfr/icons"),
        PathBuf::from("/usr/share/tiny-dfr/icons/"),
        PathBuf::from("/usr/share/icons/"),
    ];
    let mut loader = IconLoader::new();
    search_paths.extend(loader.search_paths().into_owned());
    loader.set_search_paths(search_paths);
    loader.set_theme_name_provider(icon_theme);
    loader.update_theme_name().unwrap();
    
    let icon_loader;
    match loader.load_icon(icon_name) {
        Some(icon) => {
            if icon_name.contains("symbolic") {
                println!("[load_image] DEBUG: Found icon: {} directly", icon_name);
            }
            icon_loader = icon;
        }
        None => {
            if icon_name.contains("symbolic") {
                println!("[load_image] DEBUG: Icon not found directly: {}, trying with .svg extension", icon_name);
            }
            match loader.load_icon(format!("{}.svg", icon_name)) {
                Some(icon) => {
                    if icon_name.contains("symbolic") {
                        println!("[load_image] DEBUG: Found icon: {}.svg", icon_name);
                    }
                    icon_loader = icon;
                }
                None => {
                    if icon_name.contains("symbolic") {
                        println!("[load_image] DEBUG: Icon not found with .svg: {}, trying with .png extension", icon_name);
                    }
                    match loader.load_icon(format!("{}.png", icon_name)) {
                        Some(icon) => {
                            if icon_name.contains("symbolic") {
                                println!("[load_image] DEBUG: Found icon: {}.png", icon_name);
                            }
                            icon_loader = icon;
                        }
                        None => {
                            if icon_name.contains("symbolic") {
                                println!("[load_image] DEBUG: Icon not found: {}, returning error", icon_name);
                            }
                            return Err(anyhow::anyhow!("Icon not found: {}, trying /usr/share/pixmaps", icon_name));
                        }
                    }
                }
            }
        }
    };
    
    let icon = icon_loader.file_for_size(256);
    let result = match icon.icon_type() {
        IconFileType::SVG => {
            let handle = rsvg::Loader::new().read_path(icon.path())?;
            Ok(ButtonImage::Svg(handle))
        }
        IconFileType::PNG => {
            let mut file = std::fs::File::open(icon.path())?;
            let surf = ImageSurface::create_from_png(&mut file)?;
            if surf.height() == ICON_SIZE && surf.width() == ICON_SIZE {
                Ok(ButtonImage::Bitmap(surf))
            } else {
                        let resized = ImageSurface::create(Format::ARgb32, ICON_SIZE, ICON_SIZE).unwrap();
        let c = Context::new(&resized).unwrap();
        c.scale(ICON_SIZE as f64 / surf.width() as f64, ICON_SIZE as f64 / surf.height() as f64);
        c.set_source_surface(surf, 0.0, 0.0).unwrap();
        c.set_antialias(Antialias::Best);
                c.paint().unwrap();
                Ok(ButtonImage::Bitmap(resized))
            }
        }
        IconFileType::XPM => {
            panic!("Legacy XPM icons are not supported")
        }
    };

    // Cache the result if successful
    if let Ok(ref image) = result {
        cache_image(cache_key, image);
    }

    result
}

/// Try to load SVG from a specific path
pub fn try_load_svg_path(icon_name: &str, path: &str) -> Result<ButtonImage> {
    if DEBUG_LOGGING {
        println!("[try_load_svg_path] Loading SVG: {} from disk", path);
    }

    let result = Loader::new().read_path(format!("{}", path)).or_else(|_| {
        Loader::new().read_path(format!("/usr/share/pixmaps/{}.svg", icon_name))
    })?;

    let image = ButtonImage::Svg(result);

    // Cache the result
    let cache_key = format!("svg:{}:{}", icon_name, path);
    cache_image(cache_key, &image);

    Ok(image)
}

/// Try to load PNG from a specific path
pub fn try_load_png_path(icon_name: &str, path: &str) -> Result<ButtonImage> {
    if DEBUG_LOGGING {
        println!("[try_load_png_path] Loading PNG: {} from disk", path);
    }

    let mut file = File::open(format!("{}", path)).or_else(|_| {
        File::open(format!("/usr/share/pixmaps/{}.png", icon_name))
    })?;
    
    let surf = ImageSurface::create_from_png(&mut file)?;
    let result = if surf.height() == ICON_SIZE && surf.width() == ICON_SIZE {
        Ok(ButtonImage::Bitmap(surf))
    } else {
        let resized = ImageSurface::create(Format::ARgb32, ICON_SIZE, ICON_SIZE).unwrap();
        let c = Context::new(&resized).unwrap();
        c.scale(ICON_SIZE as f64 / surf.width() as f64, ICON_SIZE as f64 / surf.height() as f64);
        c.set_source_surface(surf, 0.0, 0.0).unwrap();
        c.set_antialias(Antialias::Best);
        c.paint().unwrap();
        Ok(ButtonImage::Bitmap(resized))
    };

    // Cache the result if successful
    if let Ok(ref image) = result {
        let cache_key = format!("png:{}:{}", icon_name, path);
        cache_image(cache_key, image);
    }

    result
} 