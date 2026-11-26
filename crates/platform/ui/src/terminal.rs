use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use egui::{Color32, FontFamily, FontId, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};
use zen_i18n::t;
use zen_terminal::keys::{self, Key, Modifiers};
use zen_terminal::links::{self, LinkKind, ScreenLink};
use zen_terminal::{Cell, Color, GridPoint, Options, Screen, SelectionKind, Session, Size};
use zen_theme::{ResolvedTheme, TerminalKey};

use crate::theme;
use crate::theme_bridge::color;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalPalette {
    pub background: Color32,
    pub foreground: Color32,
    pub cursor: Color32,
    pub selection: Color32,
    pub ansi: [Color32; 16],
}

impl TerminalPalette {
    pub fn from_theme(theme: &ResolvedTheme) -> TerminalPalette {
        TerminalPalette {
            background: color(theme.terminal(TerminalKey::Background)),
            foreground: color(theme.terminal(TerminalKey::Foreground)),
            cursor: color(theme.terminal(TerminalKey::Cursor)),
            selection: color(theme.terminal(TerminalKey::Selection)),
            ansi: theme.ansi().map(color),
        }
    }

    pub fn zen_dark() -> TerminalPalette {
        TerminalPalette::from_theme(&ResolvedTheme::zen_dark())
    }

    pub fn resolve(&self, color: Color, default: Color32) -> Color32 {
        match color {
            Color::Default => default,
            Color::Indexed(index) => match Color::xterm_rgb(index) {
                Some((r, g, b)) => Color32::from_rgb(r, g, b),
                None => self.ansi[usize::from(index)],
            },
            Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        }
    }
}

const FONT_SIZE: f32 = 13.0;

const PADDING: f32 = 6.0;

pub struct TerminalView {
    session: Session,
    repaint: Arc<OnceLock<egui::Context>>,
    pane: PaneView,
    palette: TerminalPalette,
}

impl std::fmt::Debug for TerminalView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalView").field("session", &self.session).finish_non_exhaustive()
    }
}

impl TerminalView {
    pub fn new(cwd: &Path) -> Result<Self, String> {
        Self::with_options(Options::shell(cwd))
    }

    pub fn with_options(options: Options) -> Result<Self, String> {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let repaint: Arc<OnceLock<egui::Context>> = Arc::new(OnceLock::new());
        let waker = Arc::clone(&repaint);
        let session = Session::spawn(options, move || {
            if let Some(ctx) = waker.get() {
                ctx.request_repaint();
            }
        })
        .map_err(|error| match error {
            zen_terminal::Error::Spawn(cause) => t!("terminal-start-failed", error = cause.to_string()),
            zen_terminal::Error::Unsupported => t!("terminal-unsupported"),
            zen_terminal::Error::Incompatible { .. } => t!("terminal-start-failed", error = error.to_string()),
        })?;
        let serial = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(TerminalView {
            session,
            repaint,
            pane: PaneView { focus_requested: true, ..PaneView::new(egui::Id::unique(("zen-terminal-view", serial))) },
            palette: TerminalPalette::zen_dark(),
        })
    }

    pub fn title(&self) -> String {
        self.session.title().filter(|title| !title.trim().is_empty()).unwrap_or_else(|| t!("terminal-tab-local"))
    }

    pub fn has_exited(&self) -> bool {
        self.session.has_exited()
    }

    pub fn set_palette(&mut self, palette: TerminalPalette) {
        self.palette = palette;
    }

    pub fn id(&self) -> egui::Id {
        self.pane.id()
    }

