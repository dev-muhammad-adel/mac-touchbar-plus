use cairo::Context;
use input_linux::Key;
use crate::toggle_key;
use drm::control::ClipRect;
use crate::helper::BrowserStatus;

fn extract_domain(url: &str) -> Option<String> {
    // Simple domain extraction - remove protocol and path
    if let Some(domain) = url.strip_prefix("http://") {
        domain.split('/').next().map(|s| s.to_string())
    } else if let Some(domain) = url.strip_prefix("https://") {
        domain.split('/').next().map(|s| s.to_string())
    } else {
        url.split('/').next().map(|s| s.to_string())
    }
}

pub struct BrowserScreen {
    pub buttons: Vec<Button>,
    pub last_status: Option<BrowserStatus>,
    pub address_bar_focused: bool,
}

impl BrowserScreen {
    pub fn new() -> Self {
        // Fractions: Back/Forward/Refresh/Close Tab/New Tab = 0.6, Address Bar = 3.0
        let buttons = vec![
            Button::new_icon_with_fraction("go-previous-symbolic", input_linux::Key::Unknown, false, 0.6), // Back
            Button::new_icon_with_fraction("go-next-symbolic", input_linux::Key::Unknown, false, 0.6),     // Forward
            Button::new_icon_with_fraction("view-refresh-symbolic", input_linux::Key::Unknown, false, 0.6), // Refresh
            Button::new_icon_with_fraction("emblem-web-symbolic", input_linux::Key::Unknown, false, 3.0), // Address Bar (display only)
            // Button::new_icon_with_fraction("close-symbolic", input_linux::Key::Unknown, false, 0.6), // Close Tab
            Button::new_icon_with_fraction("tab-new-symbolic", input_linux::Key::Unknown, false, 0.6),      // New Tab
        ];
        Self { 
            buttons,
            last_status: None,
            address_bar_focused: false,
        }
    }

    pub fn reset_button_states(&mut self) {
        for button in &mut self.buttons {
            button.active = false;
            button.changed = true;
        }
    }

    pub fn update_status(&mut self, status: BrowserStatus) {
        self.last_status = Some(status);
        // Mark address bar button as changed to update the display
        if let Some(button) = self.buttons.get_mut(3) {
            button.changed = true;
        }
    }

    pub fn focus_address_bar(&mut self) {
        self.address_bar_focused = true;
        // Mark address bar button as changed to update the display
        if let Some(button) = self.buttons.get_mut(3) {
            button.changed = true;
        }
    }

