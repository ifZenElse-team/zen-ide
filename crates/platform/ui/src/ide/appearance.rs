use egui::{Key, Rect, Sense, Ui, Vec2, ViewportCommand, pos2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_workbench::Workbench;

use super::IdeView;
use super::settings::{EditorFont, IdeSettings};
use super::tool_windows::ToolWindowLayout;

const CENTRED_COLUMNS: f32 = 120.0;
const CENTRED_GUTTER: f32 = 96.0;
const FULL_SCREEN_WAIT: f64 = 2.0;
const BRANCHES_TOP: f32 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Presentation,
    DistractionFree,
    Zen,
}

impl Mode {
    pub(super) fn name(self) -> &'static str {
        match self {
            Mode::Presentation => "presentation",
            Mode::DistractionFree => "distractionFree",
            Mode::Zen => "zen",
        }
    }

    pub(super) fn of_action(action: &str) -> Option<Mode> {
        match action {
            "view.presentationMode.toggle" => Some(Mode::Presentation),
            "view.distractionFreeMode.toggle" => Some(Mode::DistractionFree),
            "view.zenMode.toggle" => Some(Mode::Zen),
            _ => None,
        }
    }

    fn full_screen(self) -> bool {
        self != Mode::DistractionFree
    }
}

#[derive(Default)]
pub(super) struct Appearance {
    mode: Option<Mode>,
    kept: Option<ToolWindowLayout>,
    full_screen: bool,
    asked: Option<(bool, Option<f64>)>,
    by_mode: bool,
    escape: bool,
}

impl Appearance {
    pub(super) fn hides_chrome(&self) -> bool {
        self.mode.is_some()
    }

    pub(super) fn presents(&self) -> bool {
        self.mode == Some(Mode::Presentation)
    }

    pub(super) fn hides_line_numbers(&self) -> bool {
        matches!(self.mode, Some(Mode::DistractionFree | Mode::Zen))
    }

    pub(super) fn full_screen(&self) -> bool {
        self.asked.map_or(self.full_screen, |(on, _)| on)
    }

    pub(super) fn editor_font(&self, settings: &IdeSettings) -> EditorFont {
        let mut font = settings.editor_font.clone();
        if self.mode == Some(Mode::Presentation) {
            font.size = settings.presentation_font_size;
        }
        font
    }

    pub(super) fn editor_rect(&self, ctx: &egui::Context, area: Rect, font: &EditorFont) -> Rect {
        if !self.hides_line_numbers() {
            return area;
        }
        let char_width = super::metrics::EditorMetrics::new(ctx, font, 4).char_width;
        let width = CENTRED_COLUMNS * char_width + CENTRED_GUTTER;
        if area.width() <= width {
            return area;
        }
        let left = area.center().x - width / 2.0;
        Rect::from_min_max(pos2(left, area.top()), pos2(left + width, area.bottom()))
    }

    pub(super) fn frame(&mut self, ctx: &egui::Context) {
        let (shown, now) = ctx.input(|input| (input.viewport().fullscreen.unwrap_or(false), input.time));
        if shown != self.full_screen && self.asked.is_none() {
            self.by_mode = false;
        }
        self.full_screen = shown;
        match self.asked {
            Some((on, _)) if on == shown => self.asked = None,
            Some((on, None)) => {
                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(on));
                self.asked = Some((on, Some(now)));
            }
            Some((_, Some(sent))) if now - sent > FULL_SCREEN_WAIT => self.asked = None,
            _ => {}
        }
    }

    pub(super) fn full_screen_by_mode(&self) -> bool {
        self.by_mode
    }

    fn ask_full_screen(&mut self, on: bool) {
        if on != self.full_screen() {
            self.asked = Some((on, None));
        }
    }
}

impl IdeView {
    pub(super) fn kept_layout(&self) -> ToolWindowLayout {
        self.appearance.kept.clone().unwrap_or_else(|| self.tool_windows.clone())
    }

