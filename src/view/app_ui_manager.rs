use cairo::Context;
use crate::view::vlc_screen::{VlcScreen, VlcAction};
use crate::view::browser_screen::{BrowserScreen, BrowserAction};
use crate::view::module_screen::draw_module_screen;

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
        window_class: Option<&str>, // Change to Option to handle None case
        drag_position: Option<f64>, // Add drag position parameter
        modified_regions: &mut Vec<drm::control::ClipRect>,
    ) {
        match window_class {
            Some(class) => {
                match class.to_lowercase().as_str() {
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
                        self.draw_default_ui(c, x, y, width, height, radius, anim_progress, class);
                    }
                }
            }
            None => {
                // No window class available (logout state) - show empty module screen
                self.draw_default_ui(c, x, y, width, height, radius, anim_progress, "");
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
        // Use the unified module screen drawing function
        draw_module_screen(
            c,
            x,
            y,
            width,
            height,
            radius,
            0, // area_height not used in text-only mode
            true, // complete_redraw
            window_class,
            anim_progress,
            true, // Show pill background for default UI
        );
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