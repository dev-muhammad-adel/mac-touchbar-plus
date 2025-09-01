use cairo::Context;
use crate::helper::MediaStatus;

// All constants removed as they were unused

pub struct VlcScreen {
    pub last_status: Option<MediaStatus>,
    pub is_dragging: bool,
}

impl VlcScreen {
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

    fn draw_apple_style_button(&self, c: &Context, x: f64, y: f64, width: f64, height: f64, is_playing: bool, anim_progress: f64) {
        c.save().unwrap();
        
        // Apple-style background (#3d3939 dark gray)
        let icon_gradient = cairo::LinearGradient::new(x, y, x, y + height);
        icon_gradient.add_color_stop_rgba(0.0, 0.239, 0.224, 0.224, anim_progress); // #3d3939
        icon_gradient.add_color_stop_rgba(0.5, 0.239, 0.224, 0.224, anim_progress); // #3d3939
        icon_gradient.add_color_stop_rgba(1.0, 0.239, 0.224, 0.224, anim_progress); // #3d3939
        c.set_source(&icon_gradient);
        
        // Rounded rectangle like the image (more rounded corners, better proportions)
        let radius = height * 0.3; // More rounded like the image
        c.new_path();
        c.move_to(x + radius, y);
        c.line_to(x + width - radius, y);
        c.curve_to(x + width, y, x + width, y, x + width, y + radius);
        c.line_to(x + width, y + height - radius);
        c.curve_to(x + width, y + height, x + width, y + height, x + width - radius, y + height);
        c.line_to(x + radius, y + height);
        c.curve_to(x, y + height, x, y + height, x, y + height - radius);
        c.line_to(x, y + radius);
        c.curve_to(x, y, x, y, x + radius, y);
        c.close_path();
        c.fill().unwrap();
        
        // Apple-style border (very subtle like the image)
        c.set_line_width(0.8);
        c.set_source_rgba(0.4, 0.4, 0.4, anim_progress * 0.4); // Darker border for dark background
        c.stroke().unwrap();
        
        // Draw Apple-style play/pause icon (bigger, centered in wider button)
        c.set_source_rgba(1.0, 1.0, 1.0, anim_progress); // Pure white like the image
        if is_playing {
            // Draw pause icon (two thick rectangles, Apple style)
            let bar_width = 7.0; // Thicker bars
            let bar_height = 22.0; // Taller bars
            let bar_spacing = 8.0; // More spacing
            let icon_center_x = x + width / 2.0;
            let icon_center_y = y + height / 2.0;
            let left_bar_x = icon_center_x - (bar_width * 2.0 + bar_spacing) / 2.0;
            let left_bar_y = icon_center_y - bar_height / 2.0;
            let right_bar_x = left_bar_x + bar_width + bar_spacing;
            
            // Left bar (rounded rectangle)
            c.rectangle(left_bar_x, left_bar_y, bar_width, bar_height);
            c.fill().unwrap();
            
            // Right bar (rounded rectangle)
            c.rectangle(right_bar_x, left_bar_y, bar_width, bar_height);
            c.fill().unwrap();
        } else {
            // Draw play icon (Apple-style triangle, bigger like the image)
            let icon_center_x = x + width / 2.0;
            let icon_center_y = y + height / 2.0;
            let triangle_size = 20.0; // Bigger triangle
            
            c.move_to(icon_center_x - triangle_size / 2.0 + 1.0, icon_center_y - triangle_size / 2.0);
            c.line_to(icon_center_x + triangle_size / 2.0 + 1.0, icon_center_y);
            c.line_to(icon_center_x - triangle_size / 2.0 + 1.0, icon_center_y + triangle_size / 2.0);
            c.close_path();
            c.fill().unwrap();
        }
        
        c.restore().unwrap();
    }

