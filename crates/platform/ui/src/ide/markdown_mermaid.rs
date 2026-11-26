use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use egui::{Color32, TextureHandle, TextureId, vec2};
use zen_image::SvgFonts;
use zen_workbench::markdown::mermaid::{self, Diagram, Palette};

use super::IdePalette;

const KEEP_PASSES: u64 = 600;
const FAMILY: &str = "Inter";

type Slot = Arc<OnceLock<Result<Diagram, String>>>;

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    source: String,
    palette: Palette,
    scale: u32,
}

enum State {
    Loading(Slot),
    Ready(egui::Vec2, TextureHandle),
    Failed(String),
}

struct Entry {
    state: State,
    used: u64,
}

#[derive(Default)]
pub(super) struct Mermaids {
    entries: Mutex<HashMap<Key, Entry>>,
    pending: Mutex<Vec<(Key, Slot)>>,
}

pub(super) enum Shown {
    Loading,
    Ready(egui::Vec2, TextureId),
    Failed(String),
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn rgb(colour: Color32) -> [u8; 3] {
    [colour.r(), colour.g(), colour.b()]
}

fn palette_of(p: &IdePalette) -> Palette {
    Palette {
        background: rgb(p.editor_background),
        text: rgb(p.text),
        muted: rgb(p.text_secondary),
        node: rgb(p.sticky_background),
        border: rgb(p.accent),
        panel: rgb(p.current_line),
    }
}

fn fonts() -> &'static SvgFonts {
    static FONTS: OnceLock<SvgFonts> = OnceLock::new();
    FONTS.get_or_init(|| SvgFonts::new(&[crate::fonts::INTER_REGULAR, crate::fonts::INTER_BOLD], FAMILY))
}

fn texture(ctx: &egui::Context, diagram: &Diagram) -> TextureHandle {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let picture = &diagram.picture;
    let pixels = egui::ColorImage::from_rgba_unmultiplied([picture.width, picture.height], &picture.rgba);
    let name = format!("markdown-mermaid-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    ctx.load_texture(name, pixels, egui::TextureOptions::LINEAR)
}

impl Mermaids {
    pub(super) fn get(&self, ctx: &egui::Context, source: &str, p: &IdePalette) -> Shown {
        let key = Key { source: source.to_owned(), palette: palette_of(p), scale: ctx.pixels_per_point().to_bits() };
        let pass = ctx.cumulative_pass_nr();
        let mut entries = lock(&self.entries);
        let entry = entries.entry(key).or_insert_with_key(|key| {
            let slot = Slot::default();
            lock(&self.pending).push((key.clone(), Arc::clone(&slot)));
            Entry { state: State::Loading(slot), used: pass }
        });
        entry.used = pass;
        if let State::Loading(slot) = &entry.state
            && let Some(result) = slot.get()
        {
            entry.state = match result {
                Ok(diagram) => State::Ready(vec2(diagram.width, diagram.height), texture(ctx, diagram)),
                Err(error) => State::Failed(error.clone()),
            };
        }
        match &entry.state {
            State::Loading(_) => Shown::Loading,
            State::Ready(size, texture) => Shown::Ready(*size, texture.id()),
            State::Failed(error) => Shown::Failed(error.clone()),
        }
    }

    pub(super) fn start(&self, ctx: &egui::Context) {
        let pass = ctx.cumulative_pass_nr();
        lock(&self.entries).retain(|_, entry| pass.saturating_sub(entry.used) < KEEP_PASSES);
        let jobs = std::mem::take(&mut *lock(&self.pending));
        if jobs.is_empty() {
            return;
        }
        let ctx = ctx.clone();
        let run = move || {
            for (key, slot) in jobs {
                let drawn = mermaid::draw(&key.source, &key.palette, fonts(), f32::from_bits(key.scale));
                let _ = slot.set(drawn);
                ctx.request_repaint();
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(run);
        #[cfg(target_arch = "wasm32")]
        run();
    }

    #[cfg(test)]
    pub(super) fn loading(&self) -> bool {
        lock(&self.entries).values().any(|entry| matches!(entry.state, State::Loading(_)))
    }
}
