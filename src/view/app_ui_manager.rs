use cairo::Context;
use crate::view::media_player_screen::{MediaPlayerScreen, MediaPlayerAction};
use crate::view::browser_screen::{BrowserScreen, BrowserAction};
use crate::view::spotify_screen::{SpotifyScreen, SpotifyAction};
use crate::view::module_screen::draw_module_screen;

// Centralized state for media player window classes - easy to edit and maintain
pub const MEDIA_PLAYER_WINDOW_CLASSES: &[&str] = &["vlc", "org.kde.dragonplayer","dragonplayer", "smplayer", "spotify"];

// Helper function to check if a window class is a media player
pub fn is_media_player_window_class(window_class: &str) -> bool {
    MEDIA_PLAYER_WINDOW_CLASSES.contains(&window_class)
}

// Centralized state for browser window classes - easy to edit and maintain
pub const BROWSER_WINDOW_CLASSES: &[&str] = &["firefox", "chrome", "chromium", "brave", "brave-browser", "edge", "safari", "opera", "google-chrome","zen"];

// Helper function to check if a window class is a browser
pub fn is_browser_window_class(window_class: &str) -> bool {
    BROWSER_WINDOW_CLASSES.contains(&window_class)
}


pub struct AppUiManager {
    pub media_player_screen: MediaPlayerScreen,
    pub browser_screen: BrowserScreen,
    pub spotify_screen: SpotifyScreen,
}

impl AppUiManager {
    pub fn new() -> Self {
        AppUiManager {
            media_player_screen: MediaPlayerScreen::new(),
            browser_screen: BrowserScreen::new(),
            spotify_screen: SpotifyScreen::new(),
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
            "spotify" => {
                self.spotify_screen.draw(c, x, y, width, height, radius, anim_progress, drag_position);
            }
            class if is_media_player_window_class(class) => {
                self.media_player_screen.draw(c, x, y, width, height, radius, anim_progress, drag_position);
            }
            class if is_browser_window_class(class) => {
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

#[derive(Debug, Clone)]
pub enum AppAction {
    MediaPlayer(MediaPlayerAction),
    Browser(BrowserAction),
    Spotify(SpotifyAction),
} 