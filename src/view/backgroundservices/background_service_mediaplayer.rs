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

// Color constants
const COLOR_DARK_BACKGROUND: (f64, f64, f64, f64) = (0.15, 0.15, 0.15, 1.0);
const COLOR_BUTTON_BORDER: (f64, f64, f64, f64) = (0.3, 0.3, 0.3, 0.6);
const COLOR_PROGRESS_BACKGROUND: (f64, f64, f64, f64) = (0.18, 0.18, 0.18, 1.0);
const COLOR_PROGRESS_BORDER: (f64, f64, f64, f64) = (0.0, 0.0, 0.0, 1.0);
const COLOR_WHITE: (f64, f64, f64, f64) = (1.0, 1.0, 1.0, 1.0);
const COLOR_SEPARATOR: (f64, f64, f64, f64) = (0.0, 0.0, 0.0, 0.6);
const COLOR_DETAILS_BACKGROUND: (f64, f64, f64, f64) = (0.280, 0.280, 0.280, 1.0);
const COLOR_DETAILS_BORDER: (f64, f64, f64, f64) = (0.2, 0.2, 0.2, 0.2);
const COLOR_TEXT_BACKGROUND: (f64, f64, f64, f64) = (0.3, 0.3, 0.3, 1.0);
const COLOR_TEXT: (f64, f64, f64, f64) = (1.0, 1.0, 1.0, 1.0);

// Service-specific colors
const COLOR_SPOTIFY: (f64, f64, f64) = (0.11, 0.73, 0.33); // #1DB954
const COLOR_CHROMIUM: (f64, f64, f64) = (0.26, 0.52, 0.96); // #4285F4
const COLOR_FIREFOX: (f64, f64, f64) = (1.0, 0.58, 0.0); // #FF9500
const COLOR_CHROME: (f64, f64, f64) = (0.26, 0.52, 0.96); // #4285F4
const COLOR_VLC: (f64, f64, f64) = (1.0, 0.53, 0.0); // #FF8800
const COLOR_MPV: (f64, f64, f64) = (1.0, 0.0, 0.0); // #FF0000
const COLOR_DEFAULT: (f64, f64, f64) = (0.5, 0.3, 0.8); // Default purple

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
        COLOR_SPOTIFY
    } else if service_lower.contains("chromium") {
        COLOR_CHROMIUM
    } else if service_lower.contains("firefox") {
        COLOR_FIREFOX
    } else if service_lower.contains("chrome") {
        COLOR_CHROME
    } else if service_lower.contains("vlc") {
        COLOR_VLC
    } else if service_lower.contains("mpv") {
        COLOR_MPV
    } else {
        COLOR_DEFAULT
    }
}

// Draw a vertical separator line
fn draw_separator(c: &Context, x: f64, y: f64, height: f64, anim_progress: f64) {
    c.save().unwrap();
    c.set_line_width(2.0);
    c.set_source_rgba(COLOR_SEPARATOR.0, COLOR_SEPARATOR.1, COLOR_SEPARATOR.2, COLOR_SEPARATOR.3);
    c.move_to(x, y);
    c.line_to(x, y + height);
    c.stroke().unwrap();
    c.restore().unwrap();
}