    pub fn request_focus(&mut self) {
        self.pane.request_focus();
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn show(&mut self, ui: &mut Ui) -> Response {
        let ctx = ui.ctx().clone();
        let _ = self.repaint.get_or_init(|| ctx.clone());
        let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
        let response = self.pane.show(ui, rect, &self.session, &self.palette);
        if self.pane.take_copy()
            && let Some(text) = self.session.selection_text()
        {
            ctx.copy_text(text);
        }
        response
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkClick {
    pub kind: LinkKind,
    pub text: String,
    pub target: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub at: Pos2,
}

impl LinkClick {
    pub fn command_target(&self) -> String {
        match (self.kind, self.line, self.column) {
            (LinkKind::Url, ..) | (LinkKind::File, None, _) => self.target.clone(),
            (LinkKind::File, Some(line), None) => format!("{}:{line}", self.target),
            (LinkKind::File, Some(line), Some(column)) => format!("{}:{line}:{column}", self.target),
        }
    }
}

const KNOWN_PATHS: usize = 256;

#[derive(Debug)]
pub struct PaneView {
    id: egui::Id,
    scroll_remainder: f32,
    focus_requested: bool,
    link_bases: Vec<PathBuf>,
    known_paths: HashMap<String, Option<PathBuf>>,
    hovered: Option<ScreenLink>,
    clicked: Option<LinkClick>,
    grid: Option<(Pos2, Vec2)>,
    copy_requested: bool,
}

impl PaneView {
    pub fn new(id: egui::Id) -> Self {
        PaneView {
            id,
            scroll_remainder: 0.0,
            focus_requested: false,
            link_bases: Vec::new(),
            known_paths: HashMap::new(),
            hovered: None,
            clicked: None,
            grid: None,
            copy_requested: false,
        }
    }

    pub fn cell_center(&self, row: usize, col: usize) -> Option<Pos2> {
        let (origin, cell) = self.grid?;
        Some(origin + Vec2::new((col as f32 + 0.5) * cell.x, (row as f32 + 0.5) * cell.y))
    }

    pub fn set_link_bases(&mut self, bases: Vec<PathBuf>) {
        if bases != self.link_bases {
            self.link_bases = bases;
            self.known_paths.clear();
        }
    }

    pub fn hovered_link(&self) -> Option<&ScreenLink> {
        self.hovered.as_ref()
    }

    pub fn take_copy(&mut self) -> bool {
        std::mem::take(&mut self.copy_requested)
    }

    pub fn take_link_click(&mut self) -> Option<LinkClick> {
        self.clicked.take()
    }

    fn resolve(&mut self, target: &str) -> Option<PathBuf> {
        if let Some(known) = self.known_paths.get(target) {
            return known.clone();
        }
        if self.known_paths.len() >= KNOWN_PATHS {
            self.known_paths.clear();
        }
        let found = links::resolve(target, &self.link_bases);
        self.known_paths.insert(target.to_owned(), found.clone());
        found
    }

    fn link_under(&mut self, screen: &Screen, origin: Pos2, cell: Vec2, pointer: Pos2) -> Option<(ScreenLink, String)> {
        let offset = pointer - origin;
        if offset.x < 0.0 || offset.y < 0.0 {
            return None;
        }
        let (row, col) = ((offset.y / cell.y) as usize, (offset.x / cell.x) as usize);
        let found = screen.link_at(row, col)?;
        let target = match found.link.kind {
            LinkKind::Url => found.link.target.clone(),
            LinkKind::File => self.resolve(&found.link.target)?.display().to_string(),
        };
        Some((found, target))
    }

    pub fn id(&self) -> egui::Id {
        self.id
    }

    pub fn request_focus(&mut self) {
        self.focus_requested = true;
    }

    pub fn show(&mut self, ui: &mut Ui, rect: Rect, session: &Session, palette: &TerminalPalette) -> Response {
        let ctx = ui.ctx().clone();
        let response = ui.interact(rect, self.id, Sense::click_and_drag());
        crate::accessibility::name_with(&response, egui::Role::Terminal, || t!("ide-terminal-title"));
        if response.clicked() || response.secondary_clicked() || std::mem::take(&mut self.focus_requested) {
            response.request_focus();
        }
        let focused = response.has_focus();

        let fonts = Fonts::new(&ctx);
        let cell = fonts.cell_size(&ctx);
        let grid = rect.shrink(PADDING);
        let size = Size::new(
            ((grid.height() / cell.y).floor().max(1.0) as u16).max(1),
            ((grid.width() / cell.x).floor().max(2.0) as u16).max(2),
        );
        session.resize(size);

        if focused {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    self.id,
                    egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
                );
            });
        }
        let screen = session.snapshot();
        if focused && handle_keyboard(ui, session, &screen) {
            self.copy_requested = true;
        }
        if response.hovered() {
            self.handle_wheel(ui, session, &screen, cell.y);
        }
        select_with_pointer(ui, &response, session, grid.min, cell, size);

        let screen = session.snapshot();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, palette.background);
        paint(palette, &painter, &fonts, grid.min, cell, &screen, focused);
        self.grid = Some((grid.min, cell));
        self.follow_links(ui, &response, &screen, grid.min, cell, palette);
        response
    }

    fn follow_links(
        &mut self,
        ui: &Ui,
        response: &Response,
        screen: &Screen,
        origin: Pos2,
        cell: Vec2,
        palette: &TerminalPalette,
    ) {
        let command = ui.input(|input| input.modifiers.command);
        let pointer = response.hover_pos().filter(|_| command);
        let under = pointer.and_then(|pointer| self.link_under(screen, origin, cell, pointer));
        let Some((link, target)) = under else {
            self.hovered = None;
            return;
        };
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        let painter = ui.painter_at(response.rect);
        for (row, cols) in &link.cells {
            let y = origin.y + (*row as f32 + 1.0) * cell.y - 1.5;
            let color = screen
                .cell(*row, cols.start)
                .map_or(palette.foreground, |first| CellStyle::of(palette, first).foreground);
            let x = origin.x + cols.start as f32 * cell.x..=origin.x + cols.end as f32 * cell.x;
            painter.hline(x, y, Stroke::new(1.0, color));
        }
        if response.clicked()
            && let Some(at) = response.interact_pointer_pos()
        {
            self.clicked = Some(LinkClick {
                kind: link.link.kind,
                text: link.link.text.clone(),
                target,
                line: link.link.line,
                column: link.link.column,
                at,
            });
        }
        self.hovered = Some(link);
    }

    fn handle_wheel(&mut self, ui: &Ui, session: &Session, screen: &Screen, line_height: f32) {
        let delta = ui.input(|input| input.smooth_scroll_delta.y);
        if delta == 0.0 {
            return;
        }
        self.scroll_remainder += delta / line_height;
        let lines = self.scroll_remainder.trunc();
        self.scroll_remainder -= lines;
        let lines = lines as i32;
        if lines == 0 {
            return;
        }
        if screen.alt_screen && screen.alternate_scroll {
            let key = if lines > 0 { Key::Up } else { Key::Down };
            let bytes: Vec<u8> = (0..lines.unsigned_abs())
                .flat_map(|_| keys::encode(key, Modifiers::default(), screen.app_cursor))
                .collect();
            session.write(bytes);
        } else if !screen.alt_screen {
            session.scroll(lines);
        }
    }
}

