//! Simple generic background service player screen for basic media controls
use cairo::Context;
use crate::helper::MediaStatus;
use rsvg::{CairoRenderer, Loader};
use crate::view::backgroundservices::*;
use std::io::Write;

// UI Constants
pub const ICON_SIZE: f64 = 42.0;
pub const ITEM_WIDTH: f64 = 80.0;
pub const ITEM_BACKGROUND_R: f64 = 103.0 / 255.0; // #676767 R component
pub const ITEM_BACKGROUND_G: f64 = 103.0 / 255.0; // #676767 G component  
pub const ITEM_BACKGROUND_B: f64 = 103.0 / 255.0; // #676767 B component
pub const ACTIVE_ITEM_BACKGROUND_R: f64 = 70.0 / 255.0; // #4682B4 R component
pub const ACTIVE_ITEM_BACKGROUND_G: f64 = 130.0 / 255.0; // #4682B4 G component  
pub const ACTIVE_ITEM_BACKGROUND_B: f64 = 180.0 / 255.0; // #4682B4 B component
pub const DETAILS_BACKGROUND_R: f64 = 103.0 / 255.0; // #676767 R component
pub const DETAILS_BACKGROUND_G: f64 = 103.0 / 255.0; // #676767 G component  
pub const DETAILS_BACKGROUND_B: f64 = 103.0 / 255.0; // #676767 B component


fn get_icon_name_for_mpris(mpris_name: &str) -> Option<&str> {
    match mpris_name {
        "org.mpris.MediaPlayer2.spotify" => Some("/usr/share/tiny-dfr/icons/tiny-dfr-icons/symbolic/media/spotify/media-playback-start-symbolic.svg"),
        mpris if mpris.contains("chromium") => Some("/usr/share/tiny-dfr/icons/tiny-dfr-icons/symbolic/media/chromium.svg"),
        _ => None,
    }
}

fn draw_fallback_text(c: &Context, current_x: f64, items_y: f64, item_height: f64, index: usize, is_expanded: bool) {
    if is_expanded {
        c.set_source_rgba(0.1, 0.1, 0.1, 1.0); // Dark text for expanded
    } else {
        c.set_source_rgba(0.4, 0.4, 0.4, 0.9); // Gray text for collapsed
    }
    c.set_font_size(12.0);
    let title = format!("MPRIS {}", index);
    let title_y = items_y + (item_height + 12.0) / 2.0;
    c.move_to(current_x + 12.0, title_y);
    c.show_text(&title).unwrap();
}

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



#[derive(Debug, Clone)]
pub enum GenericBackgroundAction {
    PlayPause,
    Next,
    Previous,
    ToggleMprisItem(usize),
    CloseGenericMedia,
    // Background service player actions
    BackgroundServicePlayerPlayPause,
    BackgroundServicePlayerNext,
    BackgroundServicePlayerPrevious,
    BackgroundServicePlayerSeek(f64), // 0.0 to 1.0
    BackgroundServicePlayerDragHead(f64), // 0.0 to 1.0 - for dragging the progress bar head
}

pub struct GenericBackgroundScreen {
    pub last_status: Option<MediaStatus>,
    pub expanded_items: Vec<bool>,
    pub background_service_player: BackgroundServicePlayer,
    pub available_mpris_services: Vec<String>,
    pub selected_service_index: Option<usize>,
}

impl GenericBackgroundScreen {
    pub fn new() -> Self {
        Self {
            last_status: None,
            expanded_items: Vec::new(), // Start empty, populated by background service helper
            background_service_player: BackgroundServicePlayer::new(),
            available_mpris_services: Vec::new(), // Start empty, populated by background service helper
            selected_service_index: None, // No service selected initially
        }
    }
    
