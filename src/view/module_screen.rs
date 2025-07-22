use cairo::Context;
use drm::control::ClipRect;
use crate::services::sessionmanager::SessionState;
use crate::config::Config;
use crate::Key;

const BUTTON_COLOR_ACTIVE: f64 = 0.600;
const BUTTON_COLOR_INACTIVE: f64 = 0.350;

/// Draws the modules section of the app.
pub fn draw_module_section(
    c: &Context,
    split_modules: &mut [crate::Button],
    modules_button_widths: &[f64],
    modules_width: f64,
    modules_count: usize,
    left_edge: f64,
    bot: f64,
    top: f64,
    radius: f64,
    height: i32,
    config: &Config,
    complete_redraw: bool,
    modified_regions: &mut Vec<ClipRect>,
    session_state: Option<&SessionState>,
) {
    let mut local_left_edge = left_edge;
    for (i, button) in split_modules.iter_mut().enumerate() {
        let this_button_width = modules_button_widths[i];
        if button.changed || complete_redraw {
            let color = if button.active {
                BUTTON_COLOR_ACTIVE
            } else if config.show_button_outlines {
                BUTTON_COLOR_INACTIVE
            } else {
                0.0
            };
            if !complete_redraw {
                c.set_source_rgb(0.0, 0.0, 0.0);
                c.rectangle(local_left_edge, bot - radius, this_button_width, top - bot + radius * 2.0);
                c.fill().unwrap();
            }
            if (button.action != Key::Unknown && button.action != Key::Time && button.action != Key::Macro1 && button.action != Key::Macro2 && button.action != Key::Macro3 && button.action != Key::Macro4) && (button.background || button.active) {
                c.set_source_rgb(color, color, color);
                c.new_sub_path();
                let left = local_left_edge + radius;
                let right = (local_left_edge + this_button_width.ceil()) - radius;
                c.arc(right, bot, radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
                c.arc(right, top, radius, (0.0f64).to_radians(), (90.0f64).to_radians());
                c.arc(left, top, radius, (90.0f64).to_radians(), (180.0f64).to_radians());
                c.arc(left, bot, radius, (180.0f64).to_radians(), (270.0f64).to_radians());
                c.close_path();
                c.fill().unwrap();
            }
            c.set_source_rgb(1.0, 1.0, 1.0);
            button.render(c, height, local_left_edge, this_button_width.ceil() as u64, 0.0);
            // Show current user info and status on first module if available

            button.changed = false;
            if !complete_redraw {
                modified_regions.push(ClipRect::new(
                    height as u16 - top as u16 - radius as u16,
                    local_left_edge as u16,
                    height as u16 - bot as u16 + radius as u16,
                    local_left_edge as u16 + this_button_width as u16
                ));
            }
        }
        local_left_edge += this_button_width;
        if i != modules_count - 1 {
            local_left_edge += crate::BUTTON_SPACING_PX as f64;
        }
    }
}

pub fn draw_module_screen(
    c: &Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
    area_height: i32,
    complete_redraw: bool,
    window_class: &str,
    anim_progress: f64, // 0.0 = transparent, 1.0 = opaque
) {
    // --- Modern pill background (rounded rectangle) ---
    let pill_x = x;
    let pill_y = y - radius;
    let pill_w = width;
    let pill_h = height + radius * 2.0;
    c.save().unwrap();
    c.set_source_rgba(0.0, 0.0, 0.0, anim_progress); // Inactive button color, faded in
    // Draw rounded rectangle (pill)
    c.new_sub_path();
    c.arc(pill_x + pill_w - radius, pill_y + radius, radius, (-90.0f64).to_radians(), (0.0f64).to_radians());
    c.arc(pill_x + pill_w - radius, pill_y + pill_h - radius, radius, (0.0f64).to_radians(), (90.0f64).to_radians());
    c.arc(pill_x + radius, pill_y + pill_h - radius, radius, (90.0f64).to_radians(), (180.0f64).to_radians());
    c.arc(pill_x + radius, pill_y + radius, radius, (180.0f64).to_radians(), (270.0f64).to_radians());
    c.close_path();
    c.fill().unwrap();
    c.restore().unwrap();

    // --- Centered window class text ---
    c.save().unwrap();
    // Font size logic similar to login screen
    let text_size = (pill_h * 0.38).min(22.0).max(14.0);
    c.set_font_size(text_size);
    c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    let ext = c.text_extents(window_class).unwrap();
    let group_h = ext.height();
    let group_w = ext.width();
    let group_y = pill_y + (pill_h - group_h) / 2.0;
    let group_x = pill_x + (pill_w - group_w) / 2.0;
    c.set_source_rgba(1.0, 1.0, 1.0, anim_progress); // Red text, faded in
    c.move_to(group_x, group_y + group_h - 2.0);
    c.show_text(window_class).unwrap();
    c.restore().unwrap();
} 