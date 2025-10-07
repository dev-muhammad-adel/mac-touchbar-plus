//! Configuration loading, validation, and management.
use crate::fonts::{FontConfig, Pattern};
use crate::LayerKey;
use crate::layers::{Button, FunctionLayer, Layer, FnLayer, MediaLayer, Custom2Layer, Custom3Layer};
use anyhow::Error;
use cairo::FontFace;
use freetype::Library as FtLibrary;
use input_linux::Key;
use nix::{
    errno::Errno,
    sys::inotify::{AddWatchFlags, InitFlags, Inotify, WatchDescriptor},
};
use serde::Deserialize;
use std::{collections::HashMap, fs::read_to_string, os::fd::AsFd};

const USER_CFG_PATH: &'static str = "/etc/tiny-dfr/config.json";

pub struct Config {
    pub show_button_outlines: bool,
    pub enable_pixel_shift: bool,
    pub font_renderer: String,
    pub font_style_cairo: String,
    pub bold_cairo: bool,
    pub italic_cairo: bool,
    pub font_face: FontFace,
    pub adaptive_brightness: bool,
    pub active_brightness: u32,
}

pub struct Theme {
    pub media_icon_theme: String,
    pub app_icon_theme: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AppLayerSplitSection {
    width: f32,
    #[serde(default)]
    items: Vec<ButtonConfig>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AppLayerSplitedLayout {
    #[serde(rename = "AppLayerKeys1Media")]
    app_layer_keys1_media: AppLayerSplitSection,
    #[serde(rename = "AppLayerKeys1Modules")]
    app_layer_keys1_modules: AppLayerSplitSection,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ConfigProxy {
    show_button_outlines: Option<bool>,
    enable_pixel_shift: Option<bool>,
    font_renderer: Option<String>,
    font_style: Option<String>,
    bold: Option<bool>,
    italic: Option<bool>,
    font_template: Option<String>,
    media_icon_theme: Option<String>,
    app_icon_theme: Option<String>,
    adaptive_brightness: Option<bool>,
    active_brightness: Option<u32>,
    primary_layer_keys: Option<Vec<ButtonConfig>>,
    app_layer_keys1: Option<Vec<ButtonConfig>>,
    app_layer_keys2: Option<Vec<ButtonConfig>>,
    app_layer_keys3: Option<Vec<ButtonConfig>>,
    app_layer_splited_layout: Option<AppLayerSplitedLayout>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ButtonConfig {
    #[serde(alias = "Svg")]
    pub icon: Option<String>,
    pub path: Option<String>,
    pub mode: Option<String>,
    pub text: Option<String>,
    pub background: Option<bool>,
    pub action: Key,
    pub fraction: Option<f32>, // Optional per-button width fraction
    pub special_type: Option<String>, // Special button type (e.g., "toggle", "macro", etc.)
}

fn load_font(name: &str) -> FontFace {
    let fontconfig = FontConfig::new();
    let mut pattern = Pattern::new(name);
    fontconfig.perform_substitutions(&mut pattern);
    let pat_match = match fontconfig.match_pattern(&pattern) {
        Ok(pat) => pat,
        Err(_) => panic!("Unable to find specified font. If you are using the default config, make sure you have at least one font installed")
    };
    let file_name = pat_match.get_file_name();
    let file_idx = pat_match.get_font_index();
    let ft_library = FtLibrary::init().unwrap();
    let face = ft_library.new_face(file_name, file_idx).unwrap();
    FontFace::create_from_ft(&face).unwrap()
}

fn load_theme() -> Theme {
    let mut base = serde_json::from_str::<ConfigProxy>(
        &read_to_string("/usr/share/tiny-dfr/config.json").unwrap(),
    )
    .unwrap();
    let user = read_to_string("/etc/tiny-dfr/config.json")
        .map_err::<Error, _>(|e| e.into())
        .and_then(|r| Ok(serde_json::from_str::<ConfigProxy>(&r)?));
    if let Ok(user) = user {
        base.media_icon_theme = user.media_icon_theme.or(base.media_icon_theme);
        base.app_icon_theme = user.app_icon_theme.or(base.app_icon_theme);
    };
    Theme {
        media_icon_theme: base.media_icon_theme.unwrap(),
        app_icon_theme: base.app_icon_theme.unwrap(),
    }
}

fn load_config(width: u16) -> (Config, HashMap<LayerKey, Box<dyn Layer>>) {
    println!("/usr/share/tiny-dfr/config.json");
    let mut base = serde_json::from_str::<ConfigProxy>(
        &read_to_string("/usr/share/tiny-dfr/config.json").unwrap(),
    )
    .unwrap();
    let user = read_to_string(USER_CFG_PATH)
        .map_err::<Error, _>(|e| e.into())
        .and_then(|r| Ok(serde_json::from_str::<ConfigProxy>(&r)?));
    if let Ok(user) = user {
        base.show_button_outlines = user.show_button_outlines.or(base.show_button_outlines);
        base.enable_pixel_shift = user.enable_pixel_shift.or(base.enable_pixel_shift);
        base.font_renderer = user.font_renderer.or(base.font_renderer);
        base.font_style = user.font_style.or(base.font_style);
        base.bold = user.bold.or(base.bold);
        base.italic = user.italic.or(base.italic);
        base.font_template = user.font_template.or(base.font_template);
        base.adaptive_brightness = user.adaptive_brightness.or(base.adaptive_brightness);
        base.primary_layer_keys = user.primary_layer_keys.or(base.primary_layer_keys);
        base.app_layer_keys1 = user.app_layer_keys1.or(base.app_layer_keys1);
        base.app_layer_keys2 = user.app_layer_keys2.or(base.app_layer_keys2);
        base.app_layer_keys3 = user.app_layer_keys3.or(base.app_layer_keys3);
        base.active_brightness = user.active_brightness.or(base.active_brightness);
        base.app_layer_splited_layout = user
            .app_layer_splited_layout
            .or(base.app_layer_splited_layout);
    };
    let fkey_layer = FnLayer::with_config(base.primary_layer_keys.unwrap());
    // --- App Layer 1: support split layout ---
    let app_layer1 = if let Some(split) = base.app_layer_splited_layout {
        MediaLayer::with_split(
            split.app_layer_keys1_modules.width,
            split.app_layer_keys1_media.items,
            split.app_layer_keys1_media.width,
        )
    } else {
        // Fallback to standard layer if no split layout
        MediaLayer::with_split(0.5, base.app_layer_keys1.unwrap(), 0.5)
    };
    // ---
    let app_layer2 = Custom2Layer::with_config(base.app_layer_keys2.unwrap());
    let app_layer3 = Custom3Layer::with_config(base.app_layer_keys3.unwrap());
    let mut layers: HashMap<LayerKey, Box<dyn Layer>> = HashMap::new();
    layers.insert(LayerKey::Media, Box::new(app_layer1));
    layers.insert(LayerKey::Fn, Box::new(fkey_layer));
    layers.insert(LayerKey::Custom2, Box::new(app_layer2));
    layers.insert(LayerKey::Custom3, Box::new(app_layer3));
    if width >= 2170 {
        for layer in layers.values_mut() {
            layer.add_esc_button();
        }
    }
    let cfg = Config {
        show_button_outlines: base.show_button_outlines.unwrap(),
        enable_pixel_shift: base.enable_pixel_shift.unwrap(),
        adaptive_brightness: base.adaptive_brightness.unwrap(),
        font_renderer: base.font_renderer.unwrap(),
        font_style_cairo: base.font_style.unwrap(),
        bold_cairo: base.bold.unwrap(),
        italic_cairo: base.italic.unwrap(),
        font_face: load_font(&base.font_template.unwrap()),
        active_brightness: base.active_brightness.unwrap(),
    };
    (cfg, layers)
}

pub struct ConfigManager {
    inotify_fd: Inotify,
    watch_desc: Option<WatchDescriptor>,
}

fn arm_inotify(inotify_fd: &Inotify) -> Option<WatchDescriptor> {
    let flags = AddWatchFlags::IN_MOVED_TO | AddWatchFlags::IN_CLOSE | AddWatchFlags::IN_ONESHOT;
    match inotify_fd.add_watch(USER_CFG_PATH, flags) {
        Ok(wd) => Some(wd),
        Err(Errno::ENOENT) => None,
        e => Some(e.unwrap()),
    }
}

impl ConfigManager {
    pub fn new() -> ConfigManager {
        let inotify_fd = Inotify::init(InitFlags::IN_NONBLOCK).unwrap();
        let watch_desc = arm_inotify(&inotify_fd);
        ConfigManager {
            inotify_fd,
            watch_desc,
        }
    }
    pub fn load_config(&self, width: u16) -> (Config, HashMap<LayerKey, Box<dyn Layer>>) {
        load_config(width)
    }
    pub fn load_theme(&self) -> Theme {
        load_theme()
    }
    pub fn update_config(
        &mut self,
        cfg: &mut Config,
        layers: &mut HashMap<LayerKey, Box<dyn Layer>>,
        width: u16,
    ) -> bool {
        if self.watch_desc.is_none() {
            self.watch_desc = arm_inotify(&self.inotify_fd);
            return false;
        }
        let evts = match self.inotify_fd.read_events() {
            Ok(e) => e,
            Err(Errno::EAGAIN) => Vec::new(),
            r => r.unwrap(),
        };
        let mut ret = false;
        for evt in evts {
            if evt.wd != self.watch_desc.unwrap() {
                continue;
            }
            let parts = load_config(width);
            *cfg = parts.0;
            *layers = parts.1;
            ret = true;
            self.watch_desc = arm_inotify(&self.inotify_fd);
        }
        ret
    }
    pub fn fd(&self) -> &impl AsFd {
        &self.inotify_fd
    }
}
