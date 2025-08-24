pub mod error;

use std::{borrow::ToOwned, error::Error as StdError};

use error::{Error, Result};

/// Enum that provides a theme name to [`IconLoader`](crate::IconLoader).
/// It can either load the system theme name from the KDE or GTK config files
/// or provide a fixed string or a theme name yielded by a completely customizable function.
/// The last option allows users to load their own config files for example.
pub enum ThemeNameProvider {
    /// Use the '~/.config/kdeglobals' file to determine the theme name.
    #[cfg(feature = "kde")]
    KDE,

    /// Use the '~/.config/gtk-3.0/settings.ini' file to determine the theme name.
    #[cfg(feature = "gtk")]
    GTK,

    /// A theme name provided by the user.
    User(String),

    /// A custom function that returns a theme name or an error.
    Custom(
        Box<dyn Fn() -> std::result::Result<String, Box<dyn StdError + Send + Sync>> + Send + Sync>,
    ),
}

impl ThemeNameProvider {
    /// Creates a new `ThemeNameProvider` that provides the given string as theme name.
    pub fn user(string: impl Into<String>) -> Self {
        ThemeNameProvider::User(string.into())
    }

    /// Creates a new custom `ThemeNameProvider` from the given function.
    pub fn custom<F, S, E>(f: F) -> Self
    where
        F: Fn() -> std::result::Result<S, E> + Send + Sync + 'static,
        S: Into<String>,
        E: StdError + Send + Sync + 'static,
    {
        ThemeNameProvider::Custom(Box::new(move || f().map(Into::into).map_err(Into::into)))
    }

    pub(crate) fn theme_name(&self) -> Result<String> {
        match self {
            #[cfg(feature = "kde")]
            ThemeNameProvider::KDE => {
                let base_dirs = xdg::BaseDirectories::new()?;

                if base_dirs.find_config_file("kdeglobals").is_none() {
                    return Err(Error::ConfigNotFound);
                }

                for config_path in base_dirs.find_config_files("kdeglobals") {
                    let config = ini::Ini::load_from_file(config_path)?;

                    for (category, properties) in config.iter() {
                        if let Some("Icons") = category {
                            for (key, value) in properties.iter() {
                                if key == "Theme" {
                                    return Ok(value.to_string());
                                }
                            }
                        }
                    }
                }

                Err(Error::ConfigMissingThemeName)
            }

            #[cfg(feature = "gtk")]
            ThemeNameProvider::GTK => {
                let base_dirs = xdg::BaseDirectories::new()?;

                if base_dirs.find_config_file("gtk-3.0/settings.ini").is_none() {
                    return Err(Error::ConfigNotFound);
                }

                for config_path in base_dirs.find_config_files("gtk-3.0/settings.ini") {
                    let config = ini::Ini::load_from_file(config_path)?;

                    for (category, properties) in config.iter() {
                        if let Some("Settings") = category {
                            for (key, value) in properties.iter() {
                                if key == "gtk-icon-theme-name" {
                                    return Ok(value.to_string());
                                }
                            }
                        }
                    }
                }

                Err(Error::ConfigMissingThemeName)
            }

            ThemeNameProvider::User(string) => Ok(string.clone()),
            ThemeNameProvider::Custom(func) => func().map_err(|source| Error::Custom { source }),
        }
    }
}

impl std::fmt::Debug for ThemeNameProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "kde")]
            ThemeNameProvider::KDE => write!(f, "ThemeNameProvider::KDE"),

            #[cfg(feature = "gtk")]
            ThemeNameProvider::GTK => write!(f, "ThemeNameProvider::GTK"),

            ThemeNameProvider::User(string) => write!(f, "ThemeNameProvider::User({})", string),
            ThemeNameProvider::Custom(_) => write!(f, "ThemeNameProvider::Custom"),
        }
    }
}

impl Default for ThemeNameProvider {
    fn default() -> Self {
        ThemeNameProvider::User(String::from("hicolor"))
    }
}

impl PartialEq for ThemeNameProvider {
    fn eq(&self, other: &Self) -> bool {
        use std::mem::discriminant;

        if discriminant(self) != discriminant(other) {
            return false;
        }

        if let ThemeNameProvider::Custom(_) = self {
            return false;
        }

        if let (ThemeNameProvider::User(string), ThemeNameProvider::User(other_string)) =
            (self, other)
        {
            return string == other_string;
        }

        true
    }
}

impl<F, S, E> From<F> for ThemeNameProvider
where
    F: Fn() -> std::result::Result<S, E> + Send + Sync + 'static,
    S: Into<String>,
    E: StdError + Send + Sync + 'static,
{
    fn from(f: F) -> Self {
        ThemeNameProvider::custom(f)
    }
}

impl From<&str> for ThemeNameProvider {
    fn from(string: &str) -> Self {
        ThemeNameProvider::User(string.to_owned())
    }
}

impl From<String> for ThemeNameProvider {
    fn from(string: String) -> Self {
        ThemeNameProvider::User(string)
    }
}
