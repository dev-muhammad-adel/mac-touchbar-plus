use std::error::Error as StdError;
use std::fmt;

/// Type alias for `std::result::Result<T, Error>`
pub type Result<T> = std::result::Result<T, Error>;

/// Error type returned by [`ThemeNameProvider`](crate::ThemeNameProvider).
#[derive(Debug)]
pub enum Error {
    /// Config file could not be found.
    ConfigNotFound,

    /// Error loading config file.
    LoadConfig {
        /// The source for the error.
        source: ini::Error,
    },

    /// Config does not contain valid theme name.
    ConfigMissingThemeName,

    /// Error originating in the `xdg` crate.
    XDG {
        /// The source for the error.
        source: xdg::BaseDirectoriesError,
    },

    /// Wrapper for errors returned by custom [`ThemeNameProvider`](crate::ThemeNameProvider).
    Custom {
        /// The source for the error.
        source: Box<dyn StdError + Send + Sync>,
    },
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::XDG { source } => Some(source),
            Error::LoadConfig { source } => Some(source),
            Error::Custom { source } => Some(source.as_ref()),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ConfigNotFound => write!(f, "Config file could not be found."),
            Error::LoadConfig { source } => write!(f, "Error loading config file: {}", source),
            Error::ConfigMissingThemeName => {
                write!(f, "Config file is missing a valid theme name.")
            }
            Error::XDG { source } => write!(f, "Error loading XDG locations: {}", source),
            Error::Custom { source } => {
                write!(f, "Error in custom theme name provider: {}", source)
            }
        }
    }
}

impl From<ini::Error> for Error {
    fn from(source: ini::Error) -> Self {
        Error::LoadConfig { source }
    }
}

impl From<xdg::BaseDirectoriesError> for Error {
    fn from(source: xdg::BaseDirectoriesError) -> Self {
        Error::XDG { source }
    }
}