    pub fn update_available_services(&mut self, services: Vec<String>) {
        println!("[generic_background_screen] Received services: {:?}", services);
        self.available_mpris_services = services;
        // Reset expanded items to match new service count - first item expanded by default
        self.expanded_items = vec![true; self.available_mpris_services.len()];
        if self.expanded_items.len() > 1 {
            self.expanded_items[1..].fill(false); // Only first item expanded
        }
        // Only select first service by default if no service is currently selected
        if self.selected_service_index.is_none() && !self.available_mpris_services.is_empty() {
            self.selected_service_index = Some(0);
            println!("[generic_background_screen] Auto-selected first service (index 0)");
        } else if self.available_mpris_services.is_empty() {
            self.selected_service_index = None;
        }
    }

    pub fn toggle_mpris_item(&mut self, index: usize) {
        println!("[generic_background_screen] ===== TOGGLE MPRIS ITEM =====");
        println!("[generic_background_screen] Toggling item at index: {}", index);
        println!("[generic_background_screen] Current expanded_items: {:?}", self.expanded_items);
        println!("[generic_background_screen] Current selected_service_index: {:?}", self.selected_service_index);
        
        // Don't do anything if no services are available
        if self.available_mpris_services.is_empty() {
            println!("[generic_background_screen] No services available, ignoring toggle");
            return;
        }
        
        if index < self.expanded_items.len() {
            // If the clicked item is currently expanded, close it only if it's not the last item
            if self.expanded_items[index] {
                // Count how many items are currently expanded
                let expanded_count = self.expanded_items.iter().filter(|&&expanded| expanded).count();
                println!("[generic_background_screen] Item is currently expanded, expanded_count: {}", expanded_count);
                
                // Only close if there's more than one item expanded
                if expanded_count > 1 {
                    self.expanded_items[index] = false;
                    println!("[generic_background_screen] Closed item at index {}", index);
                } else {
                    println!("[generic_background_screen] Keeping item open (only one expanded)");
                }
            } else {
                println!("[generic_background_screen] Item is currently collapsed, opening it");
                // Close all other items first (accordion behavior)
                for i in 0..self.expanded_items.len() {
                    if i != index {
                        self.expanded_items[i] = false;
                    }
                }
                // Then open the clicked item
                self.expanded_items[index] = true;
                println!("[generic_background_screen] Opened item at index {}", index);
            }
            
            // Update selected service when toggling
            self.selected_service_index = Some(index);
            println!("[generic_background_screen] Updated selected_service_index to: {:?}", self.selected_service_index);
        } else {
            println!("[generic_background_screen] Index {} out of bounds (len: {})", index, self.expanded_items.len());
        }
        println!("[generic_background_screen] ==============================");
    }
    
