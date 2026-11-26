mod color;
mod icls;
pub mod idea;
mod keymap;
mod theme_json;

pub use icls::IclsImporter;
pub use idea::IdeaProject;
pub use keymap::{JetBrainsKeymapImporter, KEYMAP_ACTIONS, MACOS_PARENT};
pub use theme_json::ThemeJsonImporter;
use zen_core::keymap::KeymapImporter;
use zen_theme::ThemeImporter;

pub fn importers() -> Vec<Box<dyn ThemeImporter>> {
    vec![Box::new(IclsImporter), Box::new(ThemeJsonImporter)]
}

pub fn keymap_importers() -> Vec<Box<dyn KeymapImporter>> {
    vec![Box::new(JetBrainsKeymapImporter)]
}