    pub fn draw(
        &mut self, // Changed to &mut self to update VU meter
        c: &Context,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f64,
        anim_progress: f64,
        drag_position: Option<f64>, // Add drag position parameter for visual feedback
    ) {
        // Calculate layout dimensions
        let pill_x = x;
        let pill_y = y - radius;
        let pill_w = width;
        let pill_h = height + radius * 2.0;

        if let Some(status) = &self.last_status {
            // macOS Touch Bar style layout: [icon] [current_time] [progress_bar] [total_time]
            
            // 1. Apple-style Play/Pause button (wider, bigger icon, dark background, full height)
            let icon_height = pill_h; // Full height - no vertical padding
            let icon_width = icon_height * 2.0; // Double width
            let icon_x = pill_x + 8.0;
            let icon_y = pill_y; // No vertical centering - use full height
            
            // Draw Apple-style button (wider, full height, dark background)
            self.draw_apple_style_button(c, icon_x, icon_y, icon_width, icon_height, status.is_playing, anim_progress);

            // 2. Current time (macOS system font style)
            let current_seconds = if status.duration > 0 {
                (status.position * status.duration as f64) as i64
            } else {
                0
            };
            let current_time_str = format!("{}:{:02}", current_seconds / 60, current_seconds % 60);
            
            // Calculate total time first to ensure proper spacing
            let total_time_seconds = status.duration;
            let total_time_str = format!("{}:{:02}", total_time_seconds / 60, total_time_seconds % 60);
            
            // Get text extents for both time displays to ensure proper spacing
            c.save().unwrap();
            c.set_font_size(18.0);
            c.select_font_face("SF Pro Display", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
            
            let current_time_ext = c.text_extents(&current_time_str).unwrap();
            let total_time_ext = c.text_extents(&total_time_str).unwrap();
            c.restore().unwrap();
            
            // Fixed spacing calculations to prevent overlap and cutoff
            let current_time_x = icon_x + icon_width + 20.0; // Fixed spacing from icon
            let current_time_y = pill_y + (pill_h + current_time_ext.height()) / 2.0;
            
            // Center the current time text within its allocated width
            let estimated_current_time_width = 60.0; // Fixed width for current time display
            let current_time_center_x = current_time_x + (estimated_current_time_width - current_time_ext.width()) / 2.0;
            
            // Draw current time
            c.save().unwrap();
            c.set_font_size(18.0);
            c.select_font_face("SF Pro Display", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
            c.set_source_rgba(0.95, 0.95, 0.95, anim_progress);
            c.move_to(current_time_center_x, current_time_y);
            c.show_text(&current_time_str).unwrap();
            c.restore().unwrap();

            // 3. macOS Touch Bar style progress bar with Adwaita dark theme
            // Use fixed width for current time to prevent progress bar width fluctuations
            let estimated_current_time_width = 60.0; // Fixed width for current time display
            let progress_x = current_time_x + estimated_current_time_width + 24.0; // Fixed spacing from current time
            let progress_y = pill_y + 2.0;
            let progress_h = pill_h - 4.0;
            
            // Calculate progress bar width to use full available space with proper margins
            let total_time_margin = 20.0; // Space between progress bar and total time
            // Use fixed estimated width for total time to prevent width fluctuations during dragging
            let estimated_total_time_width = 60.0; // Fixed width instead of measuring total_time_ext.width()
            let progress_w = pill_w - (progress_x - pill_x) - (estimated_total_time_width + total_time_margin + 16.0); // Use full available width minus margins
            
            // Ensure total time doesn't go beyond the right edge
            let actual_total_time_x = progress_x + progress_w + total_time_margin; // Position total time after progress bar
            
            // Simple solution: round the width to prevent sub-pixel fluctuations during dragging
            let progress_w = (progress_w * 100.0).round() / 100.0;
            
            // Adwaita dark wrapper background for progress bar area
            c.save().unwrap();
            c.set_source_rgba(0.235, 0.235, 0.235, anim_progress);
            let wrapper_radius = 6.0;
            c.new_sub_path();
            c.arc(progress_x + progress_w - wrapper_radius, progress_y + wrapper_radius, wrapper_radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
            c.arc(progress_x + progress_w - wrapper_radius, progress_y + progress_h - wrapper_radius, wrapper_radius, (0.0f64).to_radians(), (90.0f64).to_radians());
            c.arc(progress_x + wrapper_radius, progress_y + progress_h - wrapper_radius, wrapper_radius, (90.0f64).to_radians(), (180.0f64).to_radians());
            c.arc(progress_x + wrapper_radius, progress_y + wrapper_radius, wrapper_radius, (180.0f64).to_radians(), (270.0f64).to_radians());
            c.close_path();
            c.fill().unwrap();
            
            // Add subtle border to Adwaita dark wrapper
            c.set_line_width(1.0);
            c.set_source_rgba(0.4, 0.4, 0.4, anim_progress);
            c.stroke().unwrap();
            c.restore().unwrap();
            
            // Progress bar background inside Adwaita dark wrapper
            let inner_margin = 4.0;
            let inner_x = progress_x + inner_margin;
            let inner_y = progress_y + inner_margin;
            let inner_w = progress_w - (inner_margin * 2.0);
            let inner_h = progress_h - (inner_margin * 2.0);
            
            c.save().unwrap();
            c.set_source_rgba(0.157, 0.157, 0.157, anim_progress);
            let inner_radius = 4.0;
            c.new_sub_path();
            c.arc(inner_x + inner_w - inner_radius, inner_y + inner_radius, inner_radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
            c.arc(inner_x + inner_w - inner_radius, inner_y + inner_h - inner_radius, inner_radius, (0.0f64).to_radians(), (90.0f64).to_radians());
            c.arc(inner_x + inner_radius, inner_y + inner_h - inner_radius, inner_radius, (90.0f64).to_radians(), (180.0f64).to_radians());
            c.arc(inner_x + inner_radius, inner_y + inner_radius, inner_radius, (180.0f64).to_radians(), (270.0f64).to_radians());
            c.close_path();
            c.fill().unwrap();
            c.restore().unwrap();

            // Progress bar head (white, 6px wide, rounded)
            let head_position = drag_position.unwrap_or(status.position);
            if head_position > 0.0 {
                c.save().unwrap();
                c.set_source_rgba(1.0, 1.0, 1.0, anim_progress);
                let head_x = inner_x + (inner_w * head_position) - 3.0;
                let head_y = inner_y - 3.0;
                let head_radius = 3.0;
                c.new_sub_path();
                c.arc(head_x + 6.0 - head_radius, head_y + head_radius, head_radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
                c.arc(head_x + 6.0 - head_radius, head_y + inner_h + 6.0 - head_radius, head_radius, (0.0f64).to_radians(), (90.0f64).to_radians());
                c.arc(head_x + head_radius, head_y + inner_h + 6.0 - head_radius, head_radius, (90.0f64).to_radians(), (180.0f64).to_radians());
                c.arc(head_x + head_radius, head_y + head_radius, head_radius, (180.0f64).to_radians(), (270.0f64).to_radians());
                c.close_path();
                c.fill().unwrap();
                c.restore().unwrap();
            }

            // 4. Total time (macOS system font style) - positioned to prevent overlap
            c.save().unwrap();
            c.set_font_size(18.0);
            c.select_font_face("SF Pro Display", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
            c.set_source_rgba(0.95, 0.95, 0.95, anim_progress);
            
            // Center the total time text within its allocated width
            let total_time_center_x = actual_total_time_x + (estimated_total_time_width - total_time_ext.width()) / 2.0;
            let total_time_y = pill_y + (pill_h + total_time_ext.height()) / 2.0;
            c.move_to(total_time_center_x, total_time_y);
            c.show_text(&total_time_str).unwrap();
            c.restore().unwrap();
        } else {
            // Draw "VLC" text when no status available
            c.save().unwrap();
            c.set_font_size(14.0);
            c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
            c.set_source_rgba(1.0, 1.0, 1.0, anim_progress);
            
            let ext = c.text_extents("VLC").unwrap();
            let text_x = pill_x + (pill_w - ext.width()) / 2.0;
            let text_y = pill_y + (pill_h + ext.height()) / 2.0;
            
            c.move_to(text_x, text_y);
            c.show_text("VLC").unwrap();
            c.restore().unwrap();
        }
    }

    pub fn hit_test(&mut self, touch_x: f64, touch_y: f64, x: f64, y: f64, width: f64, height: f64, radius: f64) -> Option<VlcAction> {
        // touch_x and touch_y are now relative to the modules area
        // Use same calculation as draw function
        let pill_x = x; // Same as draw function
        let pill_y = y - radius; // Same as draw function
        let pill_w = width;
        let pill_h = height + radius * 2.0; // Same as draw function
        
        // Check if touch is within the pill area
        if touch_x < pill_x || touch_x > pill_x + pill_w || touch_y < pill_y || touch_y > pill_y + pill_h {
            return None;
        }
        
        // Check play/pause button
        let icon_height = pill_h; // Full height - no vertical padding
        let icon_width = icon_height * 2.0; // Double width
        let icon_x = pill_x + 8.0; // Use same positioning as drawing code
        let icon_y = pill_y; // Use same positioning as drawing code
        

        
        // Check if touch is within the rounded button area (matches visual button)
        let button_radius = icon_height * 0.3; // Same radius as visual button
        let button_tolerance = 2.0; // Add 2px tolerance for easier touch detection
        
        // First check if touch is within the bounding rectangle
        let in_bounds = touch_x >= icon_x - button_tolerance && touch_x <= icon_x + icon_width + button_tolerance &&
                       touch_y >= icon_y - button_tolerance && touch_y <= icon_y + icon_height + button_tolerance;
        

        
        if in_bounds {
            
            // Then check if touch is within the rounded rectangle
            let adjusted_x = touch_x - icon_x;
            let adjusted_y = touch_y - icon_y;
            
            // Check if touch is in corner areas that need special handling
            let in_left_corner = adjusted_x < button_radius;
            let in_right_corner = adjusted_x > icon_width - button_radius;
            let in_top_corner = adjusted_y < button_radius;
            let in_bottom_corner = adjusted_y > icon_height - button_radius;
            

            
            // If touch is in any corner, check if it's within the rounded corner
            if (in_left_corner || in_right_corner) && (in_top_corner || in_bottom_corner) {
                let corner_center_x = if in_left_corner { button_radius } else { icon_width - button_radius };
                let corner_center_y = if in_top_corner { button_radius } else { icon_height - button_radius };
                
                let distance = ((adjusted_x - corner_center_x).powi(2) + (adjusted_y - corner_center_y).powi(2)).sqrt();
                if distance > button_radius + button_tolerance {
                    return None;
                }
            }
            
            return Some(VlcAction::TogglePlayPause);
        }
        
        // Check progress bar (2px more vertical padding)
        let current_time_x = icon_x + icon_width + 12.0;
        let progress_x = current_time_x + 75.0; // Approximate width for current time with larger font
        let progress_y = pill_y + 2.0; // Use same positioning as drawing code
        let progress_h = pill_h - 4.0; // Reduced height for 2px padding on top and bottom
        
        // Calculate progress bar width dynamically to match the drawing function exactly
        // Use approximate values for hit testing since we don't have access to text extents here
        let total_time_margin = 20.0; // Same margin as drawing function
        let estimated_total_time_width = 60.0; // Estimated width for total time display
        let progress_w = pill_w - (progress_x - pill_x) - (estimated_total_time_width + total_time_margin + 16.0); // Use full available width minus margins
        
        // Check if touch is on the progress bar area
        if touch_x >= progress_x && touch_x <= progress_x + progress_w &&
           touch_y >= progress_y && touch_y <= progress_y + progress_h {
            
            // Calculate progress ratio based on touch position
            let progress_ratio = (touch_x - progress_x) / progress_w;
            
            // Check if we have a status to determine head position
            if let Some(status) = &self.last_status {
                // Calculate head position
                let head_x = progress_x + (progress_w * status.position) - 3.0; // 6px wide head, centered
                let head_y = progress_y - 3.0; // Extend head above and below progress bar
                let head_width = 6.0;
                let head_height = progress_h + 6.0;
                
                // Check if touch is on the head or very close to current position
                let current_position_x = progress_x + (progress_w * status.position);
                let distance_from_position = (touch_x - current_position_x).abs();
                
                // If touch is on head or within 15px of current position, treat as drag
                if (touch_x >= head_x && touch_x <= head_x + head_width &&
                    touch_y >= head_y && touch_y <= head_y + head_height) ||
                   distance_from_position <= 15.0 {
                    self.is_dragging = true;
                    return Some(VlcAction::DragHead(progress_ratio));
                }
                
                // If we're already dragging, continue dragging regardless of position
                if self.is_dragging {
                    return Some(VlcAction::DragHead(progress_ratio));
                }
            }
            
            // For any other touch on progress bar, treat as seek
            return Some(VlcAction::Seek(progress_ratio));
        }
        
        None
    }
}

#[derive(Debug, Clone)]
pub enum VlcAction {
    TogglePlayPause,
    Seek(f64), // 0.0 to 1.0
    DragHead(f64), // 0.0 to 1.0 - for dragging the progress bar head
    Next,
    Previous,
    Stop,
    Raise,
    Quit,
} 