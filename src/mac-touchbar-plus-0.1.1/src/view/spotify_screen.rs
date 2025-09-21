use cairo::{Context, Rectangle};
use rsvg::{Loader, CairoRenderer};
use crate::helper::MediaStatus;

// Icon paths for media controls (direct file paths)
const PLAY_ICON_PATH: &str = "/usr/share/tiny-dfr/icons/tiny-dfr-icons/symbolic/media/spotify/play.svg";
const PAUSE_ICON_PATH: &str = "/usr/share/tiny-dfr/icons/tiny-dfr-icons/symbolic/media/spotify/pause.svg";
const NEXT_ICON_PATH: &str = "/usr/share/tiny-dfr/icons/tiny-dfr-icons/symbolic/media/spotify/go-next-symbolic.svg";
const PREVIOUS_ICON_PATH: &str = "/usr/share/tiny-dfr/icons/tiny-dfr-icons/symbolic/media/spotify/go-previous-symbolic.svg";
const SPOTIFY_ICON_PATH: &str = "/usr/share/tiny-dfr/icons/tiny-dfr-icons/symbolic/media/spotify/media-playback-start-symbolic.svg";

// Color constants for Spotify screen
const SPOTIFY_BUTTON_GRADIENT_START: (f64, f64, f64, f64) = (0.11, 0.73, 0.33, 1.0); // #1DB954 (Spotify green)
const SPOTIFY_BUTTON_GRADIENT_END: (f64, f64, f64, f64) = (0.08, 0.55, 0.25, 1.0); // Darker green at bottom
const SPOTIFY_BUTTON_BORDER: (f64, f64, f64, f64) = (1.0, 1.0, 1.0, 0.4); // Subtle white border
const SPOTIFY_ICON_COLOR: (f64, f64, f64, f64) = (1.0, 1.0, 1.0, 1.0); // Pure white
const SPOTIFY_DARK_BUTTON_GRADIENT_START: (f64, f64, f64, f64) = (0.15, 0.15, 0.15, 1.0); // Dark gray
const SPOTIFY_DARK_BUTTON_GRADIENT_MID: (f64, f64, f64, f64) = (0.12, 0.12, 0.12, 1.0); // Darker gray
const SPOTIFY_DARK_BUTTON_GRADIENT_END: (f64, f64, f64, f64) = (0.08, 0.08, 0.08, 1.0); // Even darker at bottom
const SPOTIFY_DARK_BUTTON_BORDER: (f64, f64, f64, f64) = (0.3, 0.3, 0.3, 0.6); // Gray border
const SPOTIFY_DARK_ICON_COLOR: (f64, f64, f64, f64) = (0.9, 0.9, 0.9, 1.0); // Light gray
const SPOTIFY_PROGRESS_BACKGROUND: (f64, f64, f64, f64) = (0.3, 0.3, 0.3, 1.0); // Solid dark gray
const SPOTIFY_PROGRESS_FILL_START: (f64, f64, f64, f64) = (0.11, 0.73, 0.33, 1.0); // #1DB954 (Spotify green)
const SPOTIFY_PROGRESS_FILL_END: (f64, f64, f64, f64) = (0.15, 0.8, 0.4, 1.0); // Lighter green at end
const SPOTIFY_PROGRESS_HEAD: (f64, f64, f64, f64) = (1.0, 1.0, 1.0, 1.0); // White progress head
const SPOTIFY_PROGRESS_HEAD_BORDER: (f64, f64, f64, f64) = (0.11, 0.73, 0.33, 1.0); // #1DB954 (Spotify green)
const SPOTIFY_TEXT_COLOR: (f64, f64, f64, f64) = (0.9, 0.9, 0.9, 1.0); // Light gray for modern look
const SPOTIFY_ACCENT_COLOR: (f64, f64, f64, f64) = (0.11, 0.73, 0.33, 1.0); // Spotify green