fn select_with_pointer(ui: &Ui, response: &Response, session: &Session, origin: Pos2, cell: Vec2, size: Size) {
    let modifiers = ui.input(|input| input.modifiers);
    let at = |pointer: Pos2| grid_point(origin, cell, size, pointer);
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(start) = ui.input(|input| input.pointer.press_origin())
    {
        if modifiers.shift && session.has_selection() {
            session.extend_selection(at(start));
        } else {
            let kind = if modifiers.alt { SelectionKind::Block } else { SelectionKind::Simple };
            session.start_selection(kind, at(start));
        }
    }
    if response.dragged_by(egui::PointerButton::Primary)
        && let Some(pointer) = response.interact_pointer_pos()
    {
        session.extend_selection(at(pointer));
        let rows = f32::from(size.rows) * cell.y;
        if pointer.y < origin.y {
            session.scroll(1);
        } else if pointer.y > origin.y + rows {
            session.scroll(-1);
        }
        ui.ctx().request_repaint();
    }
    let Some(pointer) = response.interact_pointer_pos() else {
        return;
    };
    if response.triple_clicked() {
        session.start_selection(SelectionKind::Line, at(pointer));
    } else if response.double_clicked() {
        session.start_selection(SelectionKind::Word, at(pointer));
    } else if response.clicked() && modifiers.shift && session.has_selection() {
        session.extend_selection(at(pointer));
    } else if response.clicked() && !modifiers.command {
        session.clear_selection();
    }
}

fn grid_point(origin: Pos2, cell: Vec2, size: Size, pointer: Pos2) -> GridPoint {
    let x = ((pointer.x - origin.x) / cell.x).max(0.0);
    let y = ((pointer.y - origin.y) / cell.y).max(0.0);
    let cols = usize::from(size.cols.max(1));
    let col = (x.floor() as usize).min(cols - 1);
    let right = x.floor() as usize >= cols || x.fract() >= 0.5;
    GridPoint { row: (y.floor() as usize).min(usize::from(size.rows.max(1)) - 1), col, right }
}

