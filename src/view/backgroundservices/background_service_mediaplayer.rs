//! Background service media player UI components
use cairo::Context;
use crate::helper::MediaStatus;
use rsvg::{CairoRenderer, Loader};

// Background service player UI constants
const BACKGROUND_SERVICE_ICON_PATH: &str = "/usr/share/tiny-dfr/icons/tiny-dfr-icons/symbolic/media/spotify/media-playback-start-symbolic.svg";
const BACKGROUND_SERVICE_PLAY_ICON_PATH: &str = "/usr/share/tiny-dfr/icons/tiny-dfr-icons/symbolic/media/spotify/play.svg";
const BACKGROUND_SERVICE_PAUSE_ICON_PATH: &str = "/usr/share/tiny-dfr/icons/tiny-dfr-icons/symbolic/media/spotify/pause.svg";
const BACKGROUND_SERVICE_NEXT_ICON_PATH: &str = "/usr/share/tiny-dfr/icons/tiny-dfr-icons/symbolic/media/spotify/go-next-symbolic.svg";
const BACKGROUND_SERVICE_PREVIOUS_ICON_PATH: &str = "/usr/share/tiny-dfr/icons/tiny-dfr-icons/symbolic/media/spotify/go-previous-symbolic.svg";

fn render_svg_icon_from_path(c: &Context, icon_path: &str, x: f64, y: f64, size: f64) -> Result<(), Box<dyn std::error::Error>> {
    let loader = Loader::new();
    let handle = loader.read_path(icon_path)?;
    let renderer = CairoRenderer::new(&handle);
    
    c.save().unwrap();
    c.translate(x, y);
    c.scale(size / 24.0, size / 24.0); // Scale to desired size (assuming 24x24 base)
    
    // Create a rectangle for the render area
    let rect = cairo::Rectangle::new(0.0, 0.0, 24.0, 24.0);
    renderer.render_document(c, &rect)?;
    c.restore().unwrap();
    
    Ok(())
}

fn format_duration(seconds: i64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    
    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes, secs)
    } else {
        format!("{}:{:02}", minutes, secs)
    }
}

// Color mapping for different background services
fn get_service_color(service_name: &str) -> (f64, f64, f64) {
    let service_lower = service_name.to_lowercase();
    
    if service_lower.contains("spotify") {
        // Spotify green #1DB954
        (0.11, 0.73, 0.33)
    } else if service_lower.contains("chromium") {
        // Chromium blue #4285F4
        (0.26, 0.52, 0.96)
    } else if service_lower.contains("firefox") {
        // Firefox orange #FF9500
        (1.0, 0.58, 0.0)
    } else if service_lower.contains("chrome") {
        // Chrome blue #4285F4
        (0.26, 0.52, 0.96)
    } else if service_lower.contains("vlc") {
        // VLC orange #FF8800
        (1.0, 0.53, 0.0)
    } else if service_lower.contains("mpv") {
        // MPV red #FF0000
        (1.0, 0.0, 0.0)
    } else {
        // Default purple for unknown services
        (0.5, 0.3, 0.8)
    }
}

// Draw a vertical separator line
fn draw_separator(c: &Context, x: f64, y: f64, height: f64, anim_progress: f64) {
    c.save().unwrap();
    c.set_line_width(1.0);
    c.set_source_rgba(0.0, 0.0, 0.0, anim_progress * 0.6); // Subtle gray separator
    c.move_to(x, y);
    c.line_to(x, y + height);
    c.stroke().unwrap();
    c.restore().unwrap();
}

pub fn draw_background_service_player_control_button(c: &Context, x: f64, y: f64, width: f64, height: f64, icon_path: &str, anim_progress: f64) {
    c.save().unwrap();
    
    // Dark background for control buttons
    c.set_source_rgba(0.15, 0.15, 0.15, anim_progress);
    
    // Fully rounded (circular) button
    let radius = height / 2.0;
    c.new_sub_path();
    c.arc(x + width / 2.0, y + height / 2.0, radius, 0.0, 2.0 * std::f64::consts::PI);
    c.fill().unwrap();
    
    // Subtle border
    c.set_line_width(1.0);
    c.set_source_rgba(0.3, 0.3, 0.3, anim_progress * 0.6);
    c.stroke().unwrap();
    
    // Draw icon
    let icon_size = (width * 0.7).min(height * 0.7);
    let icon_x = x + (width - icon_size) / 2.0;
    let icon_y = y + (height - icon_size) / 2.0;
    
    if let Err(e) = render_svg_icon_from_path(c, icon_path, icon_x, icon_y, icon_size) {
        eprintln!("Failed to render SVG icon from {}: {}", icon_path, e);
    }
    
    c.restore().unwrap();
}

