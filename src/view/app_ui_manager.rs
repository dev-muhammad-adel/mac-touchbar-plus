use cairo::Context;
use crate::view::media_player_screen::{MediaPlayerScreen, MediaPlayerAction};
use crate::view::browser_screen::{BrowserScreen, BrowserAction};
use crate::view::spotify_screen::{SpotifyScreen, SpotifyAction};
use crate::view::generic_background_screen::{GenericBackgroundScreen, GenericBackgroundAction};
use crate::view::module_screen::draw_module_screen;
use std::io::Write;

// Centralized state for media player window classes - easy to edit and maintain
pub const MEDIA_PLAYER_WINDOW_CLASSES: &[&str] = &["vlc", "org.kde.dragonplayer","dragonplayer", "smplayer", "spotify"];

// Helper function to check if a window class is a media player
pub fn is_media_player_window_class(window_class: &str) -> bool {
    let window_class_lower = window_class.to_lowercase();
    MEDIA_PLAYER_WINDOW_CLASSES.iter().any(|&class| class.to_lowercase() == window_class_lower)
}

// Centralized state for browser window classes - easy to edit and maintain
pub const BROWSER_WINDOW_CLASSES: &[&str] = &["firefox", "chrome", "chromium", "brave", "brave-browser", "edge", "safari", "opera", "google-chrome","zen"];

// Helper function to check if a window class is a browser
pub fn is_browser_window_class(window_class: &str) -> bool {
    let window_class_lower = window_class.to_lowercase();
    BROWSER_WINDOW_CLASSES.iter().any(|&class| class.to_lowercase() == window_class_lower)
}



pub struct AppUiManager {
    pub media_player_screen: MediaPlayerScreen,
    pub browser_screen: BrowserScreen,
    pub spotify_screen: SpotifyScreen,
    pub generic_background_screen: GenericBackgroundScreen,
    pub generic_media_enabled: bool,
}

impl AppUiManager {
    pub fn new() -> Self {
        AppUiManager {
            media_player_screen: MediaPlayerScreen::new(),
            browser_screen: BrowserScreen::new(),
            spotify_screen: SpotifyScreen::new(),
            generic_background_screen: GenericBackgroundScreen::new(),
            generic_media_enabled: false,
        }
    }
    
    pub fn close_generic_media(&mut self) {
        self.generic_media_enabled = false;
    }
    
    pub fn update_available_services_list(&mut self, services: Vec<String>) {
        self.generic_background_screen.update_available_services(services);
    }
    
    pub fn update_available_services_list_with_auto_select(&mut self, services: Vec<String>, background_service_helper_stream: &mut Option<std::os::unix::net::UnixStream>) {
        self.generic_background_screen.update_available_services_with_auto_select(services, background_service_helper_stream);
    }
    
    pub fn handle_generic_background_action(&mut self, action: GenericBackgroundAction, background_service_helper_stream: &mut Option<std::os::unix::net::UnixStream>) {
        match action {
            GenericBackgroundAction::ToggleMprisItem(index) => {
                self.generic_background_screen.toggle_mpris_item(index);
                // Send selection command to background service helper
                self.generic_background_screen.send_selection_command(background_service_helper_stream);
            }
            GenericBackgroundAction::CloseGenericMedia => {
                self.close_generic_media();
            }
            GenericBackgroundAction::BackgroundServicePlayerPlayPause => {
                self.send_media_action_to_helper("play_pause", background_service_helper_stream);
            }
            GenericBackgroundAction::BackgroundServicePlayerNext => {
                self.send_media_action_to_helper("next", background_service_helper_stream);
            }
            GenericBackgroundAction::BackgroundServicePlayerPrevious => {
                self.send_media_action_to_helper("previous", background_service_helper_stream);
            }
            GenericBackgroundAction::BackgroundServicePlayerSeek(ratio) => {
                self.send_media_action_to_helper(&format!("seek:{}", ratio), background_service_helper_stream);
            }
            GenericBackgroundAction::BackgroundServicePlayerDragHead(ratio) => {
                self.send_media_action_to_helper(&format!("seek:{}", ratio), background_service_helper_stream);
            }
            _ => {
                // Handle other actions if needed
            }
        }
    }
    
