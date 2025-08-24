use crate::{ff_effect, input_absinfo, input_id, ABS_CNT};
use libc::{c_char, c_int, c_uint};
use nix::{
	ioctl_none,
	ioctl_read, ioctl_read_buf,
	ioctl_readwrite,
	ioctl_write_int, ioctl_write_ptr,
};

pub const UINPUT_MAX_NAME_SIZE: c_int = 80;

pub const UINPUT_VERSION: c_int = 5;

/// This is the new event type, used only by uinput.
/// 'code' is UI_FF_UPLOAD or UI_FF_ERASE, and 'value' is the unique request ID.
pub const EV_UINPUT: c_int = 0x0101;
pub const UI_FF_UPLOAD: c_int = 1;
pub const UI_FF_ERASE: c_int = 2;

#[repr(C)]
pub struct uinput_setup {
	pub id: input_id,
	pub name: [c_char; UINPUT_MAX_NAME_SIZE as usize],
	pub ff_effects_max: u32,
}

#[repr(C)]
pub struct uinput_abs_setup {
	pub code: u16,
	pub absinfo: input_absinfo,
}

#[repr(C)]
pub struct uinput_user_dev {
	pub name: [c_char; UINPUT_MAX_NAME_SIZE as usize],
	pub id: input_id,

	pub ff_effects_max: u32,
	pub absmax: [i32; ABS_CNT as usize],
	pub absmin: [i32; ABS_CNT as usize],
	pub absfuzz: [i32; ABS_CNT as usize],
	pub absflat: [i32; ABS_CNT as usize],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct uinput_ff_upload {
	pub request_id: u32,
	pub retval: i32,
	pub effect: ff_effect,
	pub old: ff_effect,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct uinput_ff_erase {
	pub request_id: u32,
	pub retval: i32,
	pub effect_id: u32,
}

ioctl_none!(ui_dev_create, b'U', 1);
ioctl_none!(ui_dev_destroy, b'U', 2);

ioctl_write_ptr! {
	/// Set device parameters for setup
	ui_dev_setup, b'U', 3, uinput_setup
}

ioctl_write_ptr! {
	/// Set absolute axis information for the device to setup
	ui_abs_setup, b'U', 4, uinput_abs_setup
}

ioctl_write_int!(ui_set_evbit, b'U', 100);
ioctl_write_int!(ui_set_keybit, b'U', 101);
ioctl_write_int!(ui_set_relbit, b'U', 102);
ioctl_write_int!(ui_set_absbit, b'U', 103);
ioctl_write_int!(ui_set_mscbit, b'U', 104);
ioctl_write_int!(ui_set_ledbit, b'U', 105);
ioctl_write_int!(ui_set_sndbit, b'U', 106);
ioctl_write_int!(ui_set_ffbit, b'U', 107);
ioctl_write_ptr!(ui_set_phys, b'U', 108, c_char);
ioctl_write_int!(ui_set_swbit, b'U', 109);
ioctl_write_int!(ui_set_propbit, b'U', 110);

ioctl_readwrite!(ui_begin_ff_upload, b'U', 200, uinput_ff_upload);
ioctl_write_ptr!(ui_end_ff_upload, b'U', 201, uinput_ff_upload);

ioctl_readwrite!(ui_begin_ff_erase, b'U', 202, uinput_ff_erase);
ioctl_write_ptr!(ui_end_ff_erase, b'U', 203, uinput_ff_erase);

ioctl_read_buf! {
	/// get the sysfs name of the created uinput device
	ui_get_sysname, b'U', 44, c_char
}

ioctl_read! {
	/// Return version of uinput protocol
	ui_get_version, b'U', 45, c_uint
}
