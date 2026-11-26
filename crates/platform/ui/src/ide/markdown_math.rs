use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use egui::text::{CharIndex, TextFormat};
use egui::{Color32, FontId, Galley, Pos2, Rect, TextureHandle, TextureId, Ui, pos2, vec2};
use zen_workbench::markdown::math::{self, Formula, Metrics};

pub(super) const INLINE_SCALE: f32 = 1.15;
pub(super) const DISPLAY_SCALE: f32 = 1.3;
const PROXY_SIZE: f32 = 4.0;
const KEEP_PASSES: u64 = 600;
const BASELINE_BELOW_CENTRE: f32 = 0.36;
const ROOM: f32 = 2.0;

type Slot = Arc<OnceLock<Result<Formula, String>>>;

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    tex: String,
    display: bool,
    size: u32,
    scale: u32,
    colour: [u8; 3],
}

enum State {
    Loading(Slot),
    Ready(Metrics, TextureHandle),
    Failed(String),
}

struct Entry {
    state: State,
    used: u64,
}

#[derive(Default)]
pub(super) struct Maths {
    entries: Mutex<HashMap<Key, Entry>>,
    pending: Mutex<Vec<(Key, Slot)>>,
}

pub(super) enum Shown {
    Loading,
    Ready(Metrics, TextureId),
    Failed(String),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Mark {
    pub start: usize,
    pub end: usize,
    pub metrics: Metrics,
    pub texture: TextureId,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn texture(ctx: &egui::Context, formula: &Formula) -> TextureHandle {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let picture = &formula.picture;
    let pixels = egui::ColorImage::from_rgba_unmultiplied([picture.width, picture.height], &picture.rgba);
    let name = format!("markdown-math-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    ctx.load_texture(name, pixels, egui::TextureOptions::LINEAR)
}

impl Maths {
    pub(super) fn get(&self, ctx: &egui::Context, tex: &str, display: bool, size: f32, colour: Color32) -> Shown {
        let key = Key {
            tex: tex.to_owned(),
            display,
            size: size.to_bits(),
            scale: ctx.pixels_per_point().to_bits(),
            colour: [colour.r(), colour.g(), colour.b()],
        };
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
                Ok(formula) => State::Ready(formula.metrics, texture(ctx, formula)),
                Err(error) => State::Failed(error.clone()),
            };
        }
        match &entry.state {
            State::Loading(_) => Shown::Loading,
            State::Ready(metrics, texture) => Shown::Ready(*metrics, texture.id()),
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
                let size = f32::from_bits(key.size);
                let _ = slot.set(math::typeset(&key.tex, key.display, size, f32::from_bits(key.scale), key.colour));
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

pub(super) fn copied(tex: &str, display: bool) -> String {
    if display { format!("$$ {tex} $$") } else { format!("${tex}$") }
}

pub(super) fn unbreakable(copy: &str) -> String {
    copy.chars()
        .map(|c| match c {
            c if c.is_whitespace() => '\u{a0}',
            '-' => '\u{2011}',
            c => c,
        })
        .collect()
}

pub(super) fn line_height(metrics: &Metrics, size: f32) -> f32 {
    let below = BASELINE_BELOW_CENTRE * size;
    (size * 1.55).max(2.0 * (metrics.ascent - below + ROOM)).max(2.0 * (metrics.descent + below + ROOM))
}

pub(super) fn proxy(ui: &Ui, shown: &str, width: f32, line_height: f32) -> TextFormat {
    let measure = |size: f32| {
        ui.painter().layout_no_wrap(shown.to_owned(), FontId::monospace(size), Color32::TRANSPARENT).size().x
    };
    let mut size = PROXY_SIZE;
    let mut natural = measure(size);
    if natural > width * 0.5 && natural > 0.0 {
        size = (size * width * 0.5 / natural).max(0.1);
        natural = measure(size);
    }
    let gaps = shown.chars().count().saturating_sub(1).max(1) as f32;
    TextFormat {
        font_id: FontId::monospace(size),
        color: Color32::TRANSPARENT,
        extra_letter_spacing: ((width - natural) / gaps).max(0.0),
        line_height: Some(line_height),
        valign: egui::Align::Center,
        ..TextFormat::default()
    }
}

fn row_of(galley: &Galley, at: usize) -> Option<(usize, usize)> {
    let mut start = 0;
    for (index, row) in galley.rows.iter().enumerate() {
        let count = row.char_count_including_newline().0;
        if at < start + count.max(1) || index + 1 == galley.rows.len() {
            return Some((index, start));
        }
        start += count;
    }
    None
}

pub(super) fn placed(galley: &Galley, origin: Pos2, marks: &[Mark]) -> Vec<(Rect, TextureId)> {
    marks
        .iter()
        .filter_map(|mark| {
            let (index, start) = row_of(galley, mark.start)?;
            let row = &galley.rows[index];
            let x = row.pos.x + row.x_offset(CharIndex(mark.start - start));
            let inside = |glyph: usize| marks.iter().any(|mark| (mark.start..mark.end).contains(&(start + glyph)));
            let baseline = row
                .glyphs
                .iter()
                .enumerate()
                .find(|(glyph, _)| !inside(*glyph))
                .map(|(_, glyph)| row.pos.y + glyph.pos.y)
                .unwrap_or(row.pos.y + (row.size.y - mark.metrics.height()) / 2.0 + mark.metrics.ascent);
            let min = origin + vec2(x, baseline - mark.metrics.ascent);
            Some((Rect::from_min_size(min, vec2(mark.metrics.width, mark.metrics.height())), mark.texture))
        })
        .collect()
}

pub(super) fn paint(ui: &Ui, galley: &Galley, origin: Pos2, marks: &[Mark]) {
    for (rect, texture) in placed(galley, origin, marks) {
        draw(ui, texture, rect);
    }
}

pub(super) fn draw(ui: &Ui, texture: TextureId, rect: Rect) {
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    ui.painter().image(texture, rect, uv, Color32::WHITE);
}