pub fn draw_background_service_player_control_button(c: &Context, x: f64, y: f64, width: f64, height: f64, icon_path: &str, anim_progress: f64, is_pressed: bool, is_hovered: bool) {
    c.save().unwrap();
    
    // Button background with visual feedback
    let (bg_color, border_color) = if is_pressed {
        // Pressed state - darker background
        ((COLOR_DETAILS_BACKGROUND.0 * 0.7, COLOR_DETAILS_BACKGROUND.1 * 0.7, COLOR_DETAILS_BACKGROUND.2 * 0.7, anim_progress),
         (COLOR_BUTTON_BORDER.0, COLOR_BUTTON_BORDER.1, COLOR_BUTTON_BORDER.2, anim_progress))
    } else if is_hovered {
        // Hovered state - slightly lighter background
        ((COLOR_DETAILS_BACKGROUND.0 * 1.2, COLOR_DETAILS_BACKGROUND.1 * 1.2, COLOR_DETAILS_BACKGROUND.2 * 1.2, anim_progress),
         (COLOR_BUTTON_BORDER.0, COLOR_BUTTON_BORDER.1, COLOR_BUTTON_BORDER.2, anim_progress))
    } else {
        // Normal state
        ((COLOR_DETAILS_BACKGROUND.0, COLOR_DETAILS_BACKGROUND.1, COLOR_DETAILS_BACKGROUND.2, anim_progress),
         (COLOR_BUTTON_BORDER.0, COLOR_BUTTON_BORDER.1, COLOR_BUTTON_BORDER.2, anim_progress * 0.5))
    };
    
    c.set_source_rgba(bg_color.0, bg_color.1, bg_color.2, bg_color.3);
    
    // Simple rectangular button that uses full width
    c.rectangle(x, y, width, height);
    c.fill().unwrap();
    
    // Add border for visual feedback
    c.set_line_width(1.0);
    c.set_source_rgba(border_color.0, border_color.1, border_color.2, border_color.3);
    c.rectangle(x, y, width, height);
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
    c.set_source_rgba(COLOR_PROGRESS_BACKGROUND.0, COLOR_PROGRESS_BACKGROUND.1, COLOR_PROGRESS_BACKGROUND.2, anim_progress);
    let radius = height / 8.0;
    c.new_sub_path();
    c.arc(x + width - radius, y + radius, radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
    c.arc(x + width - radius, y + height - radius, radius, (0.0f64).to_radians(), (90.0f64).to_radians());
    c.arc(x + radius, y + height - radius, radius, (90.0f64).to_radians(), (180.0f64).to_radians());
    c.arc(x + radius, y + radius, radius, (180.0f64).to_radians(), (270.0f64).to_radians());
    c.close_path();
    c.fill().unwrap();
    
    // Black border around progress bar
    c.set_source_rgba(COLOR_PROGRESS_BORDER.0, COLOR_PROGRESS_BORDER.1, COLOR_PROGRESS_BORDER.2, anim_progress);
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
        c.set_source_rgba(COLOR_WHITE.0, COLOR_WHITE.1, COLOR_WHITE.2, anim_progress);
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

#[derive(Debug, Clone, PartialEq)]
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
    pub pressed_button: Option<BackgroundServicePlayerAction>,
}

impl BackgroundServicePlayer {
    pub fn new() -> Self {
        Self {
            last_status: None,
            is_dragging: false,
            pressed_button: None,
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
        c.set_source_rgba(COLOR_DETAILS_BACKGROUND.0, COLOR_DETAILS_BACKGROUND.1, COLOR_DETAILS_BACKGROUND.2, anim_progress);
        
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
        c.set_source_rgba(COLOR_DETAILS_BORDER.0, COLOR_DETAILS_BORDER.1, COLOR_DETAILS_BORDER.2, COLOR_DETAILS_BORDER.3);
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
        let button_height = height * 1.0;
        let button_width = button_height * 2.2; // Wider buttons for better touch targets
        let button_spacing = 2.0; // No spacing between buttons to save space
        
        // Calculate total width of all buttons
        let total_buttons_width = (button_width * 3.0) + (button_spacing * 2.0);
        let buttons_start_x = x + width - total_buttons_width - 12.0; // 12px from right edge
        
        // Previous button
        let prev_x = buttons_start_x;
        let prev_y = y + (height - button_height) / 2.0;
        let is_prev_pressed = self.pressed_button == Some(BackgroundServicePlayerAction::Previous) && !self.is_dragging;
        draw_background_service_player_control_button(c, prev_x, prev_y, button_width, button_height, BACKGROUND_SERVICE_PREVIOUS_ICON_PATH, anim_progress, is_prev_pressed, false);
        
        // Separator between previous and play/pause buttons
        let separator1_x = prev_x + button_width + (button_spacing / 2.0);
        draw_separator(c, separator1_x, y, height, anim_progress);
        
        // Play/Pause button (main button)
        let main_button_x = prev_x + button_width + button_spacing;
        let main_button_y = prev_y;
        let is_playing = self.last_status.as_ref().map(|s| s.is_playing).unwrap_or(false);
        let play_pause_icon = if is_playing { BACKGROUND_SERVICE_PAUSE_ICON_PATH } else { BACKGROUND_SERVICE_PLAY_ICON_PATH };
        let is_play_pause_pressed = self.pressed_button == Some(BackgroundServicePlayerAction::PlayPause) && !self.is_dragging;
        draw_background_service_player_control_button(c, main_button_x, main_button_y, button_width, button_height, play_pause_icon, anim_progress, is_play_pause_pressed, false);
        
        // Separator between play/pause and next buttons
        let separator2_x = main_button_x + button_width + (button_spacing / 2.0);
        draw_separator(c, separator2_x, y, height, anim_progress); 
        
        // Next button
        let next_x = main_button_x + button_width + button_spacing;
        let next_y = prev_y;
        let is_next_pressed = self.pressed_button == Some(BackgroundServicePlayerAction::Next) && !self.is_dragging;
        draw_background_service_player_control_button(c, next_x, next_y, button_width, button_height, BACKGROUND_SERVICE_NEXT_ICON_PATH, anim_progress, is_next_pressed, false);
        
        // 2. Time display and progress bar in the center
        let content_start_x = x + 8.0; // Reduced left margin to give more space
        let available_width = buttons_start_x - content_start_x - 8.0; // Reduced right margin to give more space
        
        
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
            
            // Get text extents once and reuse them
            c.save().unwrap();
            c.set_font_size(16.0); // Match the rendering font size
            c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
            
            let current_time_ext = c.text_extents(&current_time_str).unwrap();
            let total_time_ext = c.text_extents(&total_time_str).unwrap();
            c.restore().unwrap();
            
            println!("[background_service_player] Text extents - current: width={}, height={}, total: width={}, height={}", 
                current_time_ext.width(), current_time_ext.height(), 
                total_time_ext.width(), total_time_ext.height());
            
            // Fixed width allocation for timer elements
            let current_time_width = 50.0; // Fixed width for current time
            let total_time_width = 50.0; // Fixed width for total time
            let time_margin = 8.0; // Fixed margin between elements
            
            // Calculate progress bar width based on fixed layout
            let total_timer_width = current_time_width + total_time_width + time_margin * 2.0; // 116px total
            let progress_w = available_width - total_timer_width; // Remaining space for progress bar
            
            
            // Current time - centered in fixed width area
            let current_time_x = content_start_x + (current_time_width - current_time_ext.width()) / 2.0;
            let current_time_y = y + (height + current_time_ext.height()) / 2.0;
            
            // Ensure text is within bounds
            let current_time_x = current_time_x.max(content_start_x);
            let current_time_y = current_time_y.max(y + current_time_ext.height());
            
            
            // Draw current time with a background rectangle for visibility
            c.save().unwrap();
            c.set_font_size(16.0); // Increased font size for better visibility
            c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
            
            // Draw a small background rectangle behind the text for better visibility
            let bg_padding = 4.0;
            c.set_source_rgba(COLOR_TEXT_BACKGROUND.0, COLOR_TEXT_BACKGROUND.1, COLOR_TEXT_BACKGROUND.2, COLOR_TEXT_BACKGROUND.3);
            c.rectangle(
                current_time_x - bg_padding,
                current_time_y - current_time_ext.height() - bg_padding,
                current_time_ext.width() + bg_padding * 2.0,
                current_time_ext.height() + bg_padding * 2.0
            );
            c.fill().unwrap();
            
            // Draw the text
            c.set_source_rgba(COLOR_TEXT.0, COLOR_TEXT.1, COLOR_TEXT.2, COLOR_TEXT.3);
            c.move_to(current_time_x, current_time_y);
            c.show_text(&current_time_str).unwrap();
            c.restore().unwrap();
            
            // Progress bar - FIXED START POSITION (independent of text positioning)
            let progress_x = content_start_x + current_time_width + time_margin;
            let progress_y = y + 6.0;
            let progress_h = height - 12.0;
            
            let head_position = drag_position.unwrap_or(status.position);
            draw_background_service_player_progressbar(c, progress_x, progress_y, progress_w, progress_h, head_position, anim_progress, mpris_name);
            
            // Total time - positioned after fixed progress bar
            let total_time_x = progress_x + progress_w + time_margin + (total_time_width - total_time_ext.width()) / 2.0;
            let total_time_y = y + (height + total_time_ext.height()) / 2.0;
            
            // Ensure text is within bounds
            let total_time_x = total_time_x.max(progress_x + progress_w + time_margin);
            let total_time_y = total_time_y.max(y + total_time_ext.height());
            
            
            // Draw total time with a background rectangle for visibility
            c.save().unwrap();
            c.set_font_size(16.0); // Increased font size for better visibility
            c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
            
            // Draw a small background rectangle behind the text for better visibility
            let bg_padding = 4.0;
            c.set_source_rgba(COLOR_TEXT_BACKGROUND.0, COLOR_TEXT_BACKGROUND.1, COLOR_TEXT_BACKGROUND.2, COLOR_TEXT_BACKGROUND.3);
            c.rectangle(
                total_time_x - bg_padding,
                total_time_y - total_time_ext.height() - bg_padding,
                total_time_ext.width() + bg_padding * 2.0,
                total_time_ext.height() + bg_padding * 2.0
            );
            c.fill().unwrap();
            
            // Draw the text
            c.set_source_rgba(COLOR_TEXT.0, COLOR_TEXT.1, COLOR_TEXT.2, COLOR_TEXT.3);
            c.move_to(total_time_x, total_time_y);
            c.show_text(&total_time_str).unwrap();
            c.restore().unwrap();
            
            // Draw separator line between time display and control buttons
            let separator_x = progress_x + progress_w + time_margin + total_time_width + 10.0; // 10px after total time area
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

    pub fn hit_test_controls(&mut self, touch_x: f64, touch_y: f64, x: f64, y: f64, width: f64, height: f64, is_press: bool) -> Option<BackgroundServicePlayerAction> {
        // Background service player UI layout: [current_time] [progress_bar] [total_time] [prev] [play/pause] [next]
        
        let button_height = height * 1.0;
        let button_width = button_height * 2.2; // Wider buttons for better touch targets
        let button_spacing = 2.0; // No spacing between buttons to save space
        
        
        // Calculate total width of all buttons
        let total_buttons_width = (button_width * 3.0) + (button_spacing * 2.0);
        let buttons_start_x = x + width - total_buttons_width - 12.0; // 12px from right edge
        
        // Previous button - rectangular hit test for oval buttons
        let prev_x = buttons_start_x;
        let prev_y = y + (height - button_height) / 2.0;
        if touch_x >= prev_x && touch_x <= prev_x + button_width && 
           touch_y >= prev_y && touch_y <= prev_y + button_height {
            if is_press {
                self.pressed_button = Some(BackgroundServicePlayerAction::Previous);
            }
            return Some(BackgroundServicePlayerAction::Previous);
        }
        
        // Play/Pause button (main button) - rectangular hit test for oval buttons
        let main_button_x = prev_x + button_width + button_spacing;
        let main_button_y = prev_y;
        if touch_x >= main_button_x && touch_x <= main_button_x + button_width && 
           touch_y >= main_button_y && touch_y <= main_button_y + button_height {
            if is_press {
                self.pressed_button = Some(BackgroundServicePlayerAction::PlayPause);
            }
            return Some(BackgroundServicePlayerAction::PlayPause);
        }
        
        // Next button - rectangular hit test for oval buttons
        let next_x = main_button_x + button_width + button_spacing;
        let next_y = prev_y;
        if touch_x >= next_x && touch_x <= next_x + button_width && 
           touch_y >= next_y && touch_y <= next_y + button_height {
            if is_press {
                self.pressed_button = Some(BackgroundServicePlayerAction::Next);
            }
            return Some(BackgroundServicePlayerAction::Next);
        }
        
        // Check progress bar area - MATCH FIXED LAYOUT
        let content_start_x = x + 8.0; // Match drawing function
        let available_width = buttons_start_x - content_start_x - 8.0; // Match drawing function
        
        
        if let Some(status) = &self.last_status {
            // Use same fixed values as drawing function
            let current_time_width = 50.0; // Fixed width for current time
            let total_time_width = 50.0; // Fixed width for total time
            let time_margin = 8.0; // Fixed margin between elements
            
            // Calculate progress bar width based on fixed layout
            let total_timer_width = current_time_width + total_time_width + time_margin * 2.0; // 116px total
            let progress_w = available_width - total_timer_width; // Remaining space for progress bar
            
            
            let progress_x = content_start_x + current_time_width + time_margin; // Fixed progress bar position
            let progress_y = y + 6.0;
            let progress_h = height - 12.0;
            
            // Check if touch is on the progress bar area
            let hit_test_x_min = if self.is_dragging { progress_x - 100.0 } else { progress_x };
            let hit_test_x_max = if self.is_dragging { progress_x + progress_w + 100.0 } else { progress_x + progress_w };
            let hit_test_y_min = if self.is_dragging { progress_y - 20.0 } else { progress_y };
            let hit_test_y_max = if self.is_dragging { progress_y + progress_h + 20.0 } else { progress_y + progress_h };
            
            if touch_x >= hit_test_x_min && touch_x <= hit_test_x_max &&
               touch_y >= hit_test_y_min && touch_y <= hit_test_y_max {
                
                // Calculate progress ratio based on touch position
                // Clamp touch_x to the visual progress bar area for ratio calculation
                let clamped_touch_x = touch_x.clamp(progress_x, progress_x + progress_w);
                let raw_ratio = (clamped_touch_x - progress_x) / progress_w;
                let progress_ratio = raw_ratio.clamp(0.0, 1.0);
                
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
                
                // If we're already dragging, continue dragging but don't send seek commands
                if self.is_dragging {
                    return Some(BackgroundServicePlayerAction::DragHead(progress_ratio));
                }
                
                // For any other touch on progress bar, treat as seek (only when not dragging)
                return Some(BackgroundServicePlayerAction::Seek(progress_ratio));
            }
        }
        
        None
    }
    
    pub fn stop_dragging(&mut self) {
        self.is_dragging = false;
    }
    
    pub fn clear_pressed_button(&mut self) {
        self.pressed_button = None;
    }
}
