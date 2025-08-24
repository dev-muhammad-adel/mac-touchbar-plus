use std::mem::{size_of, size_of_val};
use nix::{
	self,
	sys::ioctl::ioctl_num_type,
	convert_ioctl_res,
	ioctl_read, ioctl_read_buf,
	ioctl_write_ptr, ioctl_write_int,
	request_code_read, request_code_write,
};
use libc::{c_char, c_int, c_uint, clockid_t, ioctl};

pub use libc::{
	timeval,
	input_event, input_id, input_absinfo, input_keymap_entry, input_mask,
	ff_replay, ff_trigger, ff_envelope, ff_effect, ff_constant_effect,
	ff_ramp_effect, ff_condition_effect, ff_periodic_effect, ff_rumble_effect,
};

/// Protocol version.
pub const EV_VERSION: c_int = 0x010001;

pub const ID_BUS: usize = 0;
pub const ID_VENDOR: usize = 1;
pub const ID_PRODUCT: usize = 2;
pub const ID_VERSION: usize = 3;

pub const BUS_PCI: u16 = 0x01;
pub const BUS_ISAPNP: u16 = 0x02;
pub const BUS_USB: u16 = 0x03;
pub const BUS_HIL: u16 = 0x04;
pub const BUS_BLUETOOTH: u16 = 0x05;
pub const BUS_VIRTUAL: u16 = 0x06;

pub const BUS_ISA: u16 = 0x10;
pub const BUS_I8042: u16 = 0x11;
pub const BUS_XTKBD: u16 = 0x12;
pub const BUS_RS232: u16 = 0x13;
pub const BUS_GAMEPORT: u16 = 0x14;
pub const BUS_PARPORT: u16 = 0x15;
pub const BUS_AMIGA: u16 = 0x16;
pub const BUS_ADB: u16 = 0x17;
pub const BUS_I2C: u16 = 0x18;
pub const BUS_HOST: u16 = 0x19;
pub const BUS_GSC: u16 = 0x1A;
pub const BUS_ATARI: u16 = 0x1B;
pub const BUS_SPI: u16 = 0x1C;
pub const BUS_RMI: u16 = 0x1D;
pub const BUS_CEC: u16 = 0x1E;
pub const BUS_INTEL_ISHTP: u16 = 0x1F;

pub const MT_TOOL_FINGER: u16 = 0;
pub const MT_TOOL_PEN: u16 = 1;
pub const MT_TOOL_PALM: u16 = 2;
pub const MT_TOOL_MAX: u16 = 2;

pub const FF_STATUS_STOPPED: u16 = 0x00;
pub const FF_STATUS_PLAYING: u16 = 0x01;
pub const FF_STATUS_MAX: u16 = 0x01;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct ff_effect_union {
	#[cfg(target_pointer_width = "64")]
	pub u: [u64; 4],
	#[cfg(target_pointer_width = "32")]
	pub u: [u32; 7],
}

impl<'a> From<&'a ff_effect> for &'a ff_effect_union {
	fn from(effect: &'a ff_effect) -> Self {
		unsafe {
			let raw = &effect.u as *const _ as *const _;
			&*raw
		}
	}
}

impl<'a> From<&'a mut ff_effect> for &'a mut ff_effect_union {
	fn from(effect: &'a mut ff_effect) -> Self {
		unsafe {
			let raw = &mut effect.u as *mut _ as *mut _;
			&mut *raw
		}
	}
}

impl ff_effect_union {
	pub fn constant(&self) -> &ff_constant_effect {
		unsafe {
			let raw = &self.u as *const _ as *const _;
			&*raw
		}
	}

	pub fn constant_mut(&mut self) -> &mut ff_constant_effect {
		unsafe {
			let raw = &mut self.u as *mut _ as *mut _;
			&mut *raw
		}
	}

	pub fn ramp(&self) -> &ff_ramp_effect {
		unsafe {
			let raw = &self.u as *const _ as *const _;
			&*raw
		}
	}

	pub fn ramp_mut(&mut self) -> &mut ff_ramp_effect {
		unsafe {
			let raw = &mut self.u as *mut _ as *mut _;
			&mut *raw
		}
	}

	pub fn periodic(&self) -> &ff_periodic_effect {
		unsafe {
			let raw = &self.u as *const _ as *const _;
			&*raw
		}
	}

	pub fn periodic_mut(&mut self) -> &mut ff_periodic_effect {
		unsafe {
			let raw = &mut self.u as *mut _ as *mut _;
			&mut *raw
		}
	}

	pub fn condition(&self) -> &[ff_condition_effect; 2] {
		unsafe {
			let raw = &self.u as *const _ as *const _;
			&*raw
		}
	}

	pub fn condition_mut(&mut self) -> &mut [ff_condition_effect; 2] {
		unsafe {
			let raw = &mut self.u as *mut _ as *mut _;
			&mut *raw
		}
	}

