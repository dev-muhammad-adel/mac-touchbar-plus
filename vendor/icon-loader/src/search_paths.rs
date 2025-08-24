use std::{borrow::Cow, path::PathBuf};

use xdg::BaseDirectories;

/// Enum that provides a list of directories to [`IconLoader`](crate::IconLoader) to search for icons in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SearchPaths {
    /// Uses the `xdg` crate for system icon paths.
    #[default]
    System,

    /// A custom set of paths.
    Custom(Vec<PathBuf>),
}

impl SearchPaths {
    /// Creates a custom `SearchPaths` from a list of directories.
    pub fn custom<I, P>(iter: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        SearchPaths::Custom(iter.into_iter().map(P::into).collect())
    }

    pub(crate) fn paths(&self) -> Cow<[PathBuf]> {
        match self {
            SearchPaths::System => Cow::Owned(BaseDirectories::with_prefix("icons").map_or_else(
                |_| vec![PathBuf::from("/usr/share/icons")],
                |bd| bd.get_data_dirs(),
            )),
            SearchPaths::Custom(dirs) => Cow::Borrowed(dirs),
        }
    }
}

impl<I, P> From<I> for SearchPaths
where
    I: IntoIterator<Item = P>,
    P: Into<PathBuf>,
{
    fn from(iter: I) -> Self {
        SearchPaths::custom(iter)
    }
}
