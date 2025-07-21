use std::panic;
use drm::control::ClipRect;
use crate::display::DrmBackend;
use nix::sys::signal::{Signal, SigSet};

const CRASH_BITMAP: &[u8] = include_bytes!("crash_bitmap.raw");

pub fn setup_crash_handler() {
    // Set custom panic hook
    panic::set_hook(Box::new(|panic_info| {
        eprintln!("Application crashed! Panic info: {}", panic_info);
    }));
}

pub fn display_crash_screen(drm: &mut DrmBackend) {
    let (height, width) = drm.mode().size();
    let mut map = match drm.map() {
        Ok(map) => map,
        Err(e) => {
            eprintln!("Failed to map DRM buffer: {}", e);
            return;
        }
    };

    let data = map.as_mut();
    let mut wptr = 0;

    // Draw crash bitmap
    for byte in CRASH_BITMAP {
        for i in 0..8 {
            let bit = ((byte >> i) & 0x1) == 0;
            let color = if bit { 0xFF } else { 0x0 };
            data[wptr] = color;
            data[wptr + 1] = color;
            data[wptr + 2] = color;
            data[wptr + 3] = color;
            wptr += 4;
        }
    }

    drop(map);

    // Display the crash screen
    if let Err(e) = drm.dirty(&[ClipRect::new(0, 0, height as u16, width as u16)]) {
        eprintln!("Failed to update display with crash screen: {}", e);
    }
}

pub fn wait_for_termination() {
    let mut sigset = SigSet::empty();
    sigset.add(Signal::SIGTERM);
    let _ = sigset.wait(); // Wait for SIGTERM signal
}

pub fn handle_crash(drm: &mut DrmBackend) {
    display_crash_screen(drm);
    wait_for_termination();
}

// Safe wrapper that takes ownership of DrmBackend
pub fn run_with_crash_handler<F, T>(drm: DrmBackend, f: F) -> DrmBackend 
where 
    F: FnOnce(&mut DrmBackend) -> T + panic::UnwindSafe
{
    setup_crash_handler();

    let mut drm = drm; // Take ownership here
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        f(&mut drm)
    }));

    if result.is_err() {
        handle_crash(&mut drm);
    }

    drm
} 