fn paint(
    palette: &TerminalPalette,
    painter: &egui::Painter,
    fonts: &Fonts,
    origin: Pos2,
    cell: Vec2,
    screen: &Screen,
    focused: bool,
) {
    for (row, cols) in &screen.selection {
        let min = Pos2::new(origin.x + cols.start as f32 * cell.x, origin.y + *row as f32 * cell.y);
        let width = (cols.end - cols.start) as f32 * cell.x;
        painter.rect_filled(Rect::from_min_size(min, Vec2::new(width, cell.y)), 0.0, palette.selection);
    }
    for (row, line) in screen.lines.iter().enumerate() {
        let y = origin.y + row as f32 * cell.y;
        let mut col = 0;
        while col < line.len() {
            let style = CellStyle::of(palette, &line[col]);
            let start = col;
            while col < line.len() && CellStyle::of(palette, &line[col]).background == style.background {
                col += 1;
            }
            if style.background != palette.background && !screen.selected(row, start) {
                let min = Pos2::new(origin.x + start as f32 * cell.x, y);
                painter.rect_filled(
                    Rect::from_min_size(min, Vec2::new((col - start) as f32 * cell.x, cell.y)),
                    0.0,
                    style.background,
                );
            }
        }

        let cursor_col = (screen.cursor.visible && focused && screen.cursor.row == row).then_some(screen.cursor.col);
        let mut col = 0;
        while col < line.len() {
            let x = origin.x + col as f32 * cell.x;
            let current = &line[col];
            let style = CellStyle::of(palette, current);
            if Some(col) == cursor_col || !is_plain(current) {
                if !current.spacer {
                    let width = if current.wide { 2.0 } else { 1.0 };
                    let mut color = style.foreground;
                    if Some(col) == cursor_col {
                        painter.rect_filled(
                            Rect::from_min_size(Pos2::new(x, y), Vec2::new(cell.x * width, cell.y)),
                            0.0,
                            palette.cursor,
                        );
                        color = style.background;
                    }
                    let mut text = String::from(current.ch);
                    text.extend(&current.combining);
                    if !text.trim().is_empty() {
                        let center = Pos2::new(x + cell.x * width / 2.0, y);
                        painter.text(
                            center,
                            egui::Align2::CENTER_TOP,
                            text,
                            fonts.font(style.bold, style.italic),
                            color,
                        );
                    }
                    decorate(painter, &style, color, Pos2::new(x, y), Vec2::new(cell.x * width, cell.y));
                }
                col += 1;
                continue;
            }
            let start = col;
            let mut text = String::new();
            while col < line.len()
                && Some(col) != cursor_col
                && is_plain(&line[col])
                && CellStyle::of(palette, &line[col]).same_text(&style)
            {
                text.push(line[col].ch);
                col += 1;
            }
            if !text.trim().is_empty() {
                painter.text(
                    Pos2::new(x, y),
                    egui::Align2::LEFT_TOP,
                    text,
                    fonts.font(style.bold, style.italic),
                    style.foreground,
                );
            }
            decorate(
                painter,
                &style,
                style.foreground,
                Pos2::new(x, y),
                Vec2::new((col - start) as f32 * cell.x, cell.y),
            );
        }
    }

    if screen.cursor.visible && !focused {
        let min = Pos2::new(origin.x + screen.cursor.col as f32 * cell.x, origin.y + screen.cursor.row as f32 * cell.y);
        painter.rect_stroke(
            Rect::from_min_size(min, cell).shrink(0.5),
            0.0,
            Stroke::new(1.0, palette.cursor),
            egui::StrokeKind::Inside,
        );
    }
}

fn handle_keyboard(ui: &Ui, session: &Session, screen: &Screen) -> bool {
    let events = ui.input(|input| input.events.clone());
    let mut sent = false;
    let mut copy = false;
    for event in events {
        let bytes = match event {
            egui::Event::Text(text) => text.into_bytes(),
            egui::Event::Paste(text) => keys::paste(&text, screen.bracketed_paste),
            egui::Event::Copy if session.has_selection() => {
                copy = true;
                Vec::new()
            }
            egui::Event::Copy => vec![0x03],
            egui::Event::Cut if !cfg!(target_os = "macos") => vec![0x18],
            egui::Event::Key { key: egui::Key::A, pressed: true, modifiers, .. } if modifiers.mac_cmd => {
                session.select_all();
                Vec::new()
            }
            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                encode_key(key, modifiers, screen.app_cursor).unwrap_or_default()
            }
            _ => Vec::new(),
        };
        if !bytes.is_empty() {
            session.write(bytes);
            sent = true;
        }
    }
    if sent {
        session.clear_selection();
        session.scroll_to_bottom();
    }
    copy
}

