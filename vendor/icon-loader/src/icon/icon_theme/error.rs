use ini::Error as IniError;

use std::{
    error::Error as StdError,
    fmt::{Display, Formatter, Result as FmtResult},
    path::PathBuf,
};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    NotDirectory(PathBuf),
    IndexThemeNotFound(PathBuf),
    KeyListEmpty(PathBuf),
    Ini(IniError),
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Ini(e) => Some(e),
            _ => None,
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Error::NotDirectory(path) => write!(f, "{} is not a directory.", path.display()),
            Error::IndexThemeNotFound(path) => {
                write!(f, "File {} could not be found.", path.display())
            }
            Error::KeyListEmpty(path) => write!(
                f,
                "Icon theme with path {} has no valid key entries.",
                path.display()
            ),
            Error::Ini(e) => write!(f, "Error reading 'theme.index' file: {}", e),
        }
    }
}

impl From<IniError> for Error {
    fn from(source: IniError) -> Self {
        Error::Ini(source)
    }
}