    // Method to send selection command to background service helper
    pub fn send_selection_command(&self, background_service_helper_stream: &mut Option<std::os::unix::net::UnixStream>) {
        println!("[generic_background_screen] ===== SENDING SELECTION COMMAND =====");
        println!("[generic_background_screen] Selected service index: {:?}", self.selected_service_index);
        println!("[generic_background_screen] Available services: {:?}", self.available_mpris_services);
        
        // Don't send anything if no services are available
        if self.available_mpris_services.is_empty() {
            println!("[generic_background_screen] No services available, not sending selection command");
            return;
        }
        
        if let Some(stream) = background_service_helper_stream {
            if let Some(selected_index) = self.selected_service_index {
                if let Some(service_name) = self.available_mpris_services.get(selected_index) {
                    let command = format!("select_service:{}\n", service_name);
                    println!("[generic_background_screen] Command to send: '{}'", command.trim());
                    println!("[generic_background_screen] Command bytes: {:?}", command.as_bytes());
                    
                    if let Err(e) = stream.write_all(command.as_bytes()) {
                        eprintln!("[generic_background_screen] Failed to send selection command: {}", e);
                    } else {
                        println!("[generic_background_screen] Successfully sent selection command: {}", command.trim());
                    }
                } else {
                    println!("[generic_background_screen] No service found at index {}", selected_index);
                }
            } else {
                println!("[generic_background_screen] No service selected (selected_index is None)");
            }
        } else {
            println!("[generic_background_screen] No background service helper stream available");
        }
        println!("[generic_background_screen] ======================================");
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
        _drag_position: Option<f64>,
    ) {
        // Calculate layout dimensions (same as other screens)
        let pill_x = x;
        let pill_y = y - radius;
        let pill_w = width;
        let pill_h = height + radius * 2.0;
        
        c.save().unwrap();
        
        // Draw background using proper pill layout
        // c.set_source_rgba(0.1, 0.1, 0.1, anim_progress);
        c.new_sub_path();
        c.arc(pill_x + pill_w - radius, pill_y + radius, radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
        c.arc(pill_x + pill_w - radius, pill_y + pill_h - radius, radius, (0.0f64).to_radians(), (90.0f64).to_radians());
        c.arc(pill_x + radius, pill_y + pill_h - radius, radius, (90.0f64).to_radians(), (180.0f64).to_radians());
        c.arc(pill_x + radius, pill_y + radius, radius, (180.0f64).to_radians(), (270.0f64).to_radians());
        c.close_path();
        c.fill().unwrap();
        
        // Draw close button with close-symbolic icon
        self.draw_close_button(c, pill_x + 20.0, pill_y, pill_h);
        
        // Draw MPRIS background items using full pill height
        self.draw_mpris_items(c, pill_x, pill_y, pill_w, pill_h, _drag_position);
        
        c.restore().unwrap();
    }
    
    fn draw_no_services_message(&self, c: &Context, x: f64, y: f64, width: f64, height: f64) {
        // Start from left after the close button to avoid overlap
        let start_x = x + 100.0; // Start after close button (approximately 100px)
        let message_width = width - (start_x - x);
        let message_height = height;
        
        c.save().unwrap();
        
     
     
        
        // Draw "No services available" text
        c.set_source_rgba(0.6, 0.6, 0.6, 0.8);
        c.set_font_size(16.0);
        let text = "No supported services available";
        let text_extents = c.text_extents(text).unwrap();
        let text_width = text_extents.width();
        let text_height = text_extents.height();
        let text_x = start_x + (message_width - text_width) / 2.0;
        let text_y = y + (message_height + text_height) / 2.0;
        c.move_to(text_x, text_y);
        c.show_text(text).unwrap();
        
        c.restore().unwrap();
    }
    
    fn draw_mpris_items(&mut self, c: &Context, x: f64, y: f64, width: f64, height: f64, drag_position: Option<f64>) {
        // Show "No services available" message if no services are available
        if self.available_mpris_services.is_empty() {
            self.draw_no_services_message(c, x, y, width, height);
            return;
        }
        
        // Use full available height for items
        let item_height = height; // Use 100% of available height
        let item_width = ITEM_WIDTH;
        let item_spacing = 15.0;
        let detail_spacing = 10.0; // Spacing between item and details
        
        // Start from left after the close button to avoid overlap
        let start_x = x + 100.0; // Start after close button (approximately 100px)
        let items_y = y;
        
        // Calculate how many items are expanded to determine remaining width
        let expanded_count = self.expanded_items.iter().filter(|&&expanded| expanded).count();
        let collapsed_count = self.expanded_items.len() - expanded_count;
        
        // Calculate total width used by collapsed items
        let total_collapsed_width = collapsed_count as f64 * (item_width + item_spacing);
        let total_expanded_width = expanded_count as f64 * item_width; // No detail_spacing since there's no gap
        
        // Calculate remaining width for expanded details
        let available_width = width - (start_x - x) - total_collapsed_width - total_expanded_width;
        let detail_width = if expanded_count > 0 { available_width / expanded_count as f64 } else { 0.0 };
        
        // Ensure the last expanded item uses all remaining space to avoid empty space
        let mut remaining_width = available_width;
        
        // Calculate dynamic positions based on expanded items
        let mut current_x = start_x;
        
        // Collect mpris names to avoid borrowing issues
        let mpris_names: Vec<String> = self.available_mpris_services.clone();
        
        for (index, mpris_name) in mpris_names.iter().enumerate() {
            let is_expanded = self.expanded_items[index];
            
            // Draw minimal modern collapsed/expanded item background with rounded corners
            c.save().unwrap();
            
            let radius = 8.0; // Rounded corner radius
            
            if is_expanded {
                // Expanded state - darker background for active item
                c.set_source_rgba(ACTIVE_ITEM_BACKGROUND_R, ACTIVE_ITEM_BACKGROUND_G, ACTIVE_ITEM_BACKGROUND_B, 1.0);
            } else {
                // Collapsed state - normal background
                c.set_source_rgba(ITEM_BACKGROUND_R, ITEM_BACKGROUND_G, ITEM_BACKGROUND_B, 0.8);
            }
            
            // Draw rounded rectangle for item (no right rounding if expanded)
            c.new_path();
            // Top-left corner (always rounded)
            c.arc(current_x + radius, items_y + radius, radius, std::f64::consts::PI, 1.5 * std::f64::consts::PI);
            if is_expanded {
                // No right rounding when expanded - straight edges
                c.line_to(current_x + item_width, items_y); // Top edge (straight)
                c.line_to(current_x + item_width, items_y + item_height); // Right edge (straight)
                c.line_to(current_x + radius, items_y + item_height); // Bottom edge (straight)
            } else {
                // Normal right rounding when collapsed
                c.arc(current_x + item_width - radius, items_y + radius, radius, 1.5 * std::f64::consts::PI, 2.0 * std::f64::consts::PI); // Top-right
                c.arc(current_x + item_width - radius, items_y + item_height - radius, radius, 0.0, 0.5 * std::f64::consts::PI); // Bottom-right
                c.line_to(current_x + radius, items_y + item_height); // Bottom edge
            }
            // Bottom-left corner (always rounded)
            c.arc(current_x + radius, items_y + item_height - radius, radius, 0.5 * std::f64::consts::PI, std::f64::consts::PI);
            c.close_path();
            c.fill().unwrap();
            
            // Draw minimal border with rounded corners (no right rounding if expanded)
            if is_expanded {
                c.set_source_rgba(0.2, 0.2, 0.2, 0.3); // Dark border for expanded
                c.set_line_width(1.0);
            } else {
                c.set_source_rgba(0.7, 0.7, 0.7, 0.5); // Light border for collapsed
                c.set_line_width(0.5);
            }
            c.new_path();
            // Top-left corner (always rounded)
            c.arc(current_x + radius, items_y + radius, radius, std::f64::consts::PI, 1.5 * std::f64::consts::PI);
            if is_expanded {
                // No right rounding when expanded - straight edges
                c.line_to(current_x + item_width, items_y); // Top edge (straight)
                c.line_to(current_x + item_width, items_y + item_height); // Right edge (straight)
                c.line_to(current_x + radius, items_y + item_height); // Bottom edge (straight)
            } else {
                // Normal right rounding when collapsed
                c.arc(current_x + item_width - radius, items_y + radius, radius, 1.5 * std::f64::consts::PI, 2.0 * std::f64::consts::PI); // Top-right
                c.arc(current_x + item_width - radius, items_y + item_height - radius, radius, 0.0, 0.5 * std::f64::consts::PI); // Bottom-right
                c.line_to(current_x + radius, items_y + item_height); // Bottom edge
            }
            // Bottom-left corner (always rounded)
            c.arc(current_x + radius, items_y + item_height - radius, radius, 0.5 * std::f64::consts::PI, std::f64::consts::PI);
            c.close_path();
            c.stroke().unwrap();
            
            // Draw icon for the MPRIS item - center vertically in the item
            if let Some(icon_path) = get_icon_name_for_mpris(mpris_name) {
                let icon_size = ICON_SIZE;
                let icon_x = current_x + (item_width - icon_size) / 2.0; // Center horizontally
                let icon_y = items_y + (item_height - icon_size) / 2.0; // Center vertically
                
                // Load and draw the icon using SVG rendering
                match render_svg_icon_from_path(c, icon_path, icon_x, icon_y, icon_size) {
                    Ok(_) => {
                        // Icon rendered successfully
                    }
                    Err(_) => {
                        // Fallback to text if icon loading fails
                        draw_fallback_text(c, current_x, items_y, item_height, index, is_expanded);
                    }
                }
            } else {
                // Fallback to text if no icon is mapped
                draw_fallback_text(c, current_x, items_y, item_height, index, is_expanded);
            }
            
            // Draw minimal expand/collapse indicator
            let title_y = items_y + (item_height + 12.0) / 2.0; // Calculate title_y for indicator
            // if is_expanded {
            //     c.set_source_rgba(0.1, 0.1, 0.1, 1.0); // Dark
            //     let indicator = "−"; // Minus for expanded
            //     c.move_to(current_x + item_width - 20.0, title_y);
            //     c.show_text(indicator).unwrap();
            // } else {
            //     c.set_source_rgba(0.4, 0.4, 0.4, 0.8); // Gray
            //     let indicator = "+"; // Plus for collapsed
            //     c.move_to(current_x + item_width - 20.0, title_y);
            //     c.show_text(indicator).unwrap();
            // }
            
            c.restore().unwrap();
            
            // If expanded, show MPRIS DBus info to the right of the item (horizontal expansion)
            if is_expanded {
                // Use remaining width for the last expanded item to fill the space
                let actual_detail_width = if remaining_width > detail_width { remaining_width } else { detail_width };
                self.draw_mpris_details(c, current_x + item_width, items_y, actual_detail_width, item_height, mpris_name.as_str(), drag_position);
                // Move current_x to account for the expanded details (no gap)
                current_x += item_width + actual_detail_width + item_spacing;
                remaining_width -= actual_detail_width;
            } else {
                // Move current_x to the next item position
                current_x += item_width + item_spacing;
            }
        }
    }
    
    fn draw_mpris_details(&mut self, c: &Context, x: f64, y: f64, width: f64, height: f64, mpris_name: &str, drag_position: Option<f64>) {
        // Check if this is a background service player and use special UI
        if mpris_name.contains("spotify") {
            self.draw_background_service_player_details(c, x, y, width, height, mpris_name, drag_position);
            return;
        }
        
        // Default UI for non-background service MPRIS players
        let padding = 15.0;
        let radius = 8.0; // Same radius as items
        
        c.save().unwrap();
        
        // Draw minimal modern details background with rounded corners
        c.set_source_rgba(DETAILS_BACKGROUND_R, DETAILS_BACKGROUND_G, DETAILS_BACKGROUND_B, 0.9);
        
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
        
        // Draw minimal border with rounded corners (no left rounding - connected to item)
        c.set_source_rgba(0.2, 0.2, 0.2, 0.2); // Very subtle border
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
        
   
        c.set_source_rgba(1.0, 1.0, 1.0, 1.0); // Dark text value
        c.set_font_size(14.0);
        let dbus_value_y = y + (height - 14.0) / 2.0 + 6.0;
        c.move_to(x + padding, dbus_value_y);
        c.show_text(mpris_name).unwrap();
        
        c.restore().unwrap();
    }
    
    fn draw_background_service_player_details(&mut self, c: &Context, x: f64, y: f64, width: f64, height: f64, mpris_name: &str, drag_position: Option<f64>) {
        // Update the background service player with current status
        self.background_service_player.last_status = self.last_status.clone();
        
        // Use the background service player to draw the details
        self.background_service_player.draw_details(c, x, y, width, height, mpris_name, drag_position);
    }
    
    fn draw_close_button(&self, c: &Context, x: f64, y: f64, height: f64) {
        let icon_size = 42.0; // Fixed 42px icon size
        let icon_padding = (height - icon_size) / 2.0;
        let icon_x = x;
        let icon_y = y + icon_padding;
        
        c.save().unwrap();
        
        // Try to load the close-symbolic icon
        let icon_result = crate::utils::button_images::load_image("close-symbolic", None, "use_default", "tiny-dfr-icons");
        
        if let Ok(icon_image) = icon_result {
            match icon_image {
                crate::utils::button_images::ButtonImage::Svg(svg_handle) => {
                    let renderer = rsvg::CairoRenderer::new(&svg_handle);
                    let rect = cairo::Rectangle::new(icon_x, icon_y, icon_size, icon_size);
                    let _ = renderer.render_document(c, &rect);
                },
                crate::utils::button_images::ButtonImage::Bitmap(surface) => {
                    c.set_source_surface(&surface, icon_x, icon_y).unwrap();
                    c.paint().unwrap();
                },
                _ => {
                    // Fallback: draw a simple X
                    c.set_source_rgb(1.0, 1.0, 1.0);
                    c.set_line_width(2.0);
                    let center_x = icon_x + icon_size / 2.0;
                    let center_y = icon_y + icon_size / 2.0;
                    let cross_size = icon_size * 0.4;
                    c.move_to(center_x - cross_size, center_y - cross_size);
                    c.line_to(center_x + cross_size, center_y + cross_size);
                    c.move_to(center_x + cross_size, center_y - cross_size);
                    c.line_to(center_x - cross_size, center_y + cross_size);
                    c.stroke().unwrap();
                }
            }
        } else {
            // Fallback: draw a simple X
            c.set_source_rgb(1.0, 1.0, 1.0);
            c.set_line_width(2.0);
            let center_x = icon_x + icon_size / 2.0;
            let center_y = icon_y + icon_size / 2.0;
            let cross_size = icon_size * 0.4;
            c.move_to(center_x - cross_size, center_y - cross_size);
            c.line_to(center_x + cross_size, center_y + cross_size);
            c.move_to(center_x + cross_size, center_y - cross_size);
            c.line_to(center_x - cross_size, center_y + cross_size);
            c.stroke().unwrap();
        }
        
        c.restore().unwrap();
    }
    
    pub fn hit_test(
        &mut self,
        touch_x: f64,
        touch_y: f64,
        screen_x: f64,
        screen_y: f64,
        screen_width: f64,
        screen_height: f64,
        radius: f64,
    ) -> Option<GenericBackgroundAction> {
        // Check if touch is within screen bounds
        if touch_x < screen_x || touch_x > screen_x + screen_width ||
           touch_y < screen_y || touch_y > screen_y + screen_height {
            return None;
        }
        
        // Check MPRIS items using proper pill layout (same as draw function)
        let pill_x = screen_x;
        let pill_y = screen_y - radius;
        let pill_w = screen_width;
        let pill_h = screen_height + radius * 2.0;
        
        // Check close button first (same positioning as draw_close_button)
        let icon_size = 42.0; // Fixed 42px icon size
        let icon_padding = (pill_h - icon_size) / 2.0;
        let icon_x = pill_x + 20.0;
        let icon_y = pill_y + icon_padding;
        
        if touch_x >= icon_x && touch_x <= icon_x + icon_size &&
           touch_y >= icon_y && touch_y <= icon_y + icon_size {
            return Some(GenericBackgroundAction::CloseGenericMedia);
        }
        
        // Don't test anything if no services are available
        if self.available_mpris_services.is_empty() {
            return None;
        }
        
        // Use same calculations as draw_mpris_items
        let item_height = pill_h; // Use 100% of available height
        let item_width = ITEM_WIDTH;
        let item_spacing = 15.0;
        let detail_spacing = 10.0; // Spacing between item and details
        
        // Start from left after the close button to avoid overlap (same as draw function)
        let start_x = pill_x + 100.0; // Start after close button (approximately 100px)
        let items_y = pill_y;
        
        // Calculate how many items are expanded to determine remaining width (same as draw function)
        let expanded_count = self.expanded_items.iter().filter(|&&expanded| expanded).count();
        let collapsed_count = self.expanded_items.len() - expanded_count;
        
        // Calculate total width used by collapsed items
        let total_collapsed_width = collapsed_count as f64 * (item_width + item_spacing);
        let total_expanded_width = expanded_count as f64 * item_width; // No gap between item and details
        
        // Calculate remaining width for expanded details
        let available_width = pill_w - (start_x - pill_x) - total_collapsed_width - total_expanded_width;
        let detail_width = if expanded_count > 0 { available_width / expanded_count as f64 } else { 0.0 };
        
        // Ensure the last expanded item uses all remaining space to avoid empty space
        let mut remaining_width = available_width;
        
        // Calculate dynamic positions based on expanded items (same as draw function)
        let mut current_x = start_x;
        
        // Collect mpris names to avoid borrowing issues
        let mpris_names: Vec<String> = self.available_mpris_services.clone();
        
        for (index, _) in mpris_names.iter().enumerate() {
            let is_expanded = self.expanded_items[index];
            
            // Check if touch is within this MPRIS item
            if touch_x >= current_x && touch_x <= current_x + item_width &&
               touch_y >= items_y && touch_y <= items_y + item_height {
                return Some(GenericBackgroundAction::ToggleMprisItem(index));
            }
            
            // If expanded, also check the details area to the right of the item (no gap)
            if is_expanded {
                let detail_x = current_x + item_width; // No gap between item and details
                // Use remaining width for the last expanded item to fill the space
                let actual_detail_width = if remaining_width > detail_width { remaining_width } else { detail_width };
                if touch_x >= detail_x && touch_x <= detail_x + actual_detail_width &&
                   touch_y >= items_y && touch_y <= items_y + item_height {
                    // Check if this is a background service player and handle specific controls
                    if let Some(mpris_name) = mpris_names.get(index) {
                        if mpris_name.contains("spotify") {
                            if let Some(action) = self.hit_test_background_service_player_controls(touch_x, touch_y, detail_x, items_y, actual_detail_width, item_height) {
                                return Some(action);
                            }
                        }
                    }
                    return Some(GenericBackgroundAction::ToggleMprisItem(index));
                }
                // Move current_x to account for the expanded details (no gap)
                current_x += item_width + actual_detail_width + item_spacing;
                remaining_width -= actual_detail_width;
            } else {
                // Move current_x to the next item position
                current_x += item_width + item_spacing;
            }
        }
        
        None
    }
    
    fn hit_test_background_service_player_controls(&mut self, touch_x: f64, touch_y: f64, x: f64, y: f64, width: f64, height: f64) -> Option<GenericBackgroundAction> {
        // Update the background service player with current status
        self.background_service_player.last_status = self.last_status.clone();
        
        // Use the background service player to handle hit testing
        if let Some(action) = self.background_service_player.hit_test_controls(touch_x, touch_y, x, y, width, height) {
            // Convert BackgroundServicePlayerAction to GenericBackgroundAction
            match action {
                BackgroundServicePlayerAction::PlayPause => Some(GenericBackgroundAction::BackgroundServicePlayerPlayPause),
                BackgroundServicePlayerAction::Next => Some(GenericBackgroundAction::BackgroundServicePlayerNext),
                BackgroundServicePlayerAction::Previous => Some(GenericBackgroundAction::BackgroundServicePlayerPrevious),
                BackgroundServicePlayerAction::Seek(ratio) => Some(GenericBackgroundAction::BackgroundServicePlayerSeek(ratio)),
                BackgroundServicePlayerAction::DragHead(ratio) => Some(GenericBackgroundAction::BackgroundServicePlayerDragHead(ratio)),
            }
        } else {
            None
        }
    }
}
