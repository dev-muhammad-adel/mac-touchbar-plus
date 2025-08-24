use std::{borrow::Cow, ops::Deref, path::PathBuf, sync::Arc};

use crate::{
    error::{Error, Result},
    icon::{Icon, IconThemes},
    search_paths::SearchPaths,
    theme_name_provider::ThemeNameProvider,
};

use dashmap::DashMap;

/// The central icon loader struct.
///
/// It lets you load and cache named theme icons from system themes as well as custom themes.
#[derive(Debug)]
pub struct IconLoader {
    theme_name: String,
    fallback_theme_name: String,
    theme_cache: DashMap<String, IconThemes>,
    icon_cache: DashMap<String, Option<Arc<Icon>>>,
    search_paths: SearchPaths,
    theme_name_provider: ThemeNameProvider,
}

impl IconLoader {
    /// Creates a new `IconLoader` with default settings.
    pub fn new() -> Self {
        IconLoader {
            theme_name: String::from("hicolor"),
            fallback_theme_name: String::from("hicolor"),
            theme_name_provider: Default::default(),
            theme_cache: Default::default(),
            icon_cache: Default::default(),
            search_paths: Default::default(),
        }
    }

    /// Creates a new KDE `IconLoader`.
    /// This is a convenience function.
    #[cfg(feature = "kde")]
    pub fn new_kde() -> Result<Self> {
        let mut loader = Self::new();
        loader.set_theme_name_provider(ThemeNameProvider::KDE);
        loader.update_theme_name()?;

        Ok(loader)
    }

    /// Creates a new GTK `IconLoader`.
    /// This is a convenience function.
    #[cfg(feature = "gtk")]
    pub fn new_gtk() -> Result<Self> {
        let mut loader = Self::new();
        loader.set_theme_name_provider(ThemeNameProvider::GTK);
        loader.update_theme_name()?;

        Ok(loader)
    }

    /// Loads the icon with the name `icon_name` from the current icon theme.
    /// If the icon cannot be found, it will be looked for in the fallback icon theme.
    /// If it cannot be found in the fallback theme, `None` is returned.
    pub fn load_icon(&self, icon_name: impl AsRef<str>) -> Option<Arc<Icon>> {
        let icon_name = icon_name.as_ref();

        if !self.icon_cache.contains_key(icon_name) {
            let mut searched_themes = Vec::new();

            let icon = self
                .find_icon(self.theme_name(), icon_name, &mut searched_themes)
                .or_else(|| {
                    self.find_icon(self.fallback_theme_name(), icon_name, &mut searched_themes)
                })
                .map(Arc::new);

            self.icon_cache.insert(icon_name.into(), icon);
        }

        // Unwrapping is ok, since we just added a value
        self.icon_cache.get(icon_name).unwrap().value().clone()
    }

    /// Returns the currently used theme name.
    ///
    /// See also [`IconLoader::update_theme_name()`].
    pub fn theme_name(&self) -> &str {
        &self.theme_name
    }

    /// Returns the currently used fallback theme name.
    ///
    /// See also [`IconLoader::set_fallback_theme_name()`].
    pub fn fallback_theme_name(&self) -> &str {
        &self.fallback_theme_name
    }

    /// Returns the paths that are searched for icon themes.
    pub fn search_paths(&self) -> Cow<[PathBuf]> {
        self.search_paths.paths()
    }

    /// Sets the paths where to search for icon themes.
    ///
    /// # Arguments
    ///
    /// * `search_paths` - The paths where to look for icon themes.
    /// Anything that implements `IntoIterator<Item = Into<PathBuf>>` can be used.
    ///
    /// # Examples
    ///
    /// Custom search paths:
    /// ```
    /// use icon_loader::IconLoader;
    ///
    /// let mut loader = IconLoader::new();
    /// loader.set_search_paths(&["/path/to/icon/themes", "/other/path/to/icon/themes"]);
    /// ```
    ///
    /// System search paths:
    /// ```
    /// use icon_loader::{IconLoader, SearchPaths};
    ///
    /// let mut loader = IconLoader::new();
    /// loader.set_search_paths(SearchPaths::System);
    /// ```
    ///
    /// By default these are the system icon paths.
    pub fn set_search_paths(&mut self, search_paths: impl Into<SearchPaths>) {
        let search_paths = search_paths.into();

        if self.search_paths == search_paths {
            return;
        }

        self.search_paths = search_paths;
        self.theme_cache.clear();
        self.icon_cache.clear();
    }

