use cairo::{Context, Rectangle};
use rsvg::{Loader, CairoRenderer};
use crate::helper::MediaStatus;

// Icon paths for media controls (direct file paths)
const PLAY_ICON_PATH: &str = "/usr/share/tiny-dfr/icons/tiny-dfr-icons/symbolic/media/spotify/play.svg";
const PAUSE_ICON_PATH: &str = "/usr/share/tiny-dfr/icons/tiny-dfr-icons/symbolic/media/spotify/pause.svg";
const NEXT_ICON_PATH: &str = "/usr/share/tiny-dfr/icons/tiny-dfr-icons/symbolic/media/spotify/go-next-symbolic.svg";
const PREVIOUS_ICON_PATH: &str = "/usr/share/tiny-dfr/icons/tiny-dfr-icons/symbolic/media/spotify/go-previous-symbolic.svg";
const SPOTIFY_ICON_PATH: &str = "/usr/share/tiny-dfr/icons/tiny-dfr-icons/symbolic/media/spotify/media-playback-start-symbolic.svg";

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

pub struct SpotifyScreen {
    pub last_status: Option<MediaStatus>,
    pub is_dragging: bool,
}

impl SpotifyScreen {
    pub fn new() -> Self {
        Self {
            last_status: None,
            is_dragging: false,
        }
    }

    pub async fn update_status(&mut self) -> Option<MediaStatus> {
        // Status is now updated directly from the helper process
        self.last_status.clone()
    }
    