    pub(super) fn toggle_mode(&mut self, mode: Mode) {
        match self.appearance.mode {
            Some(current) if current == mode => {
                self.leave_mode();
                return;
            }
            Some(_) => {}
            None => {
                self.appearance.kept = Some(self.hide_all_tools());
                self.lsp.close_popup();
                self.run_widget.close_popup();
            }
        }
        self.appearance.mode = Some(mode);
        if mode.full_screen() && !self.appearance.full_screen() {
            self.appearance.ask_full_screen(true);
            self.appearance.by_mode = true;
        } else if !mode.full_screen() && self.appearance.by_mode {
            self.appearance.ask_full_screen(false);
            self.appearance.by_mode = false;
        }
    }

    pub(super) fn leave_mode(&mut self) {
        if self.appearance.mode.take().is_none() {
            return;
        }
        if let Some(kept) = self.appearance.kept.take() {
            self.restore_tools(kept);
        }
        if std::mem::take(&mut self.appearance.by_mode) {
            self.appearance.ask_full_screen(false);
        }
    }

    pub(super) fn toggle_full_screen(&mut self) {
        let on = !self.appearance.full_screen();
        self.appearance.ask_full_screen(on);
        self.appearance.by_mode = false;
    }

    pub(super) fn appearance_state(&self) -> (Value, bool) {
        (json!(self.appearance.mode.map(Mode::name)), self.appearance.full_screen())
    }

    pub(super) fn hidden_toolbar(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        if self.vcs.info.root.is_none() {
            self.vcs.open_branches = false;
            self.vcs.popup.close();
            return;
        }
        if std::mem::take(&mut self.vcs.open_branches) && !self.vcs.popup.open {
            self.vcs.popup.toggle();
        }
        let screen = ui.ctx().content_rect();
        let left = screen.center().x - super::branches_popup::WIDTH / 2.0;
        let anchor = Rect::from_min_size(pos2(left, screen.top() + BRANCHES_TOP), Vec2::ZERO);
        self.branches_popup(ui, registry, session, anchor);
    }

    pub(super) fn appearance_title(&self, action: &str) -> Option<String> {
        if action == super::ide_zoom::OPEN {
            return Some(self.ide_zoom_title());
        }
        let on = match action {
            "view.fullScreen.toggle" => self.appearance.full_screen(),
            _ => self.appearance.mode == Some(Mode::of_action(action)?),
        };
        let name = action.trim_start_matches("view.").trim_end_matches(".toggle");
        Some(zen_i18n::t!(&format!("menu-view-{}-{}", if on { "exit" } else { "enter" }, name)))
    }

    pub(super) fn watch_escape(&mut self, ctx: &egui::Context, session: &Session) {
        self.appearance.escape = self.appearance.mode.is_some()
            && ctx.input(|input| input.modifiers.is_none() && input.key_pressed(Key::Escape))
            && self.escape_free(ctx, session);
    }

    pub(super) fn follow_escape(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.appearance.escape) && ctx.input(|input| input.key_pressed(Key::Escape)) {
            self.leave_mode();
        }
    }

    fn escape_free(&self, ctx: &egui::Context, session: &Session) -> bool {
        let popups = self.window_state(session)["popups"].as_array().is_none_or(Vec::is_empty);
        let focus = keyboard_free(ctx, self.editor_view.has_focus_in(ctx));
        let workbench = session.state::<Workbench>();
        let find = workbench.is_some_and(|workbench| workbench.find().open);
        let carets =
            workbench.and_then(Workbench::active_document).is_some_and(|document| document.selections().len() > 1);
        popups
            && focus
            && !find
            && !carets
            && !self.editor_view.has_popup()
            && ctx.memory(|memory| memory.top_modal_layer()).is_none()
            && !egui::Popup::is_any_open(ctx)
    }
}

