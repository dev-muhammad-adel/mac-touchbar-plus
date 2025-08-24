use std::error::Error as StdError;
use std::fmt;

pub use crate::theme_name_provider::error::Error as ProviderError;

/// Type alias for `std::result::Result<T, Error>`
pub type Result<T> = std::result::Result<T, Error>;

/// Error type returned by this crate.
#[derive(Debug)]
pub enum Error {
    /// No theme with the given name could be found.
    ThemeNotFound {
        /// The given theme name.
        theme_name: String,
    },

    /// Error updating the default theme name.
    ThemeNameProvider {
        /// The source for the error.
        source: ProviderError,
    },
}

impl Error {
    pub(crate) fn theme_not_found(theme_name: impl Into<String>) -> Self {
        Error::ThemeNotFound {
            theme_name: theme_name.into(),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::ThemeNameProvider { source } => Some(source),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ThemeNotFound { theme_name } => {
                write!(f, "Theme with name {} not found", theme_name)
            }
            Error::ThemeNameProvider { source } => {
                write!(f, "Error updating default theme name: {}", source)
            }
        }
    }
}

impl From<ProviderError> for Error {
    fn from(source: ProviderError) -> Self {
        Error::ThemeNameProvider { source }
    }
}
