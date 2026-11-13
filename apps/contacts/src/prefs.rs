pub use nyx_theme::ThemeMode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub mode: ThemeMode,
    pub theme: String,
    pub glow: bool,
    pub reduce_motion: bool,
    pub sidebar_collapsed: bool,
    pub last_section: String,
    pub last_first: bool,
    pub sort_last: bool,
    /// Pairs marked "Not the same person" in Duplicates (`id|id`, sorted).
    pub not_duplicates: Vec<String>,
    /// Each list's colour, by list id: an index into the sidebar's swatches.
    pub list_colors: HashMap<String, usize>,
    /// Sources hidden in Settings › Sources: `local`, or an account book's id.
    pub hidden_sources: Vec<String>,
    /// Names you gave account books, by book id. Kept here: a sync rewrites the book's own.
    pub source_names: HashMap<String, String>,
}
impl Default for Prefs {
    fn default() -> Self {
        Self {
            mode: ThemeMode::Omarchy,
            theme: "tokyo-night".into(),
            glow: true,
            reduce_motion: false,
            sidebar_collapsed: false,
            last_section: "all".into(),
            last_first: false,
            sort_last: false,
            not_duplicates: Vec::new(),
            list_colors: HashMap::new(),
            hidden_sources: Vec::new(),
            source_names: HashMap::new(),
        }
    }
}
nyx_core::prefs_store!(Prefs);
