#![doc(html_root_url = "https://docs.rs/input-linux-sys/0.8.0/")]

pub use nix::{Error, Result};
pub use nix::errno::Errno;

mod events;
pub use crate::events::*;

mod input;
pub use crate::input::*;

mod uinput;
pub use crate::uinput::*;
