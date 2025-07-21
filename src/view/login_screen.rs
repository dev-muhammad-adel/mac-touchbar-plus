use cairo::{Context};

const BUTTON_COLOR_INACTIVE: f64 = 0.350;
const PILL_RADIUS: f64 = 8.0;

/// Draws the login screen with fade in/out using anim_progress as alpha (0.0 = transparent, 1.0 = opaque)
pub fn draw_login_screen(
    c: &Context,
    login_x: f64,
    _login_y: f64,
    login_area_width: f64,
    _login_area_height: f64,
    top: f64,
    bot: f64,
    _radius: f64,
    _height: i32,
    complete_redraw: bool,
    modified_regions: &mut Vec<drm::control::ClipRect>,
    _session_state: Option<&crate::services::sessionmanager::SessionState>,
    anim_progress: f64, // 0.0 = transparent, 1.0 = opaque
) {
    // --- Media-style vertical position and height ---
    let pill_y = bot - PILL_RADIUS;
    let pill_h = top - bot + PILL_RADIUS * 2.0;
    let pill_x = login_x;
    let pill_w = login_area_width;

    // --- Modern single-line message with emoji ---
    let message = "Welcome to login screen. Unlock your session.";
    let text_size = (pill_h * 0.38).min(22.0).max(14.0); // 14–22px, a bit larger for single line
    c.set_font_size(text_size);
    c.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    let ext = c.text_extents(&message).unwrap();
    let group_h = ext.height();
    let group_w = ext.width();
    // Center the text in the pill
    let group_y = pill_y + (pill_h - group_h) / 2.0;
    let group_x = pill_x + (pill_w - group_w) / 2.0;

    // --- Draw text with emoji (no background) ---
    c.set_source_rgba(1.0, 1.0, 1.0, anim_progress);
    c.move_to(group_x, group_y + group_h - 2.0);
    c.show_text(&message).unwrap();

    // Add modified region for login screen
    if !complete_redraw {
        modified_regions.push(drm::control::ClipRect::new(
            pill_y as u16,
            pill_x as u16,
            pill_h as u16,
            (pill_x + pill_w) as u16
        ));
    }
}

fn rounded_rect(c: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    c.new_sub_path();
    c.arc(x + w - r, y + r, r, (-90.0f64).to_radians(), (0.0f64).to_radians());
    c.arc(x + w - r, y + h - r, r, (0.0f64).to_radians(), (90.0f64).to_radians());
    c.arc(x + r, y + h - r, r, (90.0f64).to_radians(), (180.0f64).to_radians());
    c.arc(x + r, y + r, r, (180.0f64).to_radians(), (270.0f64).to_radians());
    c.close_path();
} 