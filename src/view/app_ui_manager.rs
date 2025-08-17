use cairo::Context;
use crate::view::vlc_screen::{VlcScreen, VlcAction};
use crate::view::browser_screen::{BrowserScreen, BrowserAction};

pub struct AppUiManager {
    pub vlc_screen: VlcScreen,
    pub browser_screen: BrowserScreen,
    current_app: Option<String>,
}

impl AppUiManager {
    pub fn new() -> Self {
        AppUiManager {
            vlc_screen: VlcScreen::new(),
            browser_screen: BrowserScreen::new(),
            current_app: None,
        }
    }

    pub async fn update_app(&mut self, window_class: &str) {
        if self.current_app.as_ref() != Some(&window_class.to_string()) {
            self.current_app = Some(window_class.to_string());
            
            // Update app-specific status
            match window_class.to_lowercase().as_str() {
                "vlc" | "vlc.exe" => {
                    println!("[AppUI] Detected VLC");
                }
                            "firefox" | "chrome" | "chromium" | "brave" | "brave-browser" | "edge" | "safari" | "opera" | "google-chrome" => {
                println!("[AppUI] Detected Browser: {}", window_class);
            }
                _ => {
                    println!("[AppUI] Unknown app: {}", window_class);
                }
            }
        }
    }

    pub fn draw_app_ui(
        &mut self,
        c: &Context,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f64,
        anim_progress: f64,
        window_class: &str,
        drag_position: Option<f64>, // Add drag position parameter
        modified_regions: &mut Vec<drm::control::ClipRect>,
    ) {
        match window_class.to_lowercase().as_str() {
            "vlc" | "vlc.exe" => {
                self.vlc_screen.draw(c, x, y, width, height, radius, anim_progress, drag_position);
            }
            "firefox" | "chrome" | "chromium" | "brave" | "brave-browser" | "edge" | "safari" | "opera" | "google-chrome" => {
                println!("[app_ui_manager] Drawing browser screen with {} active buttons", self.browser_screen.buttons.iter().filter(|b| b.active).count());
                // Check if any browser buttons have changed for partial redraw
                let any_browser_button_changed = self.browser_screen.buttons.iter().any(|b| b.changed);
                let complete_redraw = !any_browser_button_changed; // Use complete redraw if no buttons changed
                self.browser_screen.draw(c, x, y, width, height, radius, anim_progress, complete_redraw, modified_regions);
            }
            _ => {
                // Fall back to default module screen behavior
                self.draw_default_ui(c, x, y, width, height, radius, anim_progress, window_class);
            }
        }
    }

    fn draw_default_ui(
        &self,
        c: &Context,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f64,
        anim_progress: f64,
        window_class: &str,
    ) {
        // Draw the default module screen UI (just the app name)
        let pill_x = x;
        let pill_y = y - radius;
        let pill_w = width;
        let pill_h = height + radius * 2.0;
        
        // Always clear the area first to prevent text overlap
        c.save().unwrap();
        c.set_source_rgb(0.0, 0.0, 0.0);
        c.rectangle(pill_x, pill_y, pill_w, pill_h);
        c.fill().unwrap();
        c.restore().unwrap();
        
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

        // Draw app name
        c.save().unwrap();
        let text_size = (pill_h * 0.38).min(22.0).max(14.0);
        c.set_font_size(text_size);
        c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        let ext = c.text_extents(window_class).unwrap();
        let group_h = ext.height();
        let group_w = ext.width();
        let group_y = pill_y + (pill_h - group_h) / 2.0;
        let group_x = pill_x + (pill_w - group_w) / 2.0;
        c.set_source_rgba(1.0, 1.0, 1.0, anim_progress);
        c.move_to(group_x, group_y + group_h - 2.0);
        c.show_text(window_class).unwrap();
        c.restore().unwrap();
    }

    pub fn hit_test_app_ui(
        &mut self,
        touch_x: f64,
        touch_y: f64,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f64,
        window_class: &str,
    ) -> Option<AppAction> {
        println!("[app_ui_manager] hit_test_app_ui called for window_class={}, touch_x={}, touch_y={}", window_class, touch_x, touch_y);
        match window_class.to_lowercase().as_str() {
            "vlc" | "vlc.exe" => {
                if let Some(vlc_action) = self.vlc_screen.hit_test(touch_x, touch_y, x, y, width, height, radius) {
                    Some(AppAction::Vlc(vlc_action))
                } else {
                    None
                }
            }
            "firefox" | "chrome" | "chromium" | "brave" | "brave-browser" | "edge" | "safari" | "opera" | "google-chrome" => {
                println!("[app_ui_manager] Calling browser_screen.hit_test");
                if let Some(browser_action) = self.browser_screen.hit_test(touch_x, touch_y, x, y, width, height, radius) {
                    println!("[app_ui_manager] Browser action detected: {:?}", browser_action);
                    Some(AppAction::Browser(browser_action))
                } else {
                    println!("[app_ui_manager] No browser action detected");
                    None
                }
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum AppAction {
    Vlc(VlcAction),
    Browser(BrowserAction),
} 