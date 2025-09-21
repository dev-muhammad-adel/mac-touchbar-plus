//! Touch handling for LayerKeys1 (App Layer 1) specific groups
//! 
//! This module contains specialized touch handlers for the two main groups
//! in LayerKeys1: modules (left side) and media (right side).

pub mod media_touch;
pub mod modules_touch;
pub mod modules;

pub use media_touch::MediaTouchHandler;
pub use modules_touch::ModulesTouchHandler; 