    /// Sets the way in which the used theme name is determined.
    ///
    /// # Arguments
    ///
    /// * `theme_name_provider` - The provider of the default icon theme name.
    /// Anything that implements `Into<ThemeNameProvider>` can be used.
    ///
    /// # Examples
    ///
    /// User defined theme name:
    /// ```
    /// use icon_loader::IconLoader;
    ///
    /// let mut loader = IconLoader::new();
    /// loader.set_theme_name_provider("theme_name");
    /// ```
    ///
    /// KDE system theme:
    /// ```
    /// use icon_loader::{IconLoader, ThemeNameProvider};
    ///
    /// let mut loader = IconLoader::new();
    /// loader.set_theme_name_provider(ThemeNameProvider::KDE);
    /// ```
    ///
    /// [`IconLoader::update_theme_name()`] needs to be called after setting a new theme name provider.
    pub fn set_theme_name_provider(&mut self, theme_name_provider: impl Into<ThemeNameProvider>) {
        let theme_name_provider = theme_name_provider.into();

        if self.theme_name_provider == theme_name_provider {
            return;
        }

        self.theme_name_provider = theme_name_provider;
    }

    /// Queries the set [`ThemeNameProvider`](ThemeNameProvider) for the theme name to be used.
    /// Returns an error, if the set [`ThemeNameProvider`](ThemeNameProvider) returns an error or the theme with the returned name cannot be found.
    ///
    /// Set a theme name provider with [`IconLoader::set_theme_name_provider()`].
    pub fn update_theme_name(&mut self) -> Result<()> {
        let theme_name = self.theme_name_provider.theme_name()?;

        if self.theme_name == theme_name {
            return Ok(());
        }

        if self.theme_exists(&theme_name) {
            self.theme_name = theme_name;
            self.icon_cache.clear();

            Ok(())
        } else {
            Err(Error::theme_not_found(theme_name))
        }
    }

    /// Sets a new fallback theme name. If an icon cannot be found in the set theme,
    /// it will be looked for in the fallback theme.
    /// The default fallback theme name is 'hicolor'.
    pub fn set_fallback_theme_name(&mut self, fallback_theme_name: impl Into<String>) {
        let fallback_theme_name = fallback_theme_name.into();

        if self.fallback_theme_name == fallback_theme_name {
            return;
        }

        self.fallback_theme_name = fallback_theme_name;
        self.icon_cache.clear();
    }

    /// Returns whether a theme with the name `theme_name` exists in the current search paths.
    pub fn theme_exists(&self, theme_name: impl AsRef<str>) -> bool {
        let theme_name = theme_name.as_ref();

        if theme_name.is_empty() {
            return false;
        }

        !self.load_themes(theme_name).is_empty()
    }

    fn load_themes<'a>(&'a self, theme_name: &'a str) -> impl Deref<Target = IconThemes> + 'a {
        if !self.theme_cache.contains_key(theme_name) {
            let new_themes = IconThemes::find(theme_name, &self.search_paths());

            self.theme_cache.insert(theme_name.into(), new_themes);
        }

        // Unwrapping is ok, since we just added a value
        self.theme_cache.get(theme_name).unwrap()
    }

    fn find_icon(
        &self,
        theme_name: &str,
        icon_name: &str,
        searched_themes: &mut Vec<String>,
    ) -> Option<Icon> {
        if theme_name.is_empty() || icon_name.is_empty() {
            return None;
        }

        searched_themes.push(theme_name.into());

        let themes = self.load_themes(theme_name);

        if let Some(icon) = themes.find_icon(icon_name) {
            return Some(icon);
        }

        for parent in themes.parents() {
            if !searched_themes.contains(parent) {
                if let Some(icon) = self.find_icon(parent, icon_name, searched_themes) {
                    return Some(icon);
                }
            }
        }

        None
    }
}

impl Default for IconLoader {
    fn default() -> Self {
        IconLoader::new()
    }
}
