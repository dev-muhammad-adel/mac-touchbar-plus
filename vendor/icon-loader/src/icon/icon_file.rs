use super::{IconDir, IconSizeType};

use std::{
    borrow::Borrow,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Enum representing the different file types an icon can be.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum IconFileType {
    /// PNG file type
    PNG,

    /// SVG file type
    SVG,

    /// XPM file type
    XPM,
}

impl IconFileType {
    pub(crate) const fn types() -> &'static [IconFileType; 3] {
        const TYPES: [IconFileType; 3] = [IconFileType::PNG, IconFileType::SVG, IconFileType::XPM];

        &TYPES
    }
}

impl AsRef<str> for IconFileType {
    fn as_ref(&self) -> &'static str {
        match self {
            IconFileType::PNG => "png",
            IconFileType::SVG => "svg",
            IconFileType::XPM => "xpm",
        }
    }
}

/// Struct containing information about a single icon file on disk.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct IconFile {
    dir_info: Arc<IconDir>,
    path: PathBuf,
    icon_type: IconFileType,
}

impl IconFile {
    /// Returns information about the directory the icon file lives in.
    pub fn dir_info(&self) -> &IconDir {
        &self.dir_info
    }

    /// Returns this icon's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns this icon's type.
    pub const fn icon_type(&self) -> IconFileType {
        self.icon_type
    }

    /// Returns this icon's size.
    pub fn size(&self) -> u16 {
        self.dir_info.size()
    }

    /// Returns this icon's scale.
    pub fn scale(&self) -> u16 {
        self.dir_info.scale()
    }

    /// Returns this icon's context.
    pub fn context(&self) -> Option<&str> {
        self.dir_info.context()
    }

    /// Returns this icon's type.
    pub fn size_type(&self) -> IconSizeType {
        self.dir_info.size_type()
    }

    /// Returns this icon's max size.
    pub fn max_size(&self) -> u16 {
        self.dir_info.max_size()
    }

    /// Returns this icon's min size.
    pub fn min_size(&self) -> u16 {
        self.dir_info.min_size()
    }

    /// Returns this icon's size threshold.
    pub fn threshold(&self) -> u16 {
        self.dir_info.threshold()
    }

    pub(crate) const fn new(
        dir_info: Arc<IconDir>,
        path: PathBuf,
        icon_type: IconFileType,
    ) -> Self {
        Self {
            dir_info,
            path,
            icon_type,
        }
    }
}

impl AsRef<Path> for IconFile {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}

impl Borrow<Path> for IconFile {
    fn borrow(&self) -> &Path {
        self.path()
    }
}