struct Fonts {
    regular: FontId,
    bold: Option<FontId>,
    italic: Option<FontId>,
}

impl Fonts {
    fn new(ctx: &egui::Context) -> Self {
        let installed = |family: FontFamily| {
            let known = ctx.fonts(|fonts| fonts.definitions().families.contains_key(&family));
            known.then(|| FontId::new(FONT_SIZE, family))
        };
        Fonts {
            regular: FontId::monospace(FONT_SIZE),
            bold: installed(theme::family::mono_bold()),
            italic: installed(theme::family::mono_italic()),
        }
    }

    fn font(&self, bold: bool, italic: bool) -> FontId {
        let face = match (bold, italic) {
            (true, _) => self.bold.as_ref(),
            (false, true) => self.italic.as_ref(),
            (false, false) => None,
        };
        face.unwrap_or(&self.regular).clone()
    }

    fn cell_size(&self, ctx: &egui::Context) -> Vec2 {
        let (width, height) =
            ctx.fonts_mut(|fonts| (fonts.glyph_width(&self.regular, 'M'), fonts.row_height(&self.regular)));
        let pixels = ctx.pixels_per_point();
        Vec2::new(width.max(1.0), ((height * pixels).ceil() / pixels).max(1.0))
    }
}

#[derive(Clone, Copy, PartialEq)]
struct CellStyle {
    foreground: Color32,
    background: Color32,
    bold: bool,
    italic: bool,
    underline: bool,
    strikethrough: bool,
}

impl CellStyle {
    fn of(palette: &TerminalPalette, cell: &Cell) -> Self {
        let mut foreground = palette.resolve(cell.fg, palette.foreground);
        let mut background = palette.resolve(cell.bg, palette.background);
        if cell.attrs.inverse {
            std::mem::swap(&mut foreground, &mut background);
        }
        if cell.attrs.dim {
            foreground = blend(foreground, background);
        }
        if cell.attrs.hidden {
            foreground = background;
        }
        CellStyle {
            foreground,
            background,
            bold: cell.attrs.bold,
            italic: cell.attrs.italic,
            underline: cell.attrs.underline,
            strikethrough: cell.attrs.strikethrough,
        }
    }

    fn same_text(&self, other: &CellStyle) -> bool {
        self.foreground == other.foreground
            && self.bold == other.bold
            && self.italic == other.italic
            && self.underline == other.underline
            && self.strikethrough == other.strikethrough
    }
}

fn blend(color: Color32, background: Color32) -> Color32 {
    let mix = |a: u8, b: u8| ((u16::from(a) + u16::from(b)) / 2) as u8;
    Color32::from_rgb(mix(color.r(), background.r()), mix(color.g(), background.g()), mix(color.b(), background.b()))
}

fn decorate(painter: &egui::Painter, style: &CellStyle, color: Color32, min: Pos2, size: Vec2) {
    let stroke = Stroke::new(1.0, color);
    if style.underline {
        let y = min.y + size.y - 1.5;
        painter.hline(min.x..=min.x + size.x, y, stroke);
    }
    if style.strikethrough {
        let y = min.y + size.y / 2.0;
        painter.hline(min.x..=min.x + size.x, y, stroke);
    }
}

fn is_plain(cell: &Cell) -> bool {
    !cell.wide && !cell.spacer && cell.combining.is_empty() && (cell.ch == ' ' || cell.ch.is_ascii_graphic())
}

