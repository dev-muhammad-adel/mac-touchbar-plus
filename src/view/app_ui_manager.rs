use cairo::Context;
use crate::view::vlc_screen::{VlcScreen, VlcAction};
use crate::view::browser_screen::{BrowserScreen, BrowserAction};
use crate::view::module_screen::draw_module_screen;

#[derive(Debug, Clone, PartialEq)]
pub enum AppType {
    Browser,
    Vlc,
    Terminal,
    Editor,
    MediaPlayer,
    Graphics,
    Office,
    Generic,
}

pub struct AppUiManager {
    pub vlc_screen: VlcScreen,
    pub browser_screen: BrowserScreen,
    current_app: Option<AppType>,
    current_window_class: Option<String>, // Keep track of the actual window class
}

impl AppUiManager {
    pub fn new() -> Self {
        AppUiManager {
            vlc_screen: VlcScreen::new(),
            browser_screen: BrowserScreen::new(),
            current_app: None,
            current_window_class: None,
        }
    }

    pub async fn update_app(&mut self, window_class: &str) {
        let class_lower = window_class.to_lowercase();
        
        // Update the current window class
        self.current_window_class = Some(window_class.to_string());
        
        // Determine app type based on window class
        let new_app_type = match class_lower.as_str() {
            "firefox" | "chrome" | "chromium" | "brave" | "brave-browser" | 
            "edge" | "safari" | "opera" | "google-chrome" => AppType::Browser,
            "vlc" => AppType::Vlc,
            "alacritty" | "gnome-terminal" | "konsole" | "xterm" | "kitty" | 
            "terminator" | "tilix" | "urxvt" | "st" | "wezterm" => AppType::Terminal,
            "code" | "vscodium" | "atom" | "sublime_text" | "vim" | "nvim" | 
            "emacs" | "gedit" | "nano" => AppType::Editor,
            "spotify" | "rhythmbox" | "banshee" | "clementine" | "amarok" => AppType::MediaPlayer,
            "gimp" | "inkscape" | "krita" | "blender" | "photoshop" => AppType::Graphics,
            "libreoffice-writer" | "libreoffice-calc" | "libreoffice-impress" |
            "writer" | "calc" | "impress" | "abiword" | "gnumeric" => AppType::Office,
            _ => AppType::Generic,
        };
        
        // Only update if the app type changed
        if self.current_app.as_ref() != Some(&new_app_type) {
            self.current_app = Some(new_app_type.clone());
            
            match new_app_type {
                AppType::Browser => {
                    println!("[AppUI] Detected browser: {}", window_class);
                }
                AppType::Vlc => {
                    println!("[AppUI] Detected VLC media player");
                }
                AppType::Terminal => {
                    println!("[AppUI] Detected terminal: {}", window_class);
                }
                AppType::Editor => {
                    println!("[AppUI] Detected editor: {}", window_class);
                }
                AppType::MediaPlayer => {
                    println!("[AppUI] Detected media player: {}", window_class);
                }
                AppType::Graphics => {
                    println!("[AppUI] Detected graphics application: {}", window_class);
                }
                AppType::Office => {
                    println!("[AppUI] Detected office application: {}", window_class);
                }
                AppType::Generic => {
                    println!("[AppUI] Detected application: {} (using generic interface)", window_class);
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

    /// Get the current app type
    pub fn get_current_app_type(&self) -> Option<&AppType> {
        self.current_app.as_ref()
    }
    
    /// Get the current window class
    pub fn get_current_window_class(&self) -> Option<&String> {
        self.current_window_class.as_ref()
    }
    
    /// Check if the current app supports custom touch bar functionality
    pub fn has_custom_functionality(&self) -> bool {
        match self.current_app {
            Some(AppType::Browser) | Some(AppType::Vlc) => true,
            _ => false,
        }
    }
    
    /// Get app-specific context information for display
    pub fn get_app_context(&self) -> String {
        match (&self.current_app, &self.current_window_class) {
            (Some(AppType::Browser), Some(class)) => format!("Browser: {}", class),
            (Some(AppType::Vlc), _) => "VLC Media Player".to_string(),
            (Some(AppType::Terminal), Some(class)) => format!("Terminal: {}", class),
            (Some(AppType::Editor), Some(class)) => format!("Editor: {}", class),
            (Some(AppType::MediaPlayer), Some(class)) => format!("Media: {}", class),
            (Some(AppType::Graphics), Some(class)) => format!("Graphics: {}", class),
            (Some(AppType::Office), Some(class)) => format!("Office: {}", class),
            (Some(AppType::Generic), Some(class)) => class.clone(),
            _ => "No Application".to_string(),
        }
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