	pub fn rumble(&self) -> &ff_rumble_effect {
		unsafe {
			let raw = &self.u as *const _ as *const _;
			&*raw
		}
	}

	pub fn rumble_mut(&mut self) -> &mut ff_rumble_effect {
		unsafe {
			let raw = &mut self.u as *mut _ as *mut _;
			&mut *raw
		}
	}
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct repeat_settings {
	pub delay: c_uint,
	pub period: c_uint,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct input_mt_request_layout<T: ?Sized = [i32]> {
	pub code: u32,
	pub values: T,
}

ioctl_read! {
	/// get driver version
	ev_get_version, b'E', 0x01, c_int
}

ioctl_read! {
	/// get device ID
	ev_get_id, b'E', 0x02, input_id
}

ioctl_read! {
	/// get repeat settings
	ev_get_rep, b'E', 0x03, repeat_settings
}

ioctl_write_ptr! {
	/// set repeat settings
	ev_set_rep, b'E', 0x03, repeat_settings
}

ioctl_read! {
	/// get keycode
	ev_get_keycode, b'E', 0x04, [c_uint; 2]
}

ioctl_read! {
	/// get keycode
	ev_get_keycode_v2, b'E', 0x04, input_keymap_entry
}

ioctl_write_ptr! {
	/// set keycode
	ev_set_keycode, b'E', 0x04, [c_uint; 2]
}

ioctl_write_ptr! {
	/// set keycode
	ev_set_keycode_v2, b'E', 0x04, input_keymap_entry
}

ioctl_read_buf! {
	/// get device name
	ev_get_name, b'E', 0x06, c_char
}

ioctl_read_buf! {
	/// get physical location
	ev_get_phys, b'E', 0x07, c_char
}

ioctl_read_buf! {
	/// get unique identifier
	ev_get_uniq, b'E', 0x08, c_char
}

ioctl_read_buf! {
	/// get device properties
	ev_get_prop, b'E', 0x09, u8
}

/// get MT slot values
pub unsafe fn ev_get_mtslots(fd: c_int, buf: *mut input_mt_request_layout) -> nix::Result<i32> {
	// for some reason this isn't _IORW?
	convert_ioctl_res!(ioctl(fd, request_code_read!(b'E', 0x0a, size_of_val(&*buf)) as ioctl_num_type, buf))
}

ioctl_read_buf! {
	/// get global key state
	ev_get_key, b'E', 0x18, u8
}

ioctl_read_buf! {
	/// get all LEDs
	ev_get_led, b'E', 0x19, u8
}

ioctl_read_buf! {
	/// get all sounds status
	ev_get_snd, b'E', 0x1a, u8
}

ioctl_read_buf! {
	/// get all switch states
	ev_get_sw, b'E', 0x1b, u8
}

/// get event bits
pub unsafe fn ev_get_bit(fd: c_int, ev: u32, buf: &mut [u8]) -> nix::Result<i32> {
	convert_ioctl_res!(ioctl(fd, request_code_read!(b'E', 0x20 + ev, buf.len()) as ioctl_num_type, buf))
}

/// get abs value/limits
pub unsafe fn ev_get_abs(fd: c_int, abs: u32, buf: *mut input_absinfo) -> nix::Result<i32> {
	convert_ioctl_res!(ioctl(fd, request_code_read!(b'E', 0x40 + abs, size_of::<input_absinfo>()) as ioctl_num_type, buf))
}

/// set abs value/limits
pub unsafe fn ev_set_abs(fd: c_int, abs: u32, buf: *const input_absinfo) -> nix::Result<i32> {
	convert_ioctl_res!(ioctl(fd, request_code_read!(b'E', 0x40 + abs, size_of::<input_absinfo>()) as ioctl_num_type, buf))
}

/// send a force effect to a force feedback device
pub unsafe fn ev_send_ff(fd: c_int, buf: *mut ff_effect) -> nix::Result<i32> {
	// for some reason this isn't _IORW?
	convert_ioctl_res!(ioctl(fd, request_code_write!(b'E', 0x80, size_of::<ff_effect>()) as ioctl_num_type, buf))
}

ioctl_write_int! {
	/// Erase a force effect
	ev_erase_ff, b'E', 0x81
}

ioctl_read! {
	/// Report number of effects playable at the same time
	ev_get_effects, b'E', 0x84, c_int
}

ioctl_write_int! {
	/// Grab/Release device
	ev_grab, b'E', 0x90
}

ioctl_write_int! {
	/// Revoke device access
	ev_revoke, b'E', 0x91
}

ioctl_read! {
	/// Retrieve current event mask
	ev_get_mask, b'E', 0x92, input_mask
}

ioctl_write_ptr! {
	/// Set event mask
	ev_set_mask, b'E', 0x93, input_mask
}

ioctl_write_ptr! {
	/// Set clockid to be used for timestamps
	ev_set_clockid, b'E', 0xa0, clockid_t
}
