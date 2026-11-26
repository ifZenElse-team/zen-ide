use std::path::PathBuf;

use zen_workbench::file_clipboard::SystemClipboard;

pub(crate) struct ArboardClipboard;

impl ArboardClipboard {
    pub(crate) fn wanted() -> bool {
        !cfg!(test) && std::env::var("ZEN_CLIPBOARD").map_or(true, |value| value != "memory")
    }
}

impl SystemClipboard for ArboardClipboard {
    fn files(&self) -> Vec<PathBuf> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get().file_list()).unwrap_or_default()
        }
        #[cfg(target_arch = "wasm32")]
        {
            Vec::new()
        }
    }

    fn set_files(&self, files: &[PathBuf]) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set().file_list(files)).is_ok()
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = files;
            false
        }
    }

    fn set_text(&self, text: &str) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)).is_ok()
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = text;
            false
        }
    }

    fn text(&self) -> String {
        #[cfg(not(target_arch = "wasm32"))]
        {
            arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()).unwrap_or_default()
        }
        #[cfg(target_arch = "wasm32")]
        {
            String::new()
        }
    }
}