fn keyboard_free(ctx: &egui::Context, editor: bool) -> bool {
    editor || ctx.memory(egui::Memory::focused).is_none()
}

pub(super) fn title_bar(ui: &mut Ui) {
    let rect = ui.max_rect();
    title_strip(ui, rect, ui.scope_id().with("title-drag"));
}

pub(super) fn title_strip(ui: &mut Ui, rect: Rect, id: egui::Id) -> egui::Response {
    let strip = ui.interact(rect, id, Sense::click_and_drag());
    if strip.drag_started() {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if strip.double_clicked() {
        note_title_double_click(ui.ctx());
    }
    settle_title_double_click(ui.ctx(), zen_macos::system_title_bar_action);
    strip
}

const SETTLE: f64 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq)]
struct TitleDoubleClick {
    at: f64,
    outer: Option<Rect>,
    maximized: Option<bool>,
}

fn title_click_key() -> egui::Id {
    egui::Id::unique("zen-title-double-click")
}

fn note_title_double_click(ctx: &egui::Context) {
    let noted = ctx.input(|input| TitleDoubleClick {
        at: input.time,
        outer: input.viewport().outer_rect,
        maximized: input.viewport().maximized,
    });
    ctx.data_mut(|data| data.insert_temp(title_click_key(), noted));
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(SETTLE));
}

fn settle_title_double_click(ctx: &egui::Context, action: impl FnOnce() -> zen_macos::TitleBarAction) {
    let Some(noted) = ctx.data(|data| data.get_temp::<TitleDoubleClick>(title_click_key())) else { return };
    let (now, outer, maximized, minimized) = ctx.input(|input| {
        let viewport = input.viewport();
        (input.time, viewport.outer_rect, viewport.maximized, viewport.minimized)
    });
    let waited = now - noted.at;
    if waited < SETTLE {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(SETTLE - waited));
        return;
    }
    ctx.data_mut(|data| data.remove::<TitleDoubleClick>(title_click_key()));
    let answered = outer != noted.outer || maximized != noted.maximized || minimized == Some(true);
    if !answered {
        double_click_title(ctx, action());
    }
}

