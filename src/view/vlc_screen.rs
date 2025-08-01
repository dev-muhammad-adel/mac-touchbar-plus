use cairo::Context;
use crate::helper::VlcStatus;
use std::time::{SystemTime, UNIX_EPOCH};

const BUTTON_COLOR_ACTIVE: f64 = 0.600;
const BUTTON_COLOR_INACTIVE: f64 = 0.350;
const PROGRESS_BAR_HEIGHT: f64 = 4.0;
const PLAY_PAUSE_BUTTON_SIZE: f64 = 32.0;

pub struct VlcScreen {
    pub last_status: Option<VlcStatus>,
    pub is_dragging: bool,
    // VU meter state
    vu_meter_bars: [f64; 20], // 20 bars for VU meter
    last_vu_update: u64,
}

impl VlcScreen {
    pub fn new() -> Self {
        VlcScreen {
            last_status: None,
            is_dragging: false,
            vu_meter_bars: [0.0; 20],
            last_vu_update: 0,
        }
    }

    pub async fn update_status(&mut self) -> Option<VlcStatus> {
        // Status is now updated directly from the helper process
        self.last_status.clone()
    }
    
    pub fn reset_drag_state(&mut self) {
        self.is_dragging = false;
    }

    // Update VU meter bars with animated audio levels
    fn update_vu_meter(&mut self, is_playing: bool) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        
        // Update every 50ms for smooth animation
        if now - self.last_vu_update > 50 {
            self.last_vu_update = now;
            
            if is_playing {
                // Generate animated VU meter levels
                for i in 0..20 {
                    // Create a wave-like pattern with some randomness
                    let base_level = 0.3 + 0.4 * ((now as f64 * 0.01 + i as f64 * 0.3).sin());
                    let random_factor = 0.2 * ((now as f64 * 0.02 + i as f64 * 0.5).sin());
                    let level = (base_level + random_factor).max(0.0).min(1.0);
                    
                    // Smooth transition from current level
                    let current = self.vu_meter_bars[i];
                    self.vu_meter_bars[i] = current * 0.7 + level * 0.3;
                }
            } else {
                // Fade out when not playing
                for i in 0..20 {
                    self.vu_meter_bars[i] *= 0.8;
                }
            }
        }
    }

    // Draw VU meter background behind progress bar
    fn draw_vu_meter(&self, c: &Context, x: f64, y: f64, width: f64, height: f64, anim_progress: f64) {
        c.save().unwrap();
        
        // Create clipping region for VU meter
        c.new_sub_path();
        c.arc(x + width - 5.0, y + 5.0, 5.0, (-90.0f64).to_radians(), (0.0f64).to_radians());
        c.arc(x + width - 5.0, y + height - 5.0, 5.0, (0.0f64).to_radians(), (90.0f64).to_radians());
        c.arc(x + 5.0, y + height - 5.0, 5.0, (90.0f64).to_radians(), (180.0f64).to_radians());
        c.arc(x + 5.0, y + 5.0, 5.0, (180.0f64).to_radians(), (270.0f64).to_radians());
        c.close_path();
        c.clip();
        
        // Draw VU meter bars
        let bar_width = width / 20.0;
        let bar_spacing = 1.0;
        let effective_bar_width = bar_width - bar_spacing;
        
        for i in 0..20 {
            let bar_x = x + i as f64 * bar_width + bar_spacing / 2.0;
            let bar_height = height * self.vu_meter_bars[i];
            let bar_y = y + height - bar_height;
            
            // Create gradient for each bar
            let gradient = cairo::LinearGradient::new(bar_x, bar_y, bar_x, bar_y + bar_height);
            
            // Color based on level (green -> yellow -> red)
            let level = self.vu_meter_bars[i];
            if level < 0.5 {
                // Green for low levels
                let intensity = (level * 2.0).min(1.0);
                gradient.add_color_stop_rgba(0.0, 0.0, 0.6 * intensity, 0.0, anim_progress * 0.3);
                gradient.add_color_stop_rgba(1.0, 0.0, 0.4 * intensity, 0.0, anim_progress * 0.2);
            } else if level < 0.8 {
                // Yellow for medium levels
                let intensity = ((level - 0.5) * 3.33).min(1.0);
                gradient.add_color_stop_rgba(0.0, 0.6 * intensity, 0.6 * intensity, 0.0, anim_progress * 0.4);
                gradient.add_color_stop_rgba(1.0, 0.4 * intensity, 0.4 * intensity, 0.0, anim_progress * 0.3);
            } else {
                // Red for high levels
                let intensity = ((level - 0.8) * 5.0).min(1.0);
                gradient.add_color_stop_rgba(0.0, 0.6 * intensity, 0.0, 0.0, anim_progress * 0.5);
                gradient.add_color_stop_rgba(1.0, 0.4 * intensity, 0.0, 0.0, anim_progress * 0.4);
            }
            
            c.set_source(&gradient);
            c.rectangle(bar_x, bar_y, effective_bar_width, bar_height);
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
        // Draw background pill with modern gradient
        let pill_x = x;
        let pill_y = y - radius;
        let pill_w = width;
        let pill_h = height + radius * 2.0;
        
        c.save().unwrap();
        
        // Modern gradient background
        let gradient = cairo::LinearGradient::new(pill_x, pill_y, pill_x, pill_y + pill_h);
        gradient.add_color_stop_rgba(0.0, 0.15, 0.15, 0.15, anim_progress);
        gradient.add_color_stop_rgba(1.0, 0.08, 0.08, 0.08, anim_progress);
        c.set_source(&gradient);
        
        // Draw rounded rectangle (pill)
        c.new_sub_path();
        c.arc(pill_x + pill_w - radius, pill_y + radius, radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
        c.arc(pill_x + pill_w - radius, pill_y + pill_h - radius, radius, (0.0f64).to_radians(), (90.0f64).to_radians());
        c.arc(pill_x + radius, pill_y + pill_h - radius, radius, (90.0f64).to_radians(), (180.0f64).to_radians());
        c.arc(pill_x + radius, pill_y + radius, radius, (180.0f64).to_radians(), (270.0f64).to_radians());
        c.close_path();
        c.fill().unwrap();
        
        // Add subtle border
        c.set_line_width(1.0);
        c.set_source_rgba(0.4, 0.4, 0.4, anim_progress);
        c.stroke().unwrap();
        c.restore().unwrap();

        if let Some(status) = &self.last_status {
            // macOS Touch Bar style layout: [icon] [current_time] [progress_bar] [total_time]
            
            // 1. Play/Pause icon (macOS style, 26px)
            let icon_size = 26.0;
            let icon_x = pill_x + 8.0;
            let icon_y = pill_y + (pill_h - icon_size) / 2.0;
            
            c.save().unwrap();
            
            // macOS-style icon background (subtle, flat design)
            let icon_gradient = cairo::RadialGradient::new(
                icon_x + icon_size / 2.0, icon_y + icon_size / 2.0, 0.0,
                icon_x + icon_size / 2.0, icon_y + icon_size / 2.0, icon_size / 2.0
            );
            icon_gradient.add_color_stop_rgba(0.0, 0.85, 0.85, 0.85, anim_progress);
            icon_gradient.add_color_stop_rgba(1.0, 0.65, 0.65, 0.65, anim_progress);
            c.set_source(&icon_gradient);
            c.arc(icon_x + icon_size / 2.0, icon_y + icon_size / 2.0, icon_size / 2.0, 0.0, 2.0 * std::f64::consts::PI);
            c.fill().unwrap();
            
            // macOS-style border (thin, subtle)
            c.set_line_width(0.8);
            c.set_source_rgba(0.9, 0.9, 0.9, anim_progress * 0.6);
            c.stroke().unwrap();
            
            // Draw macOS-style play/pause icon
            c.set_source_rgba(0.15, 0.15, 0.15, anim_progress);
            if status.is_playing {
                // Draw pause icon (two thin rectangles, macOS style)
                let bar_width = 2.5;
                let bar_height = 12.0;
                let bar_spacing = 3.5;
                let icon_center_x = icon_x + icon_size / 2.0;
                let icon_center_y = icon_y + icon_size / 2.0;
                let left_bar_x = icon_center_x - (bar_width * 2.0 + bar_spacing) / 2.0;
                let left_bar_y = icon_center_y - bar_height / 2.0;
                let right_bar_x = left_bar_x + bar_width + bar_spacing;
                
                // Left bar (simple rectangle)
                c.rectangle(left_bar_x, left_bar_y, bar_width, bar_height);
                c.fill().unwrap();
                
                // Right bar (simple rectangle)
                c.rectangle(right_bar_x, left_bar_y, bar_width, bar_height);
                c.fill().unwrap();
            } else {
                // Draw play icon (macOS-style triangle)
                let icon_center_x = icon_x + icon_size / 2.0;
                let icon_center_y = icon_y + icon_size / 2.0;
                let triangle_size = 9.0;
                
                c.move_to(icon_center_x - triangle_size / 2.0 + 0.5, icon_center_y - triangle_size / 2.0);
                c.line_to(icon_center_x + triangle_size / 2.0 + 0.5, icon_center_y);
                c.line_to(icon_center_x - triangle_size / 2.0 + 0.5, icon_center_y + triangle_size / 2.0);
                c.close_path();
                c.fill().unwrap();
            }
            c.restore().unwrap();

            // 2. Current time (macOS system font style)
            let current_seconds = if status.duration > 0 {
                (status.position * status.duration as f64) as i64
            } else {
                0
            };
            let current_time_str = format!("{}:{:02}", current_seconds / 60, current_seconds % 60);
            
            c.save().unwrap();
            c.set_font_size(20.0); // Increased font size to 20px
            c.select_font_face("SF Pro Display", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
            c.set_source_rgba(0.95, 0.95, 0.95, anim_progress); // Brighter white like macOS
            
            let current_time_ext = c.text_extents(&current_time_str).unwrap();
            let current_time_x = icon_x + icon_size + 12.0;
            let current_time_y = pill_y + (pill_h + current_time_ext.height()) / 2.0;
            c.move_to(current_time_x, current_time_y);
            c.show_text(&current_time_str).unwrap();
            c.restore().unwrap();

            // 3. macOS Touch Bar style progress bar with head (rounded, reduced height)
            let progress_x = current_time_x + current_time_ext.width() + 12.0;
            let progress_y = pill_y + 6.0; // Reduced height progress bar with margin
            let progress_h = pill_h - 12.0; // Reduced height minus margins
            let total_time_width = 60.0; // Increased space for larger font
            let progress_w = pill_w - (progress_x - pill_x) - total_time_width - 8.0;
            
            // macOS-style progress bar background (rounded, very subtle)
            c.save().unwrap();
            c.set_source_rgba(0.2, 0.2, 0.2, anim_progress); // Darker background
            let progress_radius = 5.0; // Fixed 5px rounded corners
            c.new_sub_path();
            c.arc(progress_x + progress_w - progress_radius, progress_y + progress_radius, progress_radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
            c.arc(progress_x + progress_w - progress_radius, progress_y + progress_h - progress_radius, progress_radius, (0.0f64).to_radians(), (90.0f64).to_radians());
            c.arc(progress_x + progress_radius, progress_y + progress_h - progress_radius, progress_radius, (90.0f64).to_radians(), (180.0f64).to_radians());
            c.arc(progress_x + progress_radius, progress_y + progress_radius, progress_radius, (180.0f64).to_radians(), (270.0f64).to_radians());
            c.close_path();
            c.fill().unwrap();
            c.restore().unwrap();


            
            // Progress bar head (white, 6px wide, rounded)
            let head_position = drag_position.unwrap_or(status.position);
            if let Some(drag_pos) = drag_position {
            }
            if head_position > 0.0 {
                c.save().unwrap();
                c.set_source_rgba(1.0, 1.0, 1.0, anim_progress); // White head
                let head_x = progress_x + (progress_w * head_position) - 3.0; // 6px wide head, centered
                let head_y = progress_y - 3.0; // Extend head above and below progress bar
                let head_radius = 3.0; // Rounded corners for head
                c.new_sub_path();
                c.arc(head_x + 6.0 - head_radius, head_y + head_radius, head_radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
                c.arc(head_x + 6.0 - head_radius, head_y + progress_h + 6.0 - head_radius, head_radius, (0.0f64).to_radians(), (90.0f64).to_radians());
                c.arc(head_x + head_radius, head_y + progress_h + 6.0 - head_radius, head_radius, (90.0f64).to_radians(), (180.0f64).to_radians());
                c.arc(head_x + head_radius, head_y + head_radius, head_radius, (180.0f64).to_radians(), (270.0f64).to_radians());
                c.close_path();
                c.fill().unwrap();
                c.restore().unwrap();
            }

            // 4. Total time (macOS system font style)
            let total_seconds = status.duration;
            let total_time_str = format!("{}:{:02}", total_seconds / 60, total_seconds % 60);
            
            c.save().unwrap();
            c.set_font_size(20.0); // Increased font size to 20px to match current time
            c.select_font_face("SF Pro Display", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
            c.set_source_rgba(0.95, 0.95, 0.95, anim_progress); // Brighter white like macOS
            
            let total_time_x = progress_x + progress_w + 8.0;
            let total_time_y = pill_y + (pill_h + current_time_ext.height()) / 2.0;
            c.move_to(total_time_x, total_time_y);
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
        let pill_x = 0.0; // Relative to modules area
        let pill_y = 0.0; // Relative to modules area
        let pill_w = width;
        let pill_h = height;
        
        // Check if touch is within the pill area
        if touch_x < pill_x || touch_x > pill_x + pill_w || touch_y < pill_y || touch_y > pill_y + pill_h {
            return None;
        }
        
        // Check play/pause button
        let icon_size = 26.0; // Changed from 28.0 to 26.0
        let icon_x = pill_x + 8.0; // Changed from 10.0 to 8.0
        let icon_y = pill_y + (pill_h - icon_size) / 2.0;
        let icon_center_x = icon_x + icon_size / 2.0;
        let icon_center_y = icon_y + icon_size / 2.0;
        
        let distance = ((touch_x - icon_center_x).powi(2) + (touch_y - icon_center_y).powi(2)).sqrt();
        if distance <= icon_size / 2.0 {
            return Some(VlcAction::TogglePlayPause);
        }
        
        // Check progress bar (rounded, reduced height style)
        let current_time_x = icon_x + icon_size + 12.0;
        let progress_x = current_time_x + 75.0; // Approximate width for current time with larger font
        let progress_y = pill_y + 6.0; // Reduced height progress bar with margin
        let progress_h = pill_h - 12.0; // Reduced height minus margins
        let total_time_width = 60.0; // Increased space for larger font
        let progress_w = pill_w - (progress_x - pill_x) - total_time_width - 8.0;
        
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