// Helper function to render SVG icon from direct file path
fn render_svg_icon_from_path(c: &Context, icon_path: &str, x: f64, y: f64, size: f64) -> Result<(), Box<dyn std::error::Error>> {
    let loader = Loader::new();
    let handle = loader.read_path(icon_path)?;
    let renderer = CairoRenderer::new(&handle);
    
    let rect = Rectangle::new(x, y, size, size);
    renderer.render_document(c, &rect)?;
    
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

#[derive(Debug, Clone)]
enum ButtonType {
    PlayPause(bool), // bool indicates if currently playing
    Previous,
    Next,
}

pub struct SpotifyScreen {
    pub last_status: Option<MediaStatus>,
    pub is_dragging: bool,
    pub pressed_button: Option<SpotifyAction>,
}

impl SpotifyScreen {
    pub fn new() -> Self {
        Self {
            last_status: None,
            is_dragging: false,
            pressed_button: None,
        }
    }

    pub async fn update_status(&mut self) -> Option<MediaStatus> {
        // Status is now updated directly from the helper process
        self.last_status.clone()
    }
    
    pub fn reset_drag_state(&mut self) {
        self.is_dragging = false;
    }

    pub fn clear_pressed_button(&mut self) {
        self.pressed_button = None;
    }

    fn draw_spotify_button(&self, c: &Context, x: f64, y: f64, width: f64, height: f64, is_playing: bool, anim_progress: f64) {
        c.save().unwrap();
        
        // Spotify green gradient background
        let button_gradient = cairo::LinearGradient::new(x, y, x, y + height);
        button_gradient.add_color_stop_rgba(0.0, 0.11, 0.73, 0.33, anim_progress); // #1DB954 (Spotify green)
        button_gradient.add_color_stop_rgba(0.5, 0.11, 0.73, 0.33, anim_progress); // #1DB954 (Spotify green)
        button_gradient.add_color_stop_rgba(1.0, 0.08, 0.55, 0.25, anim_progress); // Darker green at bottom
        c.set_source(&button_gradient);
        
        // Fully rounded (circular) button for cooler look
        let radius = height / 2.0; // Perfect circle
        c.new_sub_path();
        c.arc(x + width / 2.0, y + height / 2.0, radius, 0.0, 2.0 * std::f64::consts::PI);
        c.fill().unwrap();
        
        // Subtle white border for modern look
        c.set_line_width(1.5);
        c.set_source_rgba(1.0, 1.0, 1.0, anim_progress * 0.4); // Subtle white border
        c.stroke().unwrap();
        
        // Draw Spotify-style play/pause icon
        c.set_source_rgba(1.0, 1.0, 1.0, anim_progress); // Pure white
        if is_playing {
            // Draw pause icon (two rounded rectangles, modern Spotify style)
            let bar_width = 5.0; // Thinner bars for modern look
            let bar_height = 18.0; // Taller bars
            let bar_spacing = 5.0; // Less spacing for modern look
            let icon_center_x = x + width / 2.0;
            let icon_center_y = y + height / 2.0;
            let left_bar_x = icon_center_x - (bar_width * 2.0 + bar_spacing) / 2.0;
            let left_bar_y = icon_center_y - bar_height / 2.0;
            let right_bar_x = left_bar_x + bar_width + bar_spacing;
            
            // Left bar (rounded rectangle)
            c.new_sub_path();
            c.arc(left_bar_x + bar_width/2.0, left_bar_y + bar_width/2.0, bar_width/2.0, 180.0_f64.to_radians(), 270.0_f64.to_radians());
            c.arc(right_bar_x - bar_width/2.0, left_bar_y + bar_width/2.0, bar_width/2.0, 270.0_f64.to_radians(), 0.0_f64.to_radians());
            c.arc(right_bar_x - bar_width/2.0, left_bar_y + bar_height - bar_width/2.0, bar_width/2.0, 0.0_f64.to_radians(), 90.0_f64.to_radians());
            c.arc(left_bar_x + bar_width/2.0, left_bar_y + bar_height - bar_width/2.0, bar_width/2.0, 90.0_f64.to_radians(), 180.0_f64.to_radians());
            c.close_path();
            c.fill().unwrap();
            
            // Right bar (rounded rectangle)
            c.new_sub_path();
            c.arc(right_bar_x + bar_width/2.0, left_bar_y + bar_width/2.0, bar_width/2.0, 180.0_f64.to_radians(), 270.0_f64.to_radians());
            c.arc(right_bar_x + bar_width - bar_width/2.0, left_bar_y + bar_width/2.0, bar_width/2.0, 270.0_f64.to_radians(), 0.0_f64.to_radians());
            c.arc(right_bar_x + bar_width - bar_width/2.0, left_bar_y + bar_height - bar_width/2.0, bar_width/2.0, 0.0_f64.to_radians(), 90.0_f64.to_radians());
            c.arc(right_bar_x + bar_width/2.0, left_bar_y + bar_height - bar_width/2.0, bar_width/2.0, 90.0_f64.to_radians(), 180.0_f64.to_radians());
            c.close_path();
            c.fill().unwrap();
        } else {
            // Draw play icon (modern triangle, Spotify style)
            let icon_center_x = x + width / 2.0;
            let icon_center_y = y + height / 2.0;
            let triangle_size = 16.0; // Smaller for modern look
            
            c.move_to(icon_center_x - triangle_size / 2.0 + 2.0, icon_center_y - triangle_size / 2.0);
            c.line_to(icon_center_x + triangle_size / 2.0 + 2.0, icon_center_y);
            c.line_to(icon_center_x - triangle_size / 2.0 + 2.0, icon_center_y + triangle_size / 2.0);
            c.close_path();
            c.fill().unwrap();
        }
        
        c.restore().unwrap();
    }


    fn draw_spotify_wrapper(&self, c: &Context, x: f64, y: f64, width: f64, height: f64, anim_progress: f64) {
        c.save().unwrap();
        
        // Flat dark background (no gradient for flat design)
        c.set_source_rgba(SPOTIFY_PROGRESS_BACKGROUND.0, SPOTIFY_PROGRESS_BACKGROUND.1, SPOTIFY_PROGRESS_BACKGROUND.2, anim_progress * SPOTIFY_PROGRESS_BACKGROUND.3);
        
        // Less rounded rectangle for the wrapper
        let radius = height / 6.0; // Much less rounded (was height / 2.0)
        c.new_sub_path();
        c.arc(x + width - radius, y + radius, radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
        c.arc(x + width - radius, y + height - radius, radius, (0.0f64).to_radians(), (90.0f64).to_radians());
        c.arc(x + radius, y + height - radius, radius, (90.0f64).to_radians(), (180.0f64).to_radians());
        c.arc(x + radius, y + radius, radius, (180.0f64).to_radians(), (270.0f64).to_radians());
        c.close_path();
        c.fill().unwrap();
        
        // No border for flat design
        
        c.restore().unwrap();
    }

    fn draw_spotify_logo(&self, c: &Context, x: f64, y: f64, size: f64, anim_progress: f64) {
        c.save().unwrap();
        
        // Calculate icon position and size
        let icon_size = size ; // Use most of the available space
        let icon_x = x + (size - icon_size) / 2.0;
        let icon_y = y + (size - icon_size) / 2.0;
        
        // Render SVG icon from direct file path
        if let Err(e) = render_svg_icon_from_path(c, SPOTIFY_ICON_PATH, icon_x, icon_y, icon_size) {
            eprintln!("Failed to render Spotify SVG icon from {}: {}", SPOTIFY_ICON_PATH, e);
        }
        
        c.restore().unwrap();
    }

    fn draw_icon_only_button(&self, c: &Context, x: f64, y: f64, width: f64, height: f64, button_type: ButtonType, anim_progress: f64, is_pressed: bool) {
        c.save().unwrap();
        
        // Draw button background with visual feedback
        if is_pressed {
            // Pressed state - darker rectangular background
            c.set_source_rgba(SPOTIFY_PROGRESS_BACKGROUND.0 * 0.7, SPOTIFY_PROGRESS_BACKGROUND.1 * 0.7, SPOTIFY_PROGRESS_BACKGROUND.2 * 0.7, anim_progress);
            c.rectangle(x, y, width, height);
            c.fill().unwrap();
        }
        
        // Calculate icon position and size - smaller icons
        let icon_size = (width * 0.7).min(height * 0.7); // Smaller icons
        let icon_x = x + (width - icon_size) / 2.0;
        let icon_y = y + (height - icon_size) / 2.0;
        
        // Render SVG icon from direct file path
        let icon_path = match button_type {
            ButtonType::PlayPause(is_playing) => if is_playing { PAUSE_ICON_PATH } else { PLAY_ICON_PATH },
            ButtonType::Previous => PREVIOUS_ICON_PATH,
            ButtonType::Next => NEXT_ICON_PATH,
        };
        if let Err(e) = render_svg_icon_from_path(c, icon_path, icon_x, icon_y, icon_size) {
            eprintln!("Failed to render SVG icon from {}: {}", icon_path, e);
        }
        
        c.restore().unwrap();
    }


    fn draw_separator(&self, c: &Context, x: f64, y: f64, height: f64, anim_progress: f64) {
        c.save().unwrap();

        // Draw a vertical line separator that reaches the full wrapper height
        c.set_line_width(1.0);
        c.set_source_rgba(0.0, 0.0, 0.0, anim_progress * 0.8); // Lower contrast, less visible

        // Full height separator from top to bottom of wrapper
        c.new_sub_path();
        c.move_to(x, y);
        c.line_to(x, y + height);
        c.stroke().unwrap();

        c.restore().unwrap();
    }


    fn draw_spotify_progress_bar(&self, c: &Context, x: f64, y: f64, width: f64, height: f64, progress: f64, anim_progress: f64) {
        c.save().unwrap();
        
        // Progress bar background (dark gray)
        c.set_source_rgba(0.2, 0.2, 0.2, anim_progress); // Dark background
        let radius = height / 8.0; // Small rounded corners
        c.new_sub_path();
        c.arc(x + width - radius, y + radius, radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
        c.arc(x + width - radius, y + height - radius, radius, (0.0f64).to_radians(), (90.0f64).to_radians());
        c.arc(x + radius, y + height - radius, radius, (90.0f64).to_radians(), (180.0f64).to_radians());
        c.arc(x + radius, y + radius, radius, (180.0f64).to_radians(), (270.0f64).to_radians());
        c.close_path();
        c.fill().unwrap();
        
        // Black border around progress bar
        c.set_source_rgba(0.0, 0.0, 0.0, anim_progress); // Black border
        c.set_line_width(1.0);
        c.new_sub_path();
        c.arc(x + width - radius, y + radius, radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
        c.arc(x + width - radius, y + height - radius, radius, (0.0f64).to_radians(), (90.0f64).to_radians());
        c.arc(x + radius, y + height - radius, radius, (90.0f64).to_radians(), (180.0f64).to_radians());
        c.arc(x + radius, y + radius, radius, (180.0f64).to_radians(), (270.0f64).to_radians());
        c.close_path();
        c.stroke().unwrap();
        
        // Progress fill with Spotify green gradient
        if progress > 0.0 {
            let fill_width = width * progress;
            let fill_gradient = cairo::LinearGradient::new(x, y, x + fill_width, y);
            fill_gradient.add_color_stop_rgba(0.0, 0.11, 0.73, 0.33, anim_progress); // #1DB954 (Spotify green)
            fill_gradient.add_color_stop_rgba(1.0, 0.15, 0.8, 0.4, anim_progress); // Lighter green at end
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
            
            // Green inner circle
            c.set_source_rgba(0.11, 0.73, 0.33, anim_progress); // #1DB954 (Spotify green)
            c.new_sub_path();
            c.arc(head_x, head_y, head_radius - 2.0, 0.0, 2.0 * std::f64::consts::PI);
            c.fill().unwrap();
        }
        
        c.restore().unwrap();
    }

    pub fn draw(
        &mut self,
        c: &Context,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f64,
        anim_progress: f64,
        drag_position: Option<f64>,
    ) {
        // Calculate layout dimensions
        let pill_x = x;
        let pill_y = y - radius;
        let pill_w = width;
        let pill_h = height + radius * 2.0;

        if let Some(status) = &self.last_status {
            // New Spotify layout: [Spotify Logo] [current_time] [progress_bar] [total_time] [prev] [play/pause] [next]
            
            // 1. Draw main wrapper/container background
            self.draw_spotify_wrapper(c, pill_x, pill_y, pill_w, pill_h, anim_progress);
            
            // 2. Spotify logo on the left
            let logo_size = pill_h * 0.8; // Logo size
            let logo_x = pill_x + 12.0;
            let logo_y = pill_y + (pill_h - logo_size) / 2.0;
            
            // Draw cool Spotify logo
            self.draw_spotify_logo(c, logo_x, logo_y, logo_size, anim_progress);
            
            // 3. Control buttons on the right
            let button_height = pill_h * 1.0; // Full height buttons
            let button_width = button_height * 2.2; // Wider rectangular buttons
            let button_spacing = 5.0; // Spacing between buttons
            
            // Calculate total width of all buttons
            let total_buttons_width = (button_width * 3.0) + (button_spacing * 2.0);
            let buttons_start_x = pill_x + pill_w - total_buttons_width - 12.0; // 12px from right edge
            
            // Previous button
            let prev_x = buttons_start_x;
            let prev_y = pill_y + (pill_h - button_height) / 2.0;
            let is_prev_pressed = self.pressed_button == Some(SpotifyAction::Previous) && !self.is_dragging;
            self.draw_icon_only_button(c, prev_x, prev_y, button_width, button_height, ButtonType::Previous, anim_progress, is_prev_pressed);
            
            // Small separator after previous button
            let prev_separator_x = prev_x + button_width + button_spacing / 2.0;
            self.draw_separator(c, prev_separator_x, pill_y, pill_h, anim_progress);
            
            // Play/Pause button (main button)
            let main_button_x = prev_x + button_width + button_spacing;
            let main_button_y = prev_y;
            let is_play_pause_pressed = self.pressed_button == Some(SpotifyAction::TogglePlayPause) && !self.is_dragging;
            self.draw_icon_only_button(c, main_button_x, main_button_y, button_width, button_height, ButtonType::PlayPause(status.is_playing), anim_progress, is_play_pause_pressed);
            
            // Small separator after play/pause button
            let main_separator_x = main_button_x + button_width + button_spacing / 2.0;
            self.draw_separator(c, main_separator_x, pill_y, pill_h, anim_progress);
            
            // Next button
            let next_x = main_button_x + button_width + button_spacing;
            let next_y = prev_y;
            let is_next_pressed = self.pressed_button == Some(SpotifyAction::Next) && !self.is_dragging;
            self.draw_icon_only_button(c, next_x, next_y, button_width, button_height, ButtonType::Next, anim_progress, is_next_pressed);
            
            
            let logo_separator_x = logo_x + logo_size + 10.0; // 10px before the logo
            self.draw_separator(c, logo_separator_x, pill_y, pill_h, anim_progress);

            // 4. Current time (Spotify-style typography)
            let current_seconds = if status.duration > 0 {
                (status.position * status.duration as f64) as i64
            } else {
                0
            };
            let current_time_str = format_duration(current_seconds);
            
            // Calculate total time for spacing
            let total_time_seconds = status.duration;
            let total_time_str = format_duration(total_time_seconds);
            
            // Get text extents
            c.save().unwrap();
            c.set_font_size(16.0); // Slightly smaller for modern look
            c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
            
            let current_time_ext = c.text_extents(&current_time_str).unwrap();
            let total_time_ext = c.text_extents(&total_time_str).unwrap();
            c.restore().unwrap();
            
            // Fixed spacing calculations - adjust for new layout with logo and buttons
            let logo_end_x = logo_x + logo_size + 15.0; // Space after logo
            let buttons_start_x = pill_x + pill_w - total_buttons_width - 12.0;
            let available_width = buttons_start_x - logo_end_x - 20.0; // Available space between logo and buttons
            
            // Center the time and progress bar in the available space
            let current_time_x = logo_end_x + 10.0;
            let current_time_y = pill_y + (pill_h + current_time_ext.height()) / 2.0;
            
            // Center the current time text
            let estimated_current_time_width = 55.0; // Fixed width
            let current_time_center_x = current_time_x + (estimated_current_time_width - current_time_ext.width()) / 2.0;
            
            // Draw current time with Spotify-style color
            c.save().unwrap();
            c.set_font_size(16.0);
            c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
            c.set_source_rgba(0.9, 0.9, 0.9, anim_progress); // Light gray for modern look
            c.move_to(current_time_center_x, current_time_y);
            c.show_text(&current_time_str).unwrap();
            c.restore().unwrap();

            // 5. Spotify-style progress bar
            let progress_x = current_time_x + estimated_current_time_width + 15.0; // Less spacing for compact layout
            let progress_y = pill_y + 6.0; // More padding
            let progress_h = pill_h - 12.0; // More padding
            
            // Calculate progress bar width - use available space between time and buttons
            let total_time_margin = 15.0; // Less space for compact layout
            let estimated_total_time_width = 55.0; // Fixed width
            let min_progress_width = 40.0; // Smaller minimum for compact layout
            let progress_w = (available_width - estimated_current_time_width - estimated_total_time_width - total_time_margin - 25.0).max(min_progress_width);
            
            // Ensure total time doesn't go beyond the right edge
            let actual_total_time_x = progress_x + progress_w + total_time_margin;
            
            // Round the width to prevent sub-pixel fluctuations
            let progress_w = (progress_w * 100.0).round() / 100.0;
            
            // Draw Spotify progress bar
            let head_position = drag_position.unwrap_or(status.position);
            self.draw_spotify_progress_bar(c, progress_x, progress_y, progress_w, progress_h, head_position, anim_progress);

            // 6. Total time - positioned to prevent overlap
            c.save().unwrap();
            c.set_font_size(16.0);
            c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
            c.set_source_rgba(0.9, 0.9, 0.9, anim_progress); // Light gray for modern look
            
            // Center the total time text
            let total_time_center_x = actual_total_time_x + (estimated_total_time_width - total_time_ext.width()) / 2.0;
            let total_time_y = pill_y + (pill_h + total_time_ext.height()) / 2.0;
            c.move_to(total_time_center_x, total_time_y);
            c.show_text(&total_time_str).unwrap();
            c.restore().unwrap();
            
            // Separator after total time (end time)
            let total_time_end_x = actual_total_time_x + estimated_total_time_width + 10.0; // 10px after the total time
            self.draw_separator(c, total_time_end_x, pill_y, pill_h, anim_progress);
            
            // Draw current time text with background
            c.save().unwrap();
            c.set_font_size(16.0);
            c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
            
            // Draw text background
            let text_bg_padding = 4.0;
            let text_bg_x = current_time_center_x - text_bg_padding;
            let text_bg_y = current_time_y - current_time_ext.height() - text_bg_padding;
            let text_bg_w = current_time_ext.width() + (text_bg_padding * 2.0);
            let text_bg_h = current_time_ext.height() + (text_bg_padding * 2.0);
            
            c.set_source_rgba(SPOTIFY_PROGRESS_BACKGROUND.0, SPOTIFY_PROGRESS_BACKGROUND.1, SPOTIFY_PROGRESS_BACKGROUND.2, anim_progress * SPOTIFY_PROGRESS_BACKGROUND.3);
            c.rectangle(text_bg_x, text_bg_y, text_bg_w, text_bg_h);
            c.fill().unwrap();
            
            // Draw text on top
            c.set_source_rgba(0.9, 0.9, 0.9, anim_progress); // Light gray for modern look
            c.move_to(current_time_center_x, current_time_y);
            c.show_text(&current_time_str).unwrap();
            c.restore().unwrap();
        } else {
            // Draw "Spotify" text when no status available
            c.save().unwrap();
            c.set_font_size(16.0);
            c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
            c.set_source_rgba(0.11, 0.73, 0.33, anim_progress); // Spotify green
            
            let ext = c.text_extents("Spotify").unwrap();
            let text_x = pill_x + (pill_w - ext.width()) / 2.0;
            let text_y = pill_y + (pill_h + ext.height()) / 2.0;
            
            c.move_to(text_x, text_y);
            c.show_text("Spotify").unwrap();
            c.restore().unwrap();
        }
    }

    pub fn hit_test(&mut self, touch_x: f64, touch_y: f64, x: f64, y: f64, width: f64, height: f64, radius: f64) -> Option<SpotifyAction> {
        // Use same calculation as draw function
        let pill_x = x;
        let pill_y = y - radius;
        let pill_w = width;
        let pill_h = height + radius * 2.0;
        
        // Check if touch is within the pill area
        if touch_x < pill_x || touch_x > pill_x + pill_w || touch_y < pill_y || touch_y > pill_y + pill_h {
            return None;
        }
        
        // Check control buttons on the right side
        let button_height = pill_h * 1.0; // Full height buttons (same as draw function)
        let button_width = button_height * 2.2; // Wider rectangular buttons (same as draw function)
        let button_spacing = 5.0; // Spacing between buttons (same as draw function)
        let total_buttons_width = (button_width * 3.0) + (button_spacing * 2.0);
        let buttons_start_x = pill_x + pill_w - total_buttons_width - 12.0;
        
        // Previous button
        let prev_x = buttons_start_x;
        let prev_y = pill_y + (pill_h - button_height) / 2.0;
        if touch_x >= prev_x && touch_x <= prev_x + button_width &&
           touch_y >= prev_y && touch_y <= prev_y + button_height {
            // If progress bar is being dragged, ignore control button touches
            if self.is_dragging {
                return None;
            }
            self.pressed_button = Some(SpotifyAction::Previous);
            return Some(SpotifyAction::Previous);
        }
        
        // Play/Pause button
        let main_button_x = prev_x + button_width + button_spacing;
        let main_button_y = prev_y;
        if touch_x >= main_button_x && touch_x <= main_button_x + button_width &&
           touch_y >= main_button_y && touch_y <= main_button_y + button_height {
            // If progress bar is being dragged, ignore control button touches
            if self.is_dragging {
                return None;
            }
            self.pressed_button = Some(SpotifyAction::TogglePlayPause);
            return Some(SpotifyAction::TogglePlayPause);
        }
        
        // Next button
        let next_x = main_button_x + button_width + button_spacing;
        let next_y = prev_y;
        if touch_x >= next_x && touch_x <= next_x + button_width &&
           touch_y >= next_y && touch_y <= next_y + button_height {
            // If progress bar is being dragged, ignore control button touches
            if self.is_dragging {
                return None;
            }
            self.pressed_button = Some(SpotifyAction::Next);
            return Some(SpotifyAction::Next);
        }
        
        // Check progress bar - use exact same positioning as draw function
        let logo_size = pill_h * 0.4; // Same as draw function
        let logo_x = pill_x + 12.0;
        let logo_end_x = logo_x + logo_size + 15.0; // Space after logo (same as draw function)
        let current_time_x = logo_end_x + 10.0; // After logo (same as draw function)
        let estimated_current_time_width = 55.0; // Fixed width (same as draw function)
        let progress_x = current_time_x + estimated_current_time_width + 15.0; // Less spacing for compact layout (same as draw function)
        let progress_y = pill_y + 6.0; // More padding (same as draw function)
        let progress_h = pill_h - 12.0; // More padding (same as draw function)
        
        // Calculate progress bar width - use same calculation as draw function
        let available_width = buttons_start_x - logo_end_x - 20.0; // Available space between logo and buttons (same as draw function)
        let total_time_margin = 15.0; // Less space for compact layout (same as draw function)
        let estimated_total_time_width = 55.0; // Fixed width (same as draw function)
        let min_progress_width = 40.0; // Smaller minimum for compact layout (same as draw function)
        let progress_w = (available_width - estimated_current_time_width - estimated_total_time_width - total_time_margin - 25.0).max(min_progress_width);
        
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
            
            // Check if we have a status to determine head position
            if let Some(status) = &self.last_status {
                // Calculate head position
                let head_x = progress_x + (progress_w * status.position);
                let head_y = progress_y + progress_h / 2.0;
                let head_radius = 8.0;
                
                // Check if touch is on the head or very close to current position
                let current_position_x = progress_x + (progress_w * status.position);
                let distance_from_position = (touch_x - current_position_x).abs();
                
                // If touch is on head or within 20px of current position, treat as drag
                if (touch_x >= head_x - head_radius && touch_x <= head_x + head_radius &&
                    touch_y >= head_y - head_radius && touch_y <= head_y + head_radius) ||
                   distance_from_position <= 20.0 {
                    self.is_dragging = true;
                    return Some(SpotifyAction::DragHead(progress_ratio));
                }
                
                // If we're already dragging, continue dragging regardless of position
                if self.is_dragging {
                    return Some(SpotifyAction::DragHead(progress_ratio));
                }
            }
            
            // For any other touch on progress bar, treat as seek
            return Some(SpotifyAction::Seek(progress_ratio));
        }
        
        None
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpotifyAction {
    TogglePlayPause,
    Seek(f64), // 0.0 to 1.0
    DragHead(f64), // 0.0 to 1.0 - for dragging the progress bar head
    Next,
    Previous,
    Stop,
    Raise,
    Quit,
}