    // Helper function to send media actions to the background service helper
    fn send_media_action_to_helper(&self, action: &str, background_service_helper_stream: &mut Option<std::os::unix::net::UnixStream>) {
        if let Some(stream) = background_service_helper_stream {
            let command = format!("media_action:{}\n", action);
            if let Err(e) = stream.write_all(command.as_bytes()) {
                eprintln!("[app_ui_manager] Failed to send media action to helper: {}", e);
            } else {
                println!("[app_ui_manager] Sent media action to helper: {}", action);
            }
        } else {
            println!("[app_ui_manager] No background service helper stream available for action: {}", action);
        }
    }
    
    pub fn hit_test_generic_background(
        &mut self,
        touch_x: f64,
        touch_y: f64,
        screen_x: f64,
        screen_y: f64,
        screen_width: f64,
        screen_height: f64,
        radius: f64,
    ) -> Option<GenericBackgroundAction> {
        if self.generic_media_enabled {
            self.generic_background_screen.hit_test(touch_x, touch_y, screen_x, screen_y, screen_width, screen_height, radius)
        } else {
            None
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
        // If generic media is enabled, always show generic media screen regardless of window class
        if self.generic_media_enabled {
            self.generic_background_screen.draw(c, x, y, width, height, radius, anim_progress, drag_position);
            return;
        }
        
        println!("[app_ui_manager] draw_app_ui called with window_class: {:?}", window_class);
        match window_class {
            Some(class) => {
                match class.to_lowercase().as_str() {
            class if is_media_player_window_class(class) => {
                // Check if it's Spotify specifically for special UI
                if class == "spotify" {
                    println!("[app_ui_manager] Drawing SPOTIFY screen");
                    self.spotify_screen.draw(c, x, y, width, height, radius, anim_progress, drag_position);
                } else {
                    println!("[app_ui_manager] Drawing MEDIA PLAYER screen for: {}", class);
                    self.media_player_screen.draw(c, x, y, width, height, radius, anim_progress, drag_position);
                }
            }
            class if is_browser_window_class(class) => {
                println!("[app_ui_manager] Drawing BROWSER screen for: {} with {} active buttons", class, self.browser_screen.buttons.iter().filter(|b| b.active).count());
                // Check if any browser buttons have changed for partial redraw
                let any_browser_button_changed = self.browser_screen.buttons.iter().any(|b| b.changed);
                let complete_redraw = !any_browser_button_changed; // Use complete redraw if no buttons changed
                self.browser_screen.draw(c, x, y, width, height, radius, anim_progress, complete_redraw, modified_regions);
            }
            _ => {
                // Fall back to default module screen behavior
                println!("[app_ui_manager] Drawing DEFAULT UI for: {}", class);
                self.draw_default_ui(c, x, y, width, height, radius, anim_progress, class);
            }
        }
    }
            None => {
                // No window class available (logout state) - show empty module screen
                println!("[app_ui_manager] Drawing DEFAULT UI for unknown window");
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
        
        // If generic media is enabled, always test against generic media screen
        if self.generic_media_enabled {
            if let Some(generic_action) = self.generic_background_screen.hit_test(touch_x, touch_y, x, y, width, height, radius) {
                Some(AppAction::GenericBackground(generic_action))
            } else {
                None
            }
        } else {
            match window_class.to_lowercase().as_str() {
                "spotify" => {
                    if let Some(spotify_action) = self.spotify_screen.hit_test(touch_x, touch_y, x, y, width, height, radius) {
                        Some(AppAction::Spotify(spotify_action))
                    } else {
                        None
                    }
                }
                class if is_media_player_window_class(class) => {
                    if let Some(media_action) = self.media_player_screen.hit_test(touch_x, touch_y, x, y, width, height, radius) {
                        Some(AppAction::MediaPlayer(media_action))
                    } else {
                        None
                    }
                }
                class if is_browser_window_class(class) => {
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
}

#[derive(Debug, Clone)]
pub enum AppAction {
    MediaPlayer(MediaPlayerAction),
    Browser(BrowserAction),
    Spotify(SpotifyAction),
    GenericBackground(GenericBackgroundAction),
} 