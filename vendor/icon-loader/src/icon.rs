mod icon_dir;
mod icon_file;
mod icon_theme;

pub use icon_dir::{IconDir, IconSizeType};
pub use icon_file::{IconFile, IconFileType};
pub use icon_theme::error::{Error, Result};

pub(crate) use icon_theme::IconThemes;

/// Struct containing information about a themed icon.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Icon {
    icon_name: String,
    theme_name: String,
    files: Vec<IconFile>,
}

impl Icon {
    /// Returns the associated icon's name.
    pub fn icon_name(&self) -> &str {
        &self.icon_name
    }

    /// Returns the associated icon's theme name.
    pub fn theme_name(&self) -> &str {
        &self.theme_name
    }

    /// Returns the icon files found for the associated icon.
    pub fn files(&self) -> &[IconFile] {
        &self.files
    }

    /// Returns the file of the associated icon that fits the given size best and has a scale of 1.
    /// If there is no exact fit available, the next bigger one is chosen.
    /// If there is no bigger one, the next smaller one is returned.
    /// If that cannot be found, the scale restriction is ignored.
    ///
    /// # Arguments
    ///
    /// * `size` - The ideal size of the returned icon file.
    ///
    ///# Example
    ///
    /// ```
    /// use std::ops::Deref;
    /// use icon_loader::IconLoader;
    ///
    /// let loader = IconLoader::new();
    /// if let Some(icon) = loader.load_icon("minimum") {
    ///     let icon_file = icon.file_for_size(32);
    /// };
    /// ```
    pub fn file_for_size(&self, size: u16) -> &IconFile {
        self.file_for_size_scaled(size, 1)
    }

    /// Returns the file of the associated icon that fits the given size and scale best.
    /// If there is no exact fit available, the next bigger size is chosen.
    /// If there is no bigger fit with the given scale, the next smaller one is returned.
    /// If no file with the preferred scale can be found, one with the size `size * scale` and scale 1 is looked for.
    /// If that cannot be found, the scale restriction is ignored.
    ///
    /// # Arguments
    ///
    /// * `size` - The ideal size of the returned icon file.
    /// * `scale` - The preferred scale of the returned icon file.
    ///
    ///# Example
    ///
    /// ```
    /// use std::ops::Deref;
    /// use icon_loader::IconLoader;
    ///
    /// let loader = IconLoader::new();
    /// if let Some(icon) = loader.load_icon("minimum") {
    ///     let icon_file = icon.file_for_size_scaled(32, 2);
    /// };
    /// ```
    pub fn file_for_size_scaled(&self, size: u16, scale: u16) -> &IconFile {
        if let Some(file) = self.file_for_size_filtered(size, |file| file.scale() == scale) {
            return file;
        }

        if let Some(file) = self.file_for_size_filtered(size * scale, |file| file.scale() == 1) {
            return file;
        }

        // If we don't filter, there is always at least one file on disk.
        self.file_for_size_filtered(size, |_| true).unwrap()
    }

    /// Returns the file of the associated icon that fits the given size best and matches the provided filter.
    /// If there is no exact fit available, the next bigger one is chosen.
    /// If there is no bigger one, the next smaller one is returned.
    /// Use this, if you want only files of type PNG or anything like that.
    ///
    /// # Arguments
    ///
    /// * `size` - The ideal size of the returned icon file.
    /// * `filter` - A function that takes a reference to an [`IconFile`](icon::IconFile) and returns true, if it passes the test and false otherwise.
    ///
    /// # Example
    ///
    /// ```
    /// use std::ops::Deref;
    /// use icon_loader::{IconLoader, IconFileType};
    ///
    /// let loader = IconLoader::new();
    /// if let Some(icon) = loader.load_icon("minimum") {
    ///     let icon_file = icon.file_for_size_filtered(32, |file| file.icon_type() == IconFileType::PNG);
    /// };
    /// ```
    pub fn file_for_size_filtered(
        &self,
        size: u16,
        filter: impl Fn(&IconFile) -> bool,
    ) -> Option<&IconFile> {
        let files = self.files.iter().filter(|&file| filter(file));

        // Try to return an exact fit.
        if let Some(icon_file) = files.clone().find(|file| file.dir_info().size() == size) {
            return Some(icon_file);
        }

        // Try to return a threshold fit.
        if let Some(icon_file) = files
            .clone()
            .filter(|file| file.dir_info().size_type() == IconSizeType::Threshold)
            .find(|file| {
                size >= file.dir_info().size() - file.dir_info().threshold()
                    && size <= file.dir_info().size() + file.dir_info().threshold()
            })
        {
            return Some(icon_file);
        }

        // Try to return a slightly bigger fit.
        if let Some(icon_file) = files
            .clone()
            .filter(|file| file.dir_info().size() > size)
            .min_by_key(|file| file.dir_info().size())
        {
            return Some(icon_file);
        }

        // Return the biggest available.
        files.max_by_key(|file| file.dir_info().size())
    }

    pub(crate) fn new(icon_name: String, theme_name: String, files: Vec<IconFile>) -> Option<Self> {
        if icon_name.is_empty() || theme_name.is_empty() || files.is_empty() {
            None
        } else {
            Some(Self {
                files,
                icon_name,
                theme_name,
            })
        }
    }
}
