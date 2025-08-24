use std::{
    fs::{File, OpenOptions, self},
    path::{PathBuf, Path},
    time::Instant,
    io::Write,
    cmp::min,
};
use anyhow::{Result, anyhow};
use input::event::{
    Event, switch::{Switch, SwitchEvent, SwitchState},
};
use crate::config::Config;
use crate::TIMEOUT_MS;

// Apple Touch Bar specific brightness limits
const MAX_DISPLAY_BRIGHTNESS: u32 = 509;        // Retina display max brightness (Apple-specific)
const MAX_TOUCH_BAR_BRIGHTNESS: u32 = 255;      // Touch bar 8-bit brightness range (0-255)
const BRIGHTNESS_DIM_TIMEOUT: i32 = TIMEOUT_MS * 3; // should be a multiple of TIMEOUT_MS
const BRIGHTNESS_OFF_TIMEOUT: i32 = TIMEOUT_MS * 6; // should be a multiple of TIMEOUT_MS
const DIMMED_BRIGHTNESS: u32 = 1;               // Minimum non-zero brightness to prevent complete off

fn read_attr(path: &Path, attr: &str) -> Result<u32> {
    let content = fs::read_to_string(path.join(attr))
        .map_err(|e| anyhow!("Failed to read {}: {}", attr, e))?;
    let value = content.trim().parse::<u32>()
        .map_err(|e| anyhow!("Failed to parse {}: {}", attr, e))?;
    Ok(value)
}

fn find_backlight() -> Result<PathBuf> {
    for entry in fs::read_dir("/sys/class/backlight/")? {
        let entry = entry?;
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        if ["display-pipe", "appletb_backlight"].iter().any(|s| name.contains(s)) {
            return Ok(entry.path());
        }
    }
    Err(anyhow!("No Touch Bar backlight device found"))
}

fn find_display_backlight() -> Result<PathBuf> {
    for entry in fs::read_dir("/sys/class/backlight/")? {
        let entry = entry?;
        if ["apple-panel-bl", "gmux_backlight", "intel_backlight", "acpi_video0"].iter().any(|s| entry.file_name().to_string_lossy().contains(s)) {
            return Ok(entry.path());
        }
    }
    Err(anyhow!("No Built-in Retina Display backlight device found"))
}

fn set_backlight(mut file: &File, value: u32) -> Result<()> {
    file.write_all(format!("{}\n", value).as_bytes())
        .map_err(|e| anyhow!("Failed to set backlight: {}", e))?;
    Ok(())
}

pub struct BacklightManager {
    last_active: Instant,
    max_bl: u32,
    current_bl: u32,
    lid_state: SwitchState,
    bl_file: File,
    display_bl_path: PathBuf
}

impl BacklightManager {
    pub fn new() -> Result<BacklightManager> {
        let bl_path = find_backlight()?;
        let display_bl_path = find_display_backlight()?;
        let bl_file = OpenOptions::new()
            .write(true)
            .open(bl_path.join("brightness"))
            .map_err(|e| anyhow!("Failed to open backlight file: {}", e))?;
        let max_bl = read_attr(&bl_path, "max_brightness")?;
        let current_bl = read_attr(&bl_path, "brightness")?;
        
        Ok(BacklightManager {
            bl_file,
            lid_state: SwitchState::Off,
            max_bl,
            current_bl,
            last_active: Instant::now(),
            display_bl_path
        })
    }
    fn display_to_touchbar(display: u32, active_brightness: u32) -> u32 {
        let normalized = display as f64 / MAX_DISPLAY_BRIGHTNESS as f64;
        // Add one so that the touch bar does not turn off
        let adjusted = (normalized.powf(0.5) * active_brightness as f64) as u32 + 1;
        adjusted.min(MAX_TOUCH_BAR_BRIGHTNESS) // Clamp the value to the maximum allowed brightness
    }
    pub fn process_event(&mut self, event: &Event) {
        match event {
            Event::Keyboard(_) | Event::Pointer(_) | Event::Gesture(_) | Event::Touch(_) => {
                self.last_active = Instant::now();
            },
            Event::Switch(SwitchEvent::Toggle(toggle)) => {
                match toggle.switch() {
                    Some(Switch::Lid) => {
                        self.lid_state = toggle.switch_state();
                        if toggle.switch_state() == SwitchState::Off {
                            self.last_active = Instant::now();
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    pub fn update_backlight(&mut self, cfg: &Config) -> Result<()> {
        let since_last_active = (Instant::now() - self.last_active).as_millis() as u64;
        
        let new_bl = if self.lid_state == SwitchState::On {
            0
        } else if since_last_active < BRIGHTNESS_DIM_TIMEOUT as u64 {
            if cfg.adaptive_brightness {
                match read_attr(&self.display_bl_path, "brightness") {
                    Ok(display_brightness) => {
                        BacklightManager::display_to_touchbar(display_brightness, cfg.active_brightness)
                    }
                    Err(e) => {
                        eprintln!("[BacklightManager] Failed to read display brightness: {}, using fallback", e);
                        cfg.active_brightness
                    }
                }
            } else {
                cfg.active_brightness
            }
        } else if since_last_active < BRIGHTNESS_OFF_TIMEOUT as u64 {
            DIMMED_BRIGHTNESS
        } else {
            0
        };
        
        let new_bl = min(self.max_bl, new_bl);
        
        if self.current_bl != new_bl {
            self.current_bl = new_bl;
            if let Err(e) = set_backlight(&self.bl_file, self.current_bl) {
                eprintln!("[BacklightManager] Failed to set brightness {}: {}", new_bl, e);
                // Could implement exponential backoff or fallback brightness here
            }
        }
        
        Ok(())
    }
    pub fn current_bl(&self) -> u32 {
        self.current_bl
    }
    
    pub fn is_ready(&self) -> bool {
        // Check if the backlight manager is in a usable state
        self.max_bl > 0 && self.current_bl <= self.max_bl
    }
    
    pub fn get_status(&self) -> String {
        format!("Backlight: {}/{} ({}%), Lid: {:?}, Last Active: {:?} ago", 
                self.current_bl, 
                self.max_bl,
                if self.max_bl > 0 { (self.current_bl * 100) / self.max_bl } else { 0 },
                self.lid_state,
                self.last_active.elapsed())
    }
    
    pub fn refresh_device(&mut self) -> Result<()> {
        // Try to refresh device state in case it became available
        let bl_path = find_backlight()?;
        let max_bl = read_attr(&bl_path, "max_brightness")?;
        let current_bl = read_attr(&bl_path, "brightness")?;
        
        if max_bl != self.max_bl || current_bl != self.current_bl {
            eprintln!("[BacklightManager] Device state changed: max {}->{}, current {}->{}", 
                     self.max_bl, max_bl, self.current_bl, current_bl);
            self.max_bl = max_bl;
            self.current_bl = current_bl;
        }
        
        Ok(())
    }
} 