mod color;
pub mod format;
pub mod import;
pub mod jsonc;
mod keys;
mod readable;
pub mod store;
mod theme;
mod zen_dark;
mod zen_light;

pub use color::{ParseColorError, Rgba, Style, Underline};
pub use format::{FormatError, parse, to_json};
pub use import::vscode::VsCodeImporter;
pub use import::{ImportError, ImportReport, ThemeImporter};
pub use keys::{EditorKey, Key, SyntaxKey, TerminalKey, UiKey};
pub use readable::{PAIRS, Pair, Slot, Unreadable, make_readable, unreadable};
pub use store::{Appearance, Imported, StoreError, ThemeEntry, ThemeFiles, ThemeSource, ThemeStore};
pub use theme::{MAX_DEPTH, Resolution, ResolvedTheme, Theme, Warning};
pub use zen_dark::{
    BUILTIN_IDS, ZEN_DARK_EDITOR, ZEN_DARK_ID, ZEN_DARK_SYNTAX, ZEN_DARK_TERMINAL, ZEN_DARK_UI, base_id, builtin,
    zen_dark,
};
pub use zen_light::{ZEN_LIGHT_EDITOR, ZEN_LIGHT_ID, ZEN_LIGHT_SYNTAX, ZEN_LIGHT_TERMINAL, ZEN_LIGHT_UI, zen_light};