fn encode_key(key: egui::Key, modifiers: egui::Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    use egui::Key as K;
    if modifiers.mac_cmd {
        return None;
    }
    let mods = Modifiers { shift: modifiers.shift, alt: modifiers.alt, ctrl: modifiers.ctrl };
    let named = match key {
        K::Enter => Some(Key::Enter),
        K::Backspace => Some(Key::Backspace),
        K::Tab => Some(Key::Tab),
        K::Escape => Some(Key::Escape),
        K::ArrowUp => Some(Key::Up),
        K::ArrowDown => Some(Key::Down),
        K::ArrowLeft => Some(Key::Left),
        K::ArrowRight => Some(Key::Right),
        K::Home => Some(Key::Home),
        K::End => Some(Key::End),
        K::PageUp => Some(Key::PageUp),
        K::PageDown => Some(Key::PageDown),
        K::Insert => Some(Key::Insert),
        K::Delete => Some(Key::Delete),
        K::F1 => Some(Key::F(1)),
        K::F2 => Some(Key::F(2)),
        K::F3 => Some(Key::F(3)),
        K::F4 => Some(Key::F(4)),
        K::F5 => Some(Key::F(5)),
        K::F6 => Some(Key::F(6)),
        K::F7 => Some(Key::F(7)),
        K::F8 => Some(Key::F(8)),
        K::F9 => Some(Key::F(9)),
        K::F10 => Some(Key::F(10)),
        K::F11 => Some(Key::F(11)),
        K::F12 => Some(Key::F(12)),
        _ => None,
    };
    if let Some(named) = named {
        return Some(keys::encode(named, mods, app_cursor));
    }
    if modifiers.ctrl {
        let mut symbol = key.symbol_or_name().chars();
        if let (Some(ch), None) = (symbol.next(), symbol.next()) {
            let byte = keys::control(ch)?;
            return Some(if modifiers.alt { vec![0x1b, byte] } else { vec![byte] });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_map_to_terminal_input() {
        let none = egui::Modifiers::NONE;
        assert_eq!(encode_key(egui::Key::Enter, none, false), Some(b"\r".to_vec()));
        assert_eq!(encode_key(egui::Key::ArrowUp, none, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(encode_key(egui::Key::C, egui::Modifiers::CTRL, false), Some(vec![3]));
        assert_eq!(encode_key(egui::Key::OpenBracket, egui::Modifiers::CTRL, false), Some(vec![0x1b]));
        assert_eq!(encode_key(egui::Key::A, none, false), None);
        assert_eq!(encode_key(egui::Key::V, egui::Modifiers::MAC_CMD, false), None);
    }

    #[test]
    fn styles_apply_inverse_and_palette() {
        let palette = TerminalPalette::zen_dark();
        let mut cell = Cell { fg: Color::Indexed(1), ..Cell::default() };
        assert_eq!(CellStyle::of(&palette, &cell).foreground, palette.ansi[1]);
        cell.attrs.inverse = true;
        let style = CellStyle::of(&palette, &cell);
        assert_eq!((style.foreground, style.background), (palette.background, palette.ansi[1]));
        assert_eq!(palette.resolve(Color::Indexed(196), palette.foreground), Color32::from_rgb(255, 0, 0));
        assert_eq!(palette.resolve(Color::Rgb(1, 2, 3), palette.foreground), Color32::from_rgb(1, 2, 3));
    }

    fn screen_of(rows: &[&str]) -> Screen {
        let lines = rows
            .iter()
            .map(|text| {
                let mut cells: Vec<Cell> = text.chars().map(|ch| Cell { ch, ..Cell::default() }).collect();
                cells.resize(40, Cell::default());
                cells
            })
            .collect();
        Screen { lines, cols: 40, ..Screen::default() }
    }

    fn link_frame(
        ctx: &egui::Context,
        view: &mut PaneView,
        screen: &Screen,
        pointer: Pos2,
        command: bool,
        button: Option<bool>,
    ) -> egui::CursorIcon {
        let modifiers = if command { egui::Modifiers::COMMAND } else { egui::Modifiers::NONE };
        let mut events = vec![egui::Event::ModifiersChanged(modifiers), egui::Event::PointerMoved(pointer)];
        if let Some(pressed) = button {
            events.push(egui::Event::PointerButton {
                pos: pointer,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers,
            });
        }
        let input = egui::RawInput { events, ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| {
            let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 100.0));
            let response = ui.interact(rect, view.id(), Sense::click());
            view.follow_links(ui, &response, screen, rect.min, Vec2::new(10.0, 20.0), &TerminalPalette::zen_dark());
        });
        output.textures_delta.clear();
        output.platform_output.cursor_icon
    }

    #[test]
    fn a_pointer_lands_on_the_cell_under_it_and_the_side_it_is_on() {
        let (origin, cell, size) = (Pos2::new(10.0, 10.0), Vec2::new(8.0, 16.0), Size::new(4, 10));
        let at = |x, y| grid_point(origin, cell, size, Pos2::new(x, y));
        assert_eq!(at(11.0, 11.0), GridPoint { row: 0, col: 0, right: false });
        assert_eq!(at(10.0 + 8.0 * 3.0 + 6.0, 10.0 + 16.0 * 2.0 + 1.0), GridPoint { row: 2, col: 3, right: true });
        assert_eq!(at(500.0, 500.0), GridPoint { row: 3, col: 9, right: true });
        assert_eq!(at(0.0, 0.0), GridPoint { row: 0, col: 0, right: false });
    }

    #[cfg(unix)]
    thread_local! {
        static CLOCK: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
    }

    fn pass(seconds: f64) {
        CLOCK.with(|clock| clock.set(clock.get() + seconds));
    }

    fn pointer_frame(ctx: &egui::Context, view: &mut PaneView, session: &Session, events: Vec<egui::Event>) {
        pass(1.0 / 60.0);
        let time = CLOCK.with(std::cell::Cell::get);
        let input = egui::RawInput {
            events,
            time: Some(time),
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 200.0))),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 200.0));
            view.show(ui, rect, session, &TerminalPalette::zen_dark());
        });
        output.textures_delta.clear();
    }

    #[cfg(unix)]
    fn button(pos: Pos2, pressed: bool, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers }
    }

    #[cfg(unix)]
    #[test]
    fn dragging_and_double_clicking_select_text_and_copy_takes_it() {
        let options = Options::command(
            "/bin/sh",
            &["-c", "printf 'hello world ~/src/a.rs:3 end'; sleep 30"],
            std::env::temp_dir(),
        );
        let session = Session::spawn(options, || {}).expect("the shell starts");
        let ctx = egui::Context::default();
        let mut view = PaneView::new(egui::Id::unique("selecting"));
        let start = std::time::Instant::now();
        while !session.snapshot().text().contains("end") {
            assert!(start.elapsed() < std::time::Duration::from_secs(60), "no output");
            pointer_frame(&ctx, &mut view, &session, Vec::new());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        pointer_frame(&ctx, &mut view, &session, Vec::new());
        let none = egui::Modifiers::NONE;
        let (from, to) = (
            view.cell_center(0, 0).unwrap() - Vec2::new(3.0, 0.0),
            view.cell_center(0, 4).unwrap() + Vec2::new(3.0, 0.0),
        );
        pointer_frame(&ctx, &mut view, &session, vec![egui::Event::PointerMoved(from), button(from, true, none)]);
        pointer_frame(&ctx, &mut view, &session, vec![egui::Event::PointerMoved(from + Vec2::new(4.0, 0.0))]);
        pointer_frame(&ctx, &mut view, &session, vec![egui::Event::PointerMoved(to)]);
        pointer_frame(&ctx, &mut view, &session, vec![button(to, false, none)]);
        assert_eq!(session.selection_text().as_deref(), Some("hello"));
        assert!(session.snapshot().selected(0, 2));

        view.request_focus();
        pointer_frame(&ctx, &mut view, &session, vec![egui::Event::Copy]);
        pointer_frame(&ctx, &mut view, &session, vec![egui::Event::Copy]);
        assert!(view.take_copy(), "⌘C with a selection copies instead of sending Ctrl+C");

        let path = view.cell_center(0, 15).unwrap();
        pointer_frame(&ctx, &mut view, &session, vec![egui::Event::PointerMoved(path), button(path, true, none)]);
        pointer_frame(&ctx, &mut view, &session, vec![button(path, false, none)]);
        pointer_frame(&ctx, &mut view, &session, vec![button(path, true, none)]);
        pointer_frame(&ctx, &mut view, &session, vec![button(path, false, none)]);
        assert_eq!(session.selection_text().as_deref(), Some("~/src/a.rs:3"));

        let later = view.cell_center(0, 26).unwrap();
        pointer_frame(&ctx, &mut view, &session, vec![egui::Event::PointerMoved(later)]);
        pass(0.6);
        let shift = egui::Modifiers::SHIFT;
        pointer_frame(
            &ctx,
            &mut view,
            &session,
            vec![egui::Event::ModifiersChanged(shift), button(later, true, shift)],
        );
        pointer_frame(&ctx, &mut view, &session, vec![button(later, false, shift)]);
        assert_eq!(session.selection_text().as_deref(), Some("~/src/a.rs:3 end"), "Shift-click extends");

        pass(0.6);
        pointer_frame(&ctx, &mut view, &session, vec![egui::Event::ModifiersChanged(none), button(later, true, none)]);
        pointer_frame(&ctx, &mut view, &session, vec![button(later, false, none)]);
        assert!(!session.has_selection(), "a click clears it");
        pointer_frame(&ctx, &mut view, &session, vec![egui::Event::Copy]);
        assert!(!view.take_copy(), "with nothing selected ⌘C goes to the program");
    }

    #[test]
    fn links_show_and_click_only_while_command_is_held() {
        let ctx = egui::Context::default();
        let mut view = PaneView::new(egui::Id::unique("links"));
        let screen = screen_of(&["see https://example.com/x now"]);
        let on_link = Pos2::new(85.0, 10.0);

        assert_eq!(link_frame(&ctx, &mut view, &screen, on_link, false, None), egui::CursorIcon::Default);
        assert!(view.hovered_link().is_none());
        link_frame(&ctx, &mut view, &screen, on_link, false, Some(true));
        link_frame(&ctx, &mut view, &screen, on_link, false, Some(false));
        assert_eq!(view.take_link_click(), None);

        assert_eq!(link_frame(&ctx, &mut view, &screen, on_link, true, None), egui::CursorIcon::PointingHand);
        assert_eq!(view.hovered_link().map(|link| link.cells.clone()), Some(vec![(0, 4..25)]));
        link_frame(&ctx, &mut view, &screen, on_link, true, Some(true));
        link_frame(&ctx, &mut view, &screen, on_link, true, Some(false));
        let click = view.take_link_click().expect("the ⌘-click");
        assert_eq!((click.kind, click.target.as_str()), (LinkKind::Url, "https://example.com/x"));
        assert_eq!(view.take_link_click(), None, "taken once");

        assert_eq!(link_frame(&ctx, &mut view, &screen, on_link, false, None), egui::CursorIcon::Default);
        assert!(view.hovered_link().is_none());
        assert_eq!(link_frame(&ctx, &mut view, &screen, Pos2::new(15.0, 10.0), true, None), egui::CursorIcon::Default);
        assert!(view.hovered_link().is_none());
    }

    #[test]
    fn a_path_is_a_link_only_when_it_names_a_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        let ctx = egui::Context::default();
        let mut view = PaneView::new(egui::Id::unique("paths"));
        view.set_link_bases(vec![dir.path().to_owned()]);
        let screen = screen_of(&["src/main.rs:1:4 and src/gone.rs:2"]);
        link_frame(&ctx, &mut view, &screen, Pos2::new(25.0, 10.0), false, None);
        assert_eq!(
            link_frame(&ctx, &mut view, &screen, Pos2::new(25.0, 10.0), true, None),
            egui::CursorIcon::PointingHand
        );
        link_frame(&ctx, &mut view, &screen, Pos2::new(25.0, 10.0), true, Some(true));
        link_frame(&ctx, &mut view, &screen, Pos2::new(25.0, 10.0), true, Some(false));
        let click = view.take_link_click().unwrap();
        assert_eq!(click.target, dir.path().join("src/main.rs").display().to_string());
        assert_eq!((click.line, click.column), (Some(1), Some(4)));
        assert_eq!(click.command_target(), format!("{}:1:4", dir.path().join("src/main.rs").display()));
        assert_eq!(link_frame(&ctx, &mut view, &screen, Pos2::new(255.0, 10.0), true, None), egui::CursorIcon::Default);
        assert!(view.hovered_link().is_none(), "a path that names nothing");
    }

    #[test]
    fn zen_dark_is_the_pastel_palette() {
        let rgb = Color32::from_rgb;
        let before = TerminalPalette {
            background: rgb(0x1F, 0x20, 0x23),
            foreground: rgb(0xDF, 0xE1, 0xE5),
            cursor: rgb(0xCE, 0xD0, 0xD6),
            selection: rgb(0x2E, 0x43, 0x6E),
            ansi: [
                rgb(0x45, 0x48, 0x4D),
                rgb(0xE5, 0x8A, 0x86),
                rgb(0x93, 0xC4, 0x9A),
                rgb(0xE3, 0xC4, 0x7E),
                rgb(0x8A, 0xAF, 0xE6),
                rgb(0xCD, 0xA2, 0xDC),
                rgb(0x86, 0xC7, 0xC8),
                rgb(0xC4, 0xC6, 0xCC),
                rgb(0x6F, 0x73, 0x7A),
                rgb(0xF2, 0xA7, 0xA3),
                rgb(0xAD, 0xD5, 0xB2),
                rgb(0xEE, 0xD6, 0x9C),
                rgb(0xA9, 0xC6, 0xF2),
                rgb(0xDD, 0xBC, 0xE8),
                rgb(0xA6, 0xDA, 0xDB),
                rgb(0xF2, 0xF3, 0xF5),
            ],
        };
        assert_eq!(TerminalPalette::zen_dark(), before);
    }
}