    pub fn unfocus_address_bar(&mut self) {
        self.address_bar_focused = false;
        // Mark address bar button as changed to update the display
        if let Some(button) = self.buttons.get_mut(3) {
            button.changed = true;
        }
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
        complete_redraw: bool,
        modified_regions: &mut Vec<ClipRect>,
    ) {
        println!("[browser_screen] Drawing with {} active buttons, complete_redraw={}", self.buttons.iter().filter(|b| b.active).count(), complete_redraw);
        println!("[browser_screen] Button states: Back(active={}, changed={}), Forward(active={}, changed={}), Refresh(active={}, changed={}), Home(active={}, changed={})", 
            self.buttons[0].active, self.buttons[0].changed,
            self.buttons[1].active, self.buttons[1].changed,
            self.buttons[2].active, self.buttons[2].changed,
            self.buttons[3].active, self.buttons[3].changed);
        // Calculate layout dimensions
        let pill_x = x;
        let pill_y = y - radius;
        let pill_w = width;
        let pill_h = height + radius * 2.0;

        // Background
        c.save().unwrap();
        c.set_source_rgba(0.0, 0.0, 0.0, anim_progress);
        
        // Draw rounded rectangle (pill)
        c.new_sub_path();
        c.arc(pill_x + pill_w - radius, pill_y + radius, radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
        c.arc(pill_x + pill_w - radius, pill_y + pill_h - radius, radius, (0.0f64).to_radians(), (90.0f64).to_radians());
        c.arc(pill_x + radius, pill_y + pill_h - radius, radius, (90.0f64).to_radians(), (180.0f64).to_radians());
        c.arc(pill_x + radius, pill_y + radius, radius, (180.0f64).to_radians(), (270.0f64).to_radians());
        c.close_path();
        c.fill().unwrap();
        c.restore().unwrap();

        // Calculate button layout with fractions
        let button_count = self.buttons.len();
        let button_spacing = 8.0;
        let total_spacing = if button_count > 1 { button_spacing * (button_count as f64 - 1.0) } else { 0.0 };
        let button_area = pill_w - total_spacing;
        let weights: Vec<f32> = self.buttons.iter().map(|b| b.fraction).collect();
        let total_weight: f32 = weights.iter().sum();
        let mut button_widths: Vec<f64> = weights.iter().map(|w| button_area * (*w as f64 / total_weight as f64)).collect();
        // Last button absorbs rounding error
        let sum_widths: f64 = button_widths.iter().sum();
        if let Some(last) = button_widths.last_mut() {
            *last += button_area - sum_widths;
        }
        let button_height = pill_h;
        let button_y = pill_y;

        // Draw buttons
        let mut button_x = pill_x;
        for (i, button) in self.buttons.iter_mut().enumerate() {
            let this_button_width = button_widths[i];
            
            // Clear button area if not complete redraw
            if !complete_redraw {
                c.set_source_rgb(0.0, 0.0, 0.0);
                c.rectangle(button_x, button_y, this_button_width, button_height);
                c.fill().unwrap();
            }
            
            // Draw button background
            let color = if button.active {
                0.600 // BUTTON_COLOR_ACTIVE
            } else {
                0.350 // BUTTON_COLOR_INACTIVE
            };
            
            c.save().unwrap();
            c.set_source_rgba(color, color, color, anim_progress);
            
            // Draw rounded rectangle for button
            let button_radius = button_height * 0.3;
            c.new_sub_path();
            c.move_to(button_x + button_radius, button_y);
            c.line_to(button_x + this_button_width - button_radius, button_y);
            c.curve_to(button_x + this_button_width, button_y, button_x + this_button_width, button_y, button_x + this_button_width, button_y + button_radius);
            c.line_to(button_x + this_button_width, button_y + button_height - button_radius);
            c.curve_to(button_x + this_button_width, button_y + button_height, button_x + this_button_width, button_y + button_height, button_x + this_button_width - button_radius, button_y + button_height);
            c.line_to(button_x + button_radius, button_y + button_height);
            c.curve_to(button_x, button_y + button_height, button_x, button_y + button_height, button_x, button_y + button_height - button_radius);
            c.line_to(button_x, button_y + button_radius);
            c.curve_to(button_x, button_y, button_x, button_y, button_x + button_radius, button_y);
            c.close_path();
            c.fill().unwrap();
            c.restore().unwrap();

            // Draw SVG icon (and text for Address Bar)
            c.save().unwrap();
            let icon_path = format!("/usr/share/tiny-dfr/icons/tiny-dfr-icons/browser/{}.svg", button.text);
            let is_address_bar = i == 3; // Address Bar is the 4th button
            let icon_size = (this_button_width.min(button_height) * 0.6).min(48.0);
            let icon_x = button_x + (this_button_width - icon_size) / 2.0;
            let icon_y = button_y + (button_height - icon_size) / 2.0;
            
            // Debug which button we're processing
            println!("[browser_screen] Processing button {}: '{}' with icon path: {}", i, button.text, icon_path);
            
            if let Ok(handle) = rsvg::Loader::new().read_path(&icon_path) {
                println!("[browser_screen] Successfully loaded icon: {}", icon_path);
                let renderer = rsvg::CairoRenderer::new(&handle);
                if is_address_bar {
                    // Calculate total width of icon + text + spacing
                    let display_text = "Search or enter website name".to_string();
                    c.set_font_size(24.0);
                    c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
                    let text_ext = c.text_extents(&display_text).unwrap();
                    
                    let total_width = icon_size + 8.0 + text_ext.width(); // icon + spacing + text
                    let start_x = button_x + (this_button_width - total_width) / 2.0; // Center the whole group
                    
                    // Draw icon centered with text
                    let icon_x = start_x;
                    let icon_y = button_y + (button_height - icon_size) / 2.0;
                    renderer.render_document(c, &cairo::Rectangle::new(icon_x, icon_y, icon_size, icon_size)).unwrap();
                    
                    // Always use white color
                    c.set_source_rgba(1.0, 1.0, 1.0, anim_progress);
                    
                    // Draw text next to icon
                    let text_x = icon_x + icon_size + 8.0;
                    let text_y = button_y + (button_height + text_ext.height()) / 2.0;
                    c.move_to(text_x, text_y);
                    c.show_text(&display_text).unwrap();
                } else {
                    renderer.render_document(c, &cairo::Rectangle::new(icon_x, icon_y, icon_size, icon_size)).unwrap();
                }
            } else {
                println!("[browser_screen] Failed to load icon: {} (button {}: {})", icon_path, i, button.text);
                // fallback: draw icon name as text
                c.set_font_size(14.0);
                c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
                c.set_source_rgba(1.0, 1.0, 1.0, anim_progress);
                let ext = c.text_extents(&button.text).unwrap();
                let text_x = button_x + (this_button_width - ext.width()) / 2.0;
                let text_y = button_y + (button_height + ext.height()) / 2.0;
                c.move_to(text_x, text_y);
                c.show_text(&button.text).unwrap();
            }
            c.restore().unwrap();

            // Add clip rectangle for partial redraw
            if !complete_redraw && button.changed {
                // Only add clip rectangle if button actually changed and we're not doing complete redraw
                let clip_y1 = button_y as u16;
                let clip_x1 = button_x as u16;
                let clip_y2 = (button_y + button_height) as u16;
                let clip_x2 = (button_x + this_button_width) as u16;
                
                // Ensure valid coordinates
                if clip_y2 > clip_y1 && clip_x2 > clip_x1 {
                    modified_regions.push(ClipRect::new(
                        clip_y1,
                        clip_x1,
                        clip_y2,
                        clip_x2
                    ));
                }
            }

            button.changed = false;
            button_x += this_button_width + button_spacing;
        }
    }

    pub fn hit_test(
        &mut self,
        touch_x: f64,
        touch_y: f64,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f64,
    ) -> Option<BrowserAction> {
        println!("[browser_screen] hit_test called with touch_x={}, touch_y={}, x={}, y={}, width={}, height={}", touch_x, touch_y, x, y, width, height);
        
        // Calculate layout dimensions (same as draw function)
        let pill_x = x;
        let pill_y = y - radius;
        let pill_w = width;
        let pill_h = height + radius * 2.0;

        // Check if touch is within the pill area
        if touch_x < pill_x || touch_x > pill_x + pill_w || touch_y < pill_y || touch_y > pill_y + pill_h {
            println!("[browser_screen] Touch outside pill area: pill_x={}, pill_y={}, pill_w={}, pill_h={}", pill_x, pill_y, pill_w, pill_h);
            return None;
        }
        println!("[browser_screen] Touch inside pill area, checking buttons");

        // Calculate button layout (same as draw function) - FULL HEIGHT
        let button_count = self.buttons.len();
        let button_spacing = 8.0;
        let total_spacing = if button_count > 1 { button_spacing * (button_count as f64 - 1.0) } else { 0.0 };
        let button_area = pill_w - total_spacing;
        let weights: Vec<f32> = self.buttons.iter().map(|b| b.fraction).collect();
        let total_weight: f32 = weights.iter().sum();
        let mut button_widths: Vec<f64> = weights.iter().map(|w| button_area * (*w as f64 / total_weight as f64)).collect();
        let sum_widths: f64 = button_widths.iter().sum();
        if let Some(last) = button_widths.last_mut() {
            *last += button_area - sum_widths;
        }
        let button_height = pill_h; // FULL HEIGHT
        let button_y = pill_y; // Start from top of pill

        // Check each button
        let mut button_x = pill_x;
        for (i, _button) in self.buttons.iter().enumerate() {
            println!("[browser_screen] Checking button {}: x={}, y={}, w={}, h={}", i, button_x, button_y, button_widths[i], button_height);
            if touch_x >= button_x && touch_x <= button_x + button_widths[i] &&
               touch_y >= button_y && touch_y <= button_y + button_height {
                println!("[browser_screen] Button {} hit!", i);
                return match i {
                    0 => Some(BrowserAction::Back),
                    1 => Some(BrowserAction::Forward),
                    2 => Some(BrowserAction::Refresh),
                    3 => Some(BrowserAction::AddressBar), // Address Bar is interactive
                    4 => Some(BrowserAction::CloseTab), // Changed from AddBookmark
                    5 => Some(BrowserAction::NewTab),
                    _ => None,
                };
            }
            button_x += button_widths[i] + button_spacing;
        }
        println!("[browser_screen] No button hit");

        None
    }
}

// Simple Button struct for browser screen
pub struct Button {
    pub text: String,
    pub action: Key,
    pub active: bool,
    pub changed: bool,
    pub background: bool,
    pub fraction: f32,
}

impl Button {
    fn new_icon_with_fraction(icon_name: &str, action: Key, background: bool, fraction: f32) -> Button {
        Button {
            text: icon_name.to_string(),
            action,
            active: false,
            changed: false,
            background,
            fraction,
        }
    }

    pub fn set_active<F>(&mut self, uinput: &mut input_linux::uinput::UInputHandle<F>, active: bool) 
    where F: std::os::fd::AsRawFd {
        println!("[browser_screen::Button] set_active called: text='{}', old_active={}, new_active={}", self.text, self.active, active);
        if self.active != active {
            self.active = active;
            self.changed = true;
            
            // Don't send uinput events for browser keys - they're only for visual feedback
            // Browser actions are handled via browser helper commands
        }
    }
}

#[derive(Debug, Clone)]
pub enum BrowserAction {
    Back,
    Forward,
    Refresh,
    Home,
    AddBookmark,
    NewTab,
    AddressBar,
    BookmarksManager,
    CloseTab,
} 