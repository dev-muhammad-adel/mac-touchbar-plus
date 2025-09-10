//! Button image types and management for tiny-dfr
//! 
//! This module provides the ButtonImage enum and related structures for
//! handling different types of button images (SVG, PNG, text, blank).
//! Simple direct loading without caching.

use cairo::{ImageSurface, Format, Antialias, Context};
use rsvg::{SvgHandle, Loader};
use anyhow::Result;
use icon_loader::{IconLoader, IconFileType};
use std::path::PathBuf;
use std::fs::File;

// Button image types
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

// Constants for image loading
pub const ICON_SIZE: i32 = 42;
pub const BROWSER_ICON_SIZE: i32 = 42; // Smaller size for browser screen buttons

/// Load browser icon directly from disk
pub fn load_browser_icon(icon_name: &str) -> Result<ButtonImage> {
    let direct_path = format!("/usr/share/pixmaps/{}.svg", icon_name);
    if std::path::Path::new(&direct_path).exists() {
        match Loader::new().read_path(&direct_path) {
            Ok(handle) => {
                println!("[load_browser_icon] Loaded browser icon: {} from {}", icon_name, direct_path);
                Ok(ButtonImage::Svg(handle))
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

/// Load image directly from disk without caching
pub fn load_image(icon_name: &str, _mode: Option<String>, path: &str, theme: &str) -> Result<ButtonImage> {
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
    loader.set_theme_name_provider(theme);
    loader.update_theme_name().unwrap();
    
    let icon_loader;
    match loader.load_icon(icon_name) {
        Some(icon) => {
            icon_loader = icon;
        }
        None => {
            match loader.load_icon(format!("{}.svg", icon_name)) {
                Some(icon) => {
                    icon_loader = icon;
                }
                None => {
                    match loader.load_icon(format!("{}.png", icon_name)) {
                        Some(icon) => {
                            icon_loader = icon;
                        }
                        None => {
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

    result
}

/// Try to load SVG from a specific path
pub fn try_load_svg_path(icon_name: &str, path: &str) -> Result<ButtonImage> {
    let result = Loader::new().read_path(format!("{}", path)).or_else(|_| {
        Loader::new().read_path(format!("/usr/share/pixmaps/{}.svg", icon_name))
    })?;

    Ok(ButtonImage::Svg(result))
}

/// Try to load PNG from a specific path
pub fn try_load_png_path(icon_name: &str, path: &str) -> Result<ButtonImage> {
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

    result
} 