pub fn draw_background_service_player_progressbar(c: &Context, x: f64, y: f64, width: f64, height: f64, progress: f64, anim_progress: f64, service_name: &str) {
    c.save().unwrap();
    
    // Progress bar background (dark gray)
    c.set_source_rgba(0.2, 0.2, 0.2, anim_progress);
    let radius = height / 8.0;
    c.new_sub_path();
    c.arc(x + width - radius, y + radius, radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
    c.arc(x + width - radius, y + height - radius, radius, (0.0f64).to_radians(), (90.0f64).to_radians());
    c.arc(x + radius, y + height - radius, radius, (90.0f64).to_radians(), (180.0f64).to_radians());
    c.arc(x + radius, y + radius, radius, (180.0f64).to_radians(), (270.0f64).to_radians());
    c.close_path();
    c.fill().unwrap();
    
    // Black border around progress bar
    c.set_source_rgba(0.0, 0.0, 0.0, anim_progress);
    c.set_line_width(1.0);
    c.new_sub_path();
    c.arc(x + width - radius, y + radius, radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
    c.arc(x + width - radius, y + height - radius, radius, (0.0f64).to_radians(), (90.0f64).to_radians());
    c.arc(x + radius, y + height - radius, radius, (90.0f64).to_radians(), (180.0f64).to_radians());
    c.arc(x + radius, y + radius, radius, (180.0f64).to_radians(), (270.0f64).to_radians());
    c.close_path();
    c.stroke().unwrap();
    
    // Progress fill with background service player green gradient
    if progress > 0.0 {
        let fill_width = width * progress;
        let fill_gradient = cairo::LinearGradient::new(x, y, x + fill_width, y);
        
        // Get service-specific color
        let (r, g, b) = get_service_color(service_name);
        fill_gradient.add_color_stop_rgba(0.0, r, g, b, anim_progress);
        fill_gradient.add_color_stop_rgba(1.0, r + 0.1, g + 0.1, b + 0.1, anim_progress); // Lighter version at end
        let _ = c.set_source(&fill_gradient);
        
        c.new_sub_path();
        c.arc(x + fill_width - radius, y + radius, radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
        c.arc(x + fill_width - radius, y + height - radius, radius, (0.0f64).to_radians(), (90.0f64).to_radians());
        c.arc(x + radius, y + height - radius, radius, (90.0f64).to_radians(), (180.0f64).to_radians());
        c.arc(x + radius, y + radius, radius, (180.0f64).to_radians(), (270.0f64).to_radians());
        c.close_path();
        c.fill().unwrap();
    }
    
    // Progress bar head (white circle with green center)
    if progress >= 0.0 {
        let head_x = x + (width * progress);
        let head_y = y + height / 2.0;
        let head_radius = 8.0;
        
        // White outer circle
        c.set_source_rgba(1.0, 1.0, 1.0, anim_progress);
        c.new_sub_path();
        c.arc(head_x, head_y, head_radius, 0.0, 2.0 * std::f64::consts::PI);
        c.fill().unwrap();
        
        // Service-specific colored inner circle
        let (r, g, b) = get_service_color(service_name);
        c.set_source_rgba(r, g, b, anim_progress);
        c.new_sub_path();
        c.arc(head_x, head_y, head_radius - 2.0, 0.0, 2.0 * std::f64::consts::PI);
        c.fill().unwrap();
    }
    
    c.restore().unwrap();
}

#[derive(Debug, Clone)]
pub enum BackgroundServicePlayerAction {
    PlayPause,
    Next,
    Previous,
    Seek(f64), // 0.0 to 1.0
    DragHead(f64), // 0.0 to 1.0 - for dragging the progress bar head
}

pub struct BackgroundServicePlayer {
    pub last_status: Option<MediaStatus>,
    pub is_dragging: bool,
}

impl BackgroundServicePlayer {
    pub fn new() -> Self {
        Self {
            last_status: None,
            is_dragging: false,
        }
    }

    pub fn draw_details(
        &self,
        c: &Context,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        mpris_name: &str,
        drag_position: Option<f64>,
    ) {
        let padding = 15.0;
        let radius = 8.0; // Same radius as items
        let anim_progress = 1.0; // Full opacity for now
        
        c.save().unwrap();
        
        // Draw dark background for background service player details
        c.set_source_rgba(0.15, 0.15, 0.15, anim_progress); // Dark background
        
        // Fill (straight left, rounded right)
        c.new_path();
        
        // Top-left
        c.move_to(x, y);
        
        // Top edge to before top-right corner
        c.line_to(x + width - radius, y);
        
        // Top-right corner: from top (-PI/2) to right (0)
        c.arc(
            x + width - radius,       // center x
            y + radius,               // center y
            radius,
            -0.5 * std::f64::consts::PI, // start = -PI/2 (top)
            0.0                       //  end = 0      (right)
        );
        
        // Right edge down to before bottom-right corner
        c.line_to(x + width, y + height - radius);
        
        // Bottom-right corner: from right (0) to bottom (PI/2)
        c.arc(
            x + width - radius,
            y + height - radius,
            radius,
            0.0,                      // start = 0       (right)
            0.5 * std::f64::consts::PI, // end   = PI/2   (bottom)
        );
        
        // Bottom edge back to left
        c.line_to(x, y + height);
        
        // Close & fill
        c.close_path();
        c.fill().unwrap();
        
        // Draw subtle border
        c.set_source_rgba(0.2, 0.2, 0.2, 0.2);
        c.set_line_width(0.5);
        c.new_path();
        
        // Top-left
        c.move_to(x, y);
        
        // Top edge to before top-right corner
        c.line_to(x + width - radius, y);
        
        // Top-right corner: from top (-PI/2) to right (0)
        c.arc(
            x + width - radius,       // center x
            y + radius,               // center y
            radius,
            -0.5 * std::f64::consts::PI, // start = -PI/2 (top)
            0.0                       //  end = 0      (right)
        );
        
        // Right edge down to before bottom-right corner
        c.line_to(x + width, y + height - radius);
        
        // Bottom-right corner: from right (0) to bottom (PI/2)
        c.arc(
            x + width - radius,
            y + height - radius,
            radius,
            0.0,                      // start = 0       (right)
            0.5 * std::f64::consts::PI, // end   = PI/2   (bottom)
        );
        
        // Bottom edge back to left
        c.line_to(x, y + height);
        
        // Close & stroke
        c.close_path();
        c.stroke().unwrap();
        
        // Background service player UI layout: [current_time] [progress_bar] [total_time] [prev] [play/pause] [next]
        
        // 1. Control buttons on the right
        let button_height = height * 0.9;
        let button_width = button_height * 1.6; // Slightly wider than height
        let button_spacing = 20.0;
        
        // Calculate total width of all buttons
        let total_buttons_width = (button_width * 3.0) + (button_spacing * 2.0);
        let buttons_start_x = x + width - total_buttons_width - 12.0; // 12px from right edge
        
        // Previous button
        let prev_x = buttons_start_x;
        let prev_y = y + (height - button_height) / 2.0;
        draw_background_service_player_control_button(c, prev_x, prev_y, button_width, button_height, BACKGROUND_SERVICE_PREVIOUS_ICON_PATH, anim_progress);
        
        // Separator between previous and play/pause buttons
        let separator1_x = prev_x + button_width + (button_spacing / 2.0);
        draw_separator(c, separator1_x, y, height, anim_progress);
        
        // Play/Pause button (main button)
        let main_button_x = prev_x + button_width + button_spacing;
        let main_button_y = prev_y;
        let is_playing = self.last_status.as_ref().map(|s| s.is_playing).unwrap_or(false);
        let play_pause_icon = if is_playing { BACKGROUND_SERVICE_PAUSE_ICON_PATH } else { BACKGROUND_SERVICE_PLAY_ICON_PATH };
        draw_background_service_player_control_button(c, main_button_x, main_button_y, button_width, button_height, play_pause_icon, anim_progress);
        
        // Separator between play/pause and next buttons
        let separator2_x = main_button_x + button_width + (button_spacing / 2.0);
        draw_separator(c, separator2_x, y, height, anim_progress);
        
        // Next button
        let next_x = main_button_x + button_width + button_spacing;
        let next_y = prev_y;
        draw_background_service_player_control_button(c, next_x, next_y, button_width, button_height, BACKGROUND_SERVICE_NEXT_ICON_PATH, anim_progress);
        
        // 2. Time display and progress bar in the center
        let content_start_x = x + 12.0; // Start from left edge
        let available_width = buttons_start_x - content_start_x - 20.0; // Available space between left edge and buttons
        
        if let Some(status) = &self.last_status {
            // Current time
            let current_seconds = if status.duration > 0 {
                (status.position * status.duration as f64) as i64
            } else {
                0
            };
            let current_time_str = format_duration(current_seconds);
            
            // Total time
            let total_time_seconds = status.duration;
            let total_time_str = format_duration(total_time_seconds);
            
            // Get text extents
            c.save().unwrap();
            c.set_font_size(14.0);
            c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
            
            let current_time_ext = c.text_extents(&current_time_str).unwrap();
            let total_time_ext = c.text_extents(&total_time_str).unwrap();
            c.restore().unwrap();
            
            // Fixed spacing calculations
            let estimated_current_time_width = 45.0;
            let estimated_total_time_width = 45.0;
            let time_margin = 10.0;
            let min_progress_width = 30.0;
            let progress_w = (available_width - estimated_current_time_width - estimated_total_time_width - time_margin * 2.0).max(min_progress_width);
            
            // Current time - centered
            let current_time_area_width = estimated_current_time_width;
            let current_time_x = content_start_x + (current_time_area_width - current_time_ext.width()) / 2.0;
            let current_time_y = y + (height + current_time_ext.height()) / 2.0;
            
            c.save().unwrap();
            c.set_font_size(14.0);
            c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
            c.set_source_rgba(0.9, 0.9, 0.9, anim_progress); // Light gray
            c.move_to(current_time_x, current_time_y);
            c.show_text(&current_time_str).unwrap();
            c.restore().unwrap();
            
            // Progress bar
            let progress_x = current_time_x + estimated_current_time_width + time_margin;
            let progress_y = y + 6.0;
            let progress_h = height - 12.0;
            
            let head_position = drag_position.unwrap_or(status.position);
            draw_background_service_player_progressbar(c, progress_x, progress_y, progress_w, progress_h, head_position, anim_progress, mpris_name);
            
            // Total time - centered
            let total_time_area_width = estimated_total_time_width;
            let total_time_x = progress_x + progress_w + time_margin + (total_time_area_width - total_time_ext.width()) / 2.0;
            let total_time_y = y + (height + total_time_ext.height()) / 2.0;
            
            c.save().unwrap();
            c.set_font_size(14.0);
            c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
            c.set_source_rgba(0.9, 0.9, 0.9, anim_progress); // Light gray
            c.move_to(total_time_x, total_time_y);
            c.show_text(&total_time_str).unwrap();
            c.restore().unwrap();
            
            // Draw separator line between time display and control buttons
            let separator_x = progress_x + progress_w + time_margin + estimated_total_time_width + 10.0; // 10px after total time area
            draw_separator(c, separator_x, y, height, anim_progress);
        } else {
            // Show background service player text when no status available
            c.save().unwrap();
            c.set_font_size(16.0);
            c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
            // Use service-specific color
            let (r, g, b) = get_service_color(mpris_name);
            c.set_source_rgba(r, g, b, anim_progress);
            
            let ext = c.text_extents("No media playing").unwrap();
            let text_x = content_start_x;
            let text_y = y + (height + ext.height()) / 2.0;
            
            c.move_to(text_x, text_y);
            c.show_text("No media playing").unwrap();
            c.restore().unwrap();
        }
        
        c.restore().unwrap();
    }

    pub fn hit_test_controls(&mut self, touch_x: f64, touch_y: f64, x: f64, y: f64, width: f64, height: f64) -> Option<BackgroundServicePlayerAction> {
        // Background service player UI layout: [current_time] [progress_bar] [total_time] [prev] [play/pause] [next]
        
        // 1. Control buttons on the right
        let button_height = height * 0.9;
        let button_width = button_height * 1.6; // Slightly wider than height
        let button_spacing = 20.0;
        
        // Calculate total width of all buttons
        let total_buttons_width = (button_width * 3.0) + (button_spacing * 2.0);
        let buttons_start_x = x + width - total_buttons_width - 12.0; // 12px from right edge
        
        // Previous button - rectangular hit test for oval buttons
        let prev_x = buttons_start_x;
        let prev_y = y + (height - button_height) / 2.0;
        if touch_x >= prev_x && touch_x <= prev_x + button_width && 
           touch_y >= prev_y && touch_y <= prev_y + button_height {
            return Some(BackgroundServicePlayerAction::Previous);
        }
        
        // Play/Pause button (main button) - rectangular hit test for oval buttons
        let main_button_x = prev_x + button_width + button_spacing;
        let main_button_y = prev_y;
        if touch_x >= main_button_x && touch_x <= main_button_x + button_width && 
           touch_y >= main_button_y && touch_y <= main_button_y + button_height {
            return Some(BackgroundServicePlayerAction::PlayPause);
        }
        
        // Next button - rectangular hit test for oval buttons
        let next_x = main_button_x + button_width + button_spacing;
        let next_y = prev_y;
        if touch_x >= next_x && touch_x <= next_x + button_width && 
           touch_y >= next_y && touch_y <= next_y + button_height {
            return Some(BackgroundServicePlayerAction::Next);
        }
        
        // Check progress bar area
        let content_start_x = x + 12.0; // Start from left edge
        let available_width = buttons_start_x - content_start_x - 20.0; // Available space between left edge and buttons
        
        if let Some(status) = &self.last_status {
            let estimated_current_time_width = 45.0;
            let estimated_total_time_width = 45.0;
            let time_margin = 10.0;
            let min_progress_width = 30.0;
            let progress_w = (available_width - estimated_current_time_width - estimated_total_time_width - time_margin * 2.0).max(min_progress_width);
            
            let current_time_x = content_start_x;
            let progress_x = current_time_x + estimated_current_time_width + time_margin;
            let progress_y = y + 6.0;
            let progress_h = height - 12.0;
            
            // Check if touch is on the progress bar area
            if touch_x >= progress_x && touch_x <= progress_x + progress_w &&
               touch_y >= progress_y && touch_y <= progress_y + progress_h {
                
                // Calculate progress ratio based on touch position
                let progress_ratio = (touch_x - progress_x) / progress_w;
                
                // Check if touch is on the progress bar head (within 15px of current position)
                let current_position = status.position;
                let head_x = progress_x + (progress_w * current_position);
                let head_y = progress_y + progress_h / 2.0;
                let head_width = 16.0; // Width of the head (8px radius * 2)
                let head_height = 16.0; // Height of the head (8px radius * 2)
                let distance_from_position = ((touch_x - head_x).powi(2) + (touch_y - head_y).powi(2)).sqrt();
                
                // If touch is on head or within 15px of current position, treat as drag
                if (touch_x >= head_x && touch_x <= head_x + head_width &&
                    touch_y >= head_y && touch_y <= head_y + head_height) ||
                   distance_from_position <= 15.0 {
                    self.is_dragging = true;
                    return Some(BackgroundServicePlayerAction::DragHead(progress_ratio));
                }
                
                // If we're already dragging, continue dragging regardless of position
                if self.is_dragging {
                    return Some(BackgroundServicePlayerAction::DragHead(progress_ratio));
                }
                
                // For any other touch on progress bar, treat as seek
                return Some(BackgroundServicePlayerAction::Seek(progress_ratio));
            }
        }
        
        None
    }
    
    pub fn stop_dragging(&mut self) {
        self.is_dragging = false;
    }
}
