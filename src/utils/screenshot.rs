use cairo::ImageSurface;
use std::path::Path;
use std::fs::File;
use std::io::BufWriter;
use image::{ImageBuffer, RgbaImage, ImageEncoder};
use anyhow::Result;
use crate::services::sessionmanager::SessionState;

pub struct ScreenshotManager;

impl ScreenshotManager {
    pub fn capture_touchbar_screenshot(surface: &mut ImageSurface, filename: &str, session: &Option<SessionState>) -> Result<()> {
        // Get surface dimensions first
        let width = surface.width() as u32;
        let height = surface.height() as u32;
        
        println!("[screenshot] Surface dimensions: {}x{}", width, height);
        
        // Get surface data
        let data = surface.data()?;
        
        // Convert ARGB32 data to RGBA for image crate
        let mut rgba_data = Vec::with_capacity((width * height * 4) as usize);
        
        for chunk in data.chunks(4) {
            if chunk.len() == 4 {
                // Cairo ARgb32 format is actually stored as BGRA in memory
                // Convert BGRA to RGBA for image crate
                let b = chunk[0];
                let g = chunk[1];
                let r = chunk[2];
                let a = chunk[3];
                rgba_data.push(r);
                rgba_data.push(g);
                rgba_data.push(b);
                rgba_data.push(a);
            }
        }
        
        // Create image buffer
        let mut img: RgbaImage = ImageBuffer::from_raw(width, height, rgba_data)
            .ok_or_else(|| anyhow::anyhow!("Failed to create image buffer"))?;
        
        // Check if we need to rotate the image (touchbar might be vertical)
        // If height > width, it's likely vertical and should be rotated
        if height > width {
            img = image::imageops::rotate270(&img);
        }
        
        // Save as PNG
        let path = Path::new(filename);
        let file = File::create(path).map_err(|e| {
            anyhow::anyhow!("Failed to create screenshot file '{}': {}. Make sure you have write permissions to the directory.", filename, e)
        })?;
        let writer = BufWriter::new(file);
        
        let encoder = image::codecs::png::PngEncoder::new(writer);
        let final_width = img.width();
        let final_height = img.height();
        encoder.write_image(&img, final_width, final_height, image::ColorType::Rgba8).map_err(|e| {
            anyhow::anyhow!("Failed to write PNG data to file '{}': {}", filename, e)
        })?;
        
        // Set proper file ownership if we have session user info
        if let Some(session) = session {
            if !session.user.is_empty() {
                if let Err(e) = set_file_ownership(&filename, &session.user) {
                    eprintln!("[screenshot] Warning: Failed to set file ownership for '{}': {}", filename, e);
                }
            }
        }
        
        println!("[screenshot] Touchbar screenshot saved to: {}", filename);
        Ok(())
    }
    
    pub fn generate_screenshot_filename(session: &Option<SessionState>) -> String {
        use chrono::{Local, Timelike, Datelike};
        use std::path::PathBuf;
        
        let now = Local::now();
        
        // Get user from session if available, otherwise fall back to environment
        let user = if let Some(s) = session {
            if !s.user.is_empty() {
                s.user.clone()
            } else {
                std::env::var("USER").unwrap_or_else(|_| "unknown".to_string())
            }
        } else {
            std::env::var("USER").unwrap_or_else(|_| "unknown".to_string())
        };
        
        // Try multiple fallback locations for the user
        let possible_dirs = vec![
            // Primary: User's Pictures directory (using session user)
            if user != "unknown" {
                Some(PathBuf::from("/home").join(&user).join("Pictures").join("tiny-dfr"))
            } else {
                std::env::var("HOME").ok().map(|home| PathBuf::from(home).join("Pictures").join("tiny-dfr"))
            },
            // Fallback 1: User's home directory (using session user)
            if user != "unknown" {
                Some(PathBuf::from("/home").join(&user).join("tiny-dfr-screenshots"))
            } else {
                std::env::var("HOME").ok().map(|home| PathBuf::from(home).join("tiny-dfr-screenshots"))
            },
            // Fallback 2: XDG Pictures directory
            std::env::var("XDG_PICTURES_DIR").ok().map(|dir| PathBuf::from(dir).join("tiny-dfr")),
            // Fallback 3: Current working directory
            Some(PathBuf::from(".").join("screenshots")),
            // Fallback 4: /tmp (last resort)
            Some(PathBuf::from("/tmp")),
        ];
        
        for pictures_dir in possible_dirs {
            if let Some(dir) = pictures_dir {
                // Try to create the directory if it doesn't exist
                match std::fs::create_dir_all(&dir) {
                    Ok(_) => {
                        // Set directory ownership to the user if we have session info
                        if let Some(session) = session {
                            if !session.user.is_empty() {
                                if let Err(e) = set_file_ownership(&dir.to_string_lossy(), &session.user) {
                                    eprintln!("[screenshot] Warning: Failed to set directory ownership for {:?}: {}", dir, e);
                                }
                            }
                        }
                        
                        let filename = format!(
                            "touchbar_screenshot_{:04}{:02}{:02}_{:02}{:02}{:02}.png",
                            now.year(),
                            now.month(),
                            now.day(),
                            now.hour(),
                            now.minute(),
                            now.second()
                        );
                        
                        let full_path = dir.join(filename);
                        return full_path.to_string_lossy().to_string();
                    }
                    Err(e) => {
                        eprintln!("[screenshot] Warning: Failed to create directory {:?}: {}", dir, e);
                        continue;
                    }
                }
            }
        }
        
        // This should never happen, but just in case
        format!(
            "/tmp/touchbar_screenshot_{:04}{:02}{:02}_{:02}{:02}{:02}.png",
            now.year(),
            now.month(),
            now.day(),
            now.hour(),
            now.minute(),
            now.second()
        )
    }
}

fn set_file_ownership(filename: &str, username: &str) -> Result<()> {
    use std::process::Command;
    
    
    // Get user ID from username
    let output = Command::new("id")
        .arg("-u")
        .arg(username)
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to get user ID for '{}': {}", username, e))?;
    
    if !output.status.success() {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("User '{}' not found: {}", username, error_msg));
    }
    
    let uid_str = String::from_utf8(output.stdout)
        .map_err(|e| anyhow::anyhow!("Failed to parse user ID: {}", e))?;
    let uid: u32 = uid_str.trim().parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse user ID '{}': {}", uid_str.trim(), e))?;
    
    
    // Get group ID from username
    let output = Command::new("id")
        .arg("-g")
        .arg(username)
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to get group ID for '{}': {}", username, e))?;
    
    if !output.status.success() {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("Group for user '{}' not found: {}", username, error_msg));
    }
    
    let gid_str = String::from_utf8(output.stdout)
        .map_err(|e| anyhow::anyhow!("Failed to parse group ID: {}", e))?;
    let gid: u32 = gid_str.trim().parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse group ID '{}': {}", gid_str.trim(), e))?;
    
    
    // Change file ownership using chown with username:groupname format
    // Use -R flag for directories to change ownership recursively
    let output = Command::new("chown")
        .arg("-R")
        .arg(format!("{}:{}", username, username))
        .arg(filename)
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to change file ownership: {}", e))?;
    
    if !output.status.success() {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("chown failed: {}", error_msg));
    }
    
    Ok(())
}