    pub fn reset_drag_state(&mut self) {
        self.is_dragging = false;
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

    fn draw_spotify_nav_button(&self, c: &Context, x: f64, y: f64, width: f64, height: f64, is_next: bool, anim_progress: f64) {
        c.save().unwrap();
        
        // Darker background for nav buttons
        let button_gradient = cairo::LinearGradient::new(x, y, x, y + height);
        button_gradient.add_color_stop_rgba(0.0, 0.15, 0.15, 0.15, anim_progress); // Dark gray
        button_gradient.add_color_stop_rgba(0.5, 0.12, 0.12, 0.12, anim_progress); // Darker gray
        button_gradient.add_color_stop_rgba(1.0, 0.08, 0.08, 0.08, anim_progress); // Even darker at bottom
        c.set_source(&button_gradient);
        
        // Fully rounded (circular) button
        let radius = height / 2.0; // Perfect circle
        c.new_sub_path();
        c.arc(x + width / 2.0, y + height / 2.0, radius, 0.0, 2.0 * std::f64::consts::PI);
        c.fill().unwrap();
        
        // Subtle border
        c.set_line_width(1.0);
        c.set_source_rgba(0.3, 0.3, 0.3, anim_progress * 0.6);
        c.stroke().unwrap();
        
        // Draw next/previous icon
        c.set_source_rgba(0.9, 0.9, 0.9, anim_progress); // Light gray
        let icon_center_x = x + width / 2.0;
        let icon_center_y = y + height / 2.0;
        let icon_size = 12.0;
        
        if is_next {
            // Draw next icon (right-pointing triangle)
            c.move_to(icon_center_x - icon_size / 2.0 + 1.0, icon_center_y - icon_size / 2.0);
            c.line_to(icon_center_x + icon_size / 2.0 + 1.0, icon_center_y);
            c.line_to(icon_center_x - icon_size / 2.0 + 1.0, icon_center_y + icon_size / 2.0);
            c.close_path();
            c.fill().unwrap();
        } else {
            // Draw previous icon (left-pointing triangle)
            c.move_to(icon_center_x + icon_size / 2.0 - 1.0, icon_center_y - icon_size / 2.0);
            c.line_to(icon_center_x - icon_size / 2.0 - 1.0, icon_center_y);
            c.line_to(icon_center_x + icon_size / 2.0 - 1.0, icon_center_y + icon_size / 2.0);
            c.close_path();
            c.fill().unwrap();
        }
        
        c.restore().unwrap();
    }

    fn draw_spotify_wrapper(&self, c: &Context, x: f64, y: f64, width: f64, height: f64, anim_progress: f64) {
        c.save().unwrap();
        
        // Flat dark background (no gradient for flat design)
        c.set_source_rgba(0.15, 0.15, 0.15, anim_progress); // Solid dark gray
        
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

    fn draw_icon_only_button(&self, c: &Context, x: f64, y: f64, width: f64, height: f64, is_playing: bool, anim_progress: f64) {
        c.save().unwrap();
        
        // Calculate icon position and size - smaller icons
        let icon_size = (width * 0.7).min(height * 0.7); // Smaller icons
        let icon_x = x + (width - icon_size) / 2.0;
        let icon_y = y + (height - icon_size) / 2.0;
        
        // Render SVG icon from direct file path
        let icon_path = if is_playing { PAUSE_ICON_PATH } else { PLAY_ICON_PATH };
        if let Err(e) = render_svg_icon_from_path(c, icon_path, icon_x, icon_y, icon_size) {
            eprintln!("Failed to render SVG icon from {}: {}", icon_path, e);
        }
        
        c.restore().unwrap();
    }

    fn draw_icon_only_nav_button(&self, c: &Context, x: f64, y: f64, width: f64, height: f64, is_next: bool, anim_progress: f64) {
        c.save().unwrap();
        
        // Calculate icon position and size - smaller icons
        let icon_size = (width * 0.7).min(height * 0.7); // Smaller icons
        let icon_x = x + (width - icon_size) / 2.0;
        let icon_y = y + (height - icon_size) / 2.0;
        
        // Render SVG icon from direct file path
        let icon_path = if is_next { NEXT_ICON_PATH } else { PREVIOUS_ICON_PATH };
        if let Err(e) = render_svg_icon_from_path(c, icon_path, icon_x, icon_y, icon_size) {
            eprintln!("Failed to render SVG icon from {}: {}", icon_path, e);
        }
        
        c.restore().unwrap();
    }

    fn draw_button_separator(&self, c: &Context, x: f64, y: f64, height: f64, anim_progress: f64) {
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

    fn draw_small_separator(&self, c: &Context, x: f64, y: f64, height: f64, anim_progress: f64) {
        c.save().unwrap();

        // Draw a small vertical line separator between buttons
        c.set_line_width(0.5);
        c.set_source_rgba(0.0, 0.0, 0.0, anim_progress * 0.8); // Black separator

        // Small separator - shorter than full height
        let separator_height = height * 0.8; // 60% of wrapper height
        let separator_y = y + (height - separator_height) / 2.0; // Centered vertically
        c.new_sub_path();
        c.move_to(x, separator_y);
        c.line_to(x, separator_y + separator_height);
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
            let button_height = pill_h * 0.9; // Much bigger buttons
            let button_width = button_height; // Circular
            let button_spacing = 30.0; // Even more spacing between buttons
            
            // Calculate total width of all buttons
            let total_buttons_width = (button_width * 3.0) + (button_spacing * 2.0);
            let buttons_start_x = pill_x + pill_w - total_buttons_width - 12.0; // 12px from right edge
            
            // Previous button
            let prev_x = buttons_start_x;
            let prev_y = pill_y + (pill_h - button_height) / 2.0;
            self.draw_icon_only_nav_button(c, prev_x, prev_y, button_width, button_height, false, anim_progress);
            
            // Small separator after previous button
            let prev_separator_x = prev_x + button_width + button_spacing / 2.0;
            self.draw_small_separator(c, prev_separator_x, pill_y, pill_h, anim_progress);
            
            // Play/Pause button (main button)
            let main_button_x = prev_x + button_width + button_spacing;
            let main_button_y = prev_y;
            self.draw_icon_only_button(c, main_button_x, main_button_y, button_width, button_height, status.is_playing, anim_progress);
            
            // Small separator after play/pause button
            let main_separator_x = main_button_x + button_width + button_spacing / 2.0;
            self.draw_small_separator(c, main_separator_x, pill_y, pill_h, anim_progress);
            
            // Next button
            let next_x = main_button_x + button_width + button_spacing;
            let next_y = prev_y;
            self.draw_icon_only_nav_button(c, next_x, next_y, button_width, button_height, true, anim_progress);
            
            let logo_separator_x = logo_x + logo_size + 10.0; // 10px before the logo
            self.draw_button_separator(c, logo_separator_x, pill_y, pill_h, anim_progress);

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
        let button_height = pill_h * 0.9; // Much bigger buttons (same as draw function)
        let button_width = button_height;
        let button_spacing = 30.0; // Even more spacing between buttons (same as draw function)
        let total_buttons_width = (button_width * 3.0) + (button_spacing * 2.0);
        let buttons_start_x = pill_x + pill_w - total_buttons_width - 12.0;
        
        // Previous button
        let prev_x = buttons_start_x;
        let prev_y = pill_y + (pill_h - button_height) / 2.0;
        let prev_center_x = prev_x + button_width / 2.0;
        let prev_center_y = prev_y + button_height / 2.0;
        let prev_distance = ((touch_x - prev_center_x).powi(2) + (touch_y - prev_center_y).powi(2)).sqrt();
        if prev_distance <= button_width / 2.0 {
            return Some(SpotifyAction::Previous);
        }
        
        // Play/Pause button
        let main_button_x = prev_x + button_width + button_spacing;
        let main_button_y = prev_y;
        let main_center_x = main_button_x + button_width / 2.0;
        let main_center_y = main_button_y + button_height / 2.0;
        let main_distance = ((touch_x - main_center_x).powi(2) + (touch_y - main_center_y).powi(2)).sqrt();
        if main_distance <= button_width / 2.0 {
            return Some(SpotifyAction::TogglePlayPause);
        }
        
        // Next button
        let next_x = main_button_x + button_width + button_spacing;
        let next_y = prev_y;
        let next_center_x = next_x + button_width / 2.0;
        let next_center_y = next_y + button_height / 2.0;
        let next_distance = ((touch_x - next_center_x).powi(2) + (touch_y - next_center_y).powi(2)).sqrt();
        if next_distance <= button_width / 2.0 {
            return Some(SpotifyAction::Next);
        }
        
        // Check progress bar - use same positioning as draw function
        let logo_size = pill_h * 0.4; // Same as draw function
        let logo_x = pill_x + 12.0;
        let logo_separator_x = logo_x + logo_size + 10.0; // After logo + separator
        let current_time_x = logo_separator_x + 10.0; // After separator
        let estimated_current_time_width = 55.0;
        let progress_x = current_time_x + estimated_current_time_width + 15.0;
        let progress_y = pill_y + 6.0;
        let progress_h = pill_h - 12.0;
        
        // Calculate progress bar width dynamically - use same calculation as draw function
        let available_width = buttons_start_x - logo_separator_x - 20.0;
        let total_time_margin = 15.0;
        let estimated_total_time_width = 55.0;
        let min_progress_width = 40.0;
        let progress_w = (available_width - estimated_current_time_width - estimated_total_time_width - total_time_margin - 25.0).max(min_progress_width);
        
        // Check if touch is on the progress bar area
        if touch_x >= progress_x && touch_x <= progress_x + progress_w &&
           touch_y >= progress_y && touch_y <= progress_y + progress_h {
            
            // Calculate progress ratio based on touch position
            let progress_ratio = (touch_x - progress_x) / progress_w;
            
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

#[derive(Debug, Clone)]
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