pub(super) fn double_click_title(ctx: &egui::Context, action: zen_macos::TitleBarAction) {
    match action {
        zen_macos::TitleBarAction::Zoom => {
            let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        }
        zen_macos::TitleBarAction::Minimise => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
        zen_macos::TitleBarAction::Nothing => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(
        ctx: &egui::Context,
        (time, outer): (f64, Rect),
        events: Vec<egui::Event>,
        action: zen_macos::TitleBarAction,
    ) -> Vec<ViewportCommand> {
        let mut input = egui::RawInput { events, time: Some(time), ..egui::RawInput::default() };
        let info = egui::ViewportInfo { outer_rect: Some(outer), maximized: Some(false), ..Default::default() };
        input.viewports.insert(egui::ViewportId::ROOT, info);
        let mut output = ctx.run_ui(input, |ui| {
            let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(400.0, 40.0));
            let strip = ui.interact(rect, ui.scope_id().with("strip"), Sense::click_and_drag());
            if strip.double_clicked() {
                note_title_double_click(ui.ctx());
            }
            settle_title_double_click(ui.ctx(), || action);
        });
        output.textures_delta.clear();
        output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|viewport| viewport.commands.clone())
            .unwrap_or_default()
            .into_iter()
            .filter(|command| matches!(command, ViewportCommand::Maximized(_) | ViewportCommand::Minimized(_)))
            .collect()
    }

    fn double_click(action: zen_macos::TitleBarAction, outer_after: impl Fn(f64) -> Rect) -> Vec<ViewportCommand> {
        let at = pos2(200.0, 20.0);
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let ctx = egui::Context::default();
        let window = Rect::from_min_size(pos2(100.0, 100.0), egui::vec2(800.0, 600.0));
        let mut seen = Vec::new();
        let clicks = [egui::Event::PointerMoved(at), press(true), press(false), press(true), press(false)];
        for (index, event) in clicks.into_iter().enumerate() {
            seen.extend(frame(&ctx, (index as f64 * 0.05, window), vec![event], action));
        }
        assert!(seen.is_empty(), "nothing at once: the system may answer first");
        for time in [0.3, 0.4, 0.6] {
            seen.extend(frame(&ctx, (time, outer_after(time)), Vec::new(), action));
        }
        seen
    }

    #[test]
    fn a_double_click_on_the_title_strip_does_what_the_system_setting_says_once() {
        let unmoved = |_| Rect::from_min_size(pos2(100.0, 100.0), egui::vec2(800.0, 600.0));
        assert_eq!(double_click(zen_macos::TitleBarAction::Zoom, unmoved), [ViewportCommand::Maximized(true)]);
        assert_eq!(double_click(zen_macos::TitleBarAction::Minimise, unmoved), [ViewportCommand::Minimized(true)]);
        assert_eq!(double_click(zen_macos::TitleBarAction::Nothing, unmoved), []);
        let zoomed_by_the_system = |_| Rect::from_min_size(pos2(0.0, 25.0), egui::vec2(1440.0, 875.0));
        assert_eq!(
            double_click(zen_macos::TitleBarAction::Zoom, zoomed_by_the_system),
            [],
            "the system already zoomed the window: it is not put back"
        );
    }

    #[test]
    fn distraction_free_text_is_centred_at_the_right_margin() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();
        let font = EditorFont::default();
        let area = Rect::from_min_max(pos2(0.0, 0.0), pos2(3000.0, 800.0));
        let mut appearance = Appearance { mode: Some(Mode::Presentation), ..Appearance::default() };
        assert_eq!(appearance.editor_rect(&ctx, area, &font), area, "Presentation Mode keeps the whole width");
        appearance.mode = Some(Mode::DistractionFree);
        let centred = appearance.editor_rect(&ctx, area, &font);
        assert!(centred.width() < area.width() && centred.width() > 900.0, "{centred:?}");
        assert!((centred.center().x - area.center().x).abs() < 0.5);
        assert_eq!((centred.top(), centred.bottom()), (area.top(), area.bottom()));
        let narrow = Rect::from_min_max(pos2(0.0, 0.0), pos2(600.0, 800.0));
        assert_eq!(appearance.editor_rect(&ctx, narrow, &font), narrow, "a narrow window is left whole");
    }

    #[test]
    fn escape_is_left_to_a_focused_terminal_or_field() {
        let ctx = egui::Context::default();
        let mut text = String::new();
        let mut draw = |focus: bool| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let response = ui.text_edit_singleline(&mut text);
                if focus {
                    response.request_focus();
                }
            });
            output.textures_delta.clear();
        };
        draw(false);
        assert!(keyboard_free(&ctx, false), "nothing has the keyboard");
        draw(true);
        draw(false);
        assert!(!keyboard_free(&ctx, false), "the shell's Escape stays the shell's");
        assert!(keyboard_free(&ctx, true), "the editor's Escape leaves the mode");
    }

    #[test]
    fn only_presentation_mode_changes_the_font() {
        let settings = IdeSettings { presentation_font_size: 28.0, ..IdeSettings::default() };
        let mut appearance = Appearance::default();
        assert_eq!(appearance.editor_font(&settings).size, 13.0);
        for (mode, size, numbers) in
            [(Mode::Presentation, 28.0, false), (Mode::DistractionFree, 13.0, true), (Mode::Zen, 13.0, true)]
        {
            appearance.mode = Some(mode);
            assert_eq!(appearance.editor_font(&settings).size, size, "{mode:?}");
            assert_eq!(appearance.hides_line_numbers(), numbers, "{mode:?}");
            assert!(appearance.hides_chrome());
        }
    }
}
