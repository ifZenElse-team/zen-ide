use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use egui::{CornerRadius, Rect, Sense, Stroke, Ui, UiBuilder, Vec2, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_syntax::Highlighter;
use zen_workbench::Workbench;
use zen_workbench::language::breadcrumbs::BreadcrumbsState;
use zen_workbench::language::structure::StructureStatus;

use super::glyphs::symbol_badge;
use super::{IdePalette, IdeView, metrics};
use crate::theme;

const SETTLE: f64 = 0.3;
const PENDING_POLL: f64 = 0.2;
const LEFT: f32 = 10.0;
const INNER: f32 = 4.0;
const ICON: f32 = 14.0;
const ICON_GAP: f32 = 5.0;
const CHEVRON: f32 = 16.0;
const FONT: f32 = 13.0;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Key {
    path: PathBuf,
    version: u64,
    caret: usize,
}

enum Due {
    Now,
    Later(f64),
    Not,
}

#[derive(Debug, Default)]
pub(super) struct Breadcrumbs {
    state: Option<BreadcrumbsState>,
    seen: Option<Key>,
    edit: Option<(u64, f64)>,
    read_at: f64,
    drawn: Vec<(usize, Rect)>,
}

impl Breadcrumbs {
    fn due(&mut self, key: &Key, now: f64) -> Due {
        let Some(seen) = &self.seen else { return Due::Now };
        if seen.path != key.path {
            return Due::Now;
        }
        if seen.version != key.version {
            let edited_at = match self.edit {
                Some((version, at)) if version == key.version => at,
                _ => {
                    self.edit = Some((key.version, now));
                    now
                }
            };
            let left = SETTLE - (now - edited_at);
            return if left <= 0.0 { Due::Now } else { Due::Later(left) };
        }
        if seen.caret != key.caret {
            return Due::Now;
        }
        if self.state.as_ref().is_some_and(|state| state.status == StructureStatus::Pending) {
            let left = PENDING_POLL - (now - self.read_at);
            return if left <= 0.0 { Due::Now } else { Due::Later(left) };
        }
        Due::Not
    }

    fn clear(&mut self) {
        self.state = None;
        self.seen = None;
        self.edit = None;
        self.drawn.clear();
    }

    pub(super) fn prepare(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, enabled: bool) -> f32 {
        let key = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .filter(|document| document.image().is_none())
            .map(|document| Key {
                path: document.path().to_owned(),
                version: document.version(),
                caret: document.selections().primary().head,
            });
        let Some(key) = key.filter(|_| enabled) else {
            self.clear();
            return 0.0;
        };
        let now = ui.input(|input| input.time);
        match self.due(&key, now) {
            Due::Now => {
                self.state = registry
                    .dispatch(session, "editor.breadcrumbs.get", json!({ "path": key.path }))
                    .ok()
                    .and_then(|value| serde_json::from_value(value).ok());
                self.seen = Some(key);
                self.edit = None;
                self.read_at = now;
                if self.state.as_ref().is_some_and(|state| state.status == StructureStatus::Pending) {
                    ui.ctx().request_repaint_after(Duration::from_secs_f64(PENDING_POLL));
                }
            }
            Due::Later(left) => ui.ctx().request_repaint_after(Duration::from_secs_f64(left)),
            Due::Not => {}
        }
        if self.visible() { metrics::BREADCRUMBS } else { 0.0 }
    }

    fn visible(&self) -> bool {
        self.state.as_ref().is_some_and(|state| state.status != StructureStatus::Unavailable)
    }

    pub(super) fn show(&mut self, ui: &mut Ui, rect: Rect, p: &IdePalette) -> Option<(PathBuf, usize)> {
        self.drawn.clear();
        let state = self.state.as_ref()?;
        let painter = ui.painter_at(rect);
        let corners = CornerRadius { sw: metrics::ISLAND_RADIUS, se: metrics::ISLAND_RADIUS, ..CornerRadius::ZERO };
        if p.editor_background != p.island {
            painter.rect_filled(rect, corners, p.editor_background);
        }
        painter.hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, p.gutter_separator));
        let font = theme::regular(FONT);
        let mut x = rect.left() + LEFT;
        let mut clicked = None;
        for (index, crumb) in state.crumbs.iter().enumerate() {
            if x >= rect.right() {
                break;
            }
            if index > 0 {
                let chevron = Rect::from_center_size(pos2(x + CHEVRON / 2.0, rect.center().y), Vec2::splat(9.0));
                crate::icons::chevron_right(&painter, chevron, p.text_secondary);
                x += CHEVRON;
            }
            let galley = painter.layout_no_wrap(crumb.name.clone(), font.clone(), p.text);
            let width = INNER + ICON + ICON_GAP + galley.size().x + INNER;
            let item = Rect::from_min_size(pos2(x, rect.top() + 3.0), vec2(width, rect.height() - 6.0));
            let response =
                ui.interact(item.intersect(rect), ui.make_persistent_id(("breadcrumb", index)), Sense::click());
            crate::accessibility::button(&response, &crumb.name);
            if response.hovered() {
                painter.rect_filled(item, 4, p.hover);
            }
            let badge =
                Rect::from_center_size(pos2(item.left() + INNER + ICON / 2.0, item.center().y), Vec2::splat(ICON));
            symbol_badge(&painter, badge, crumb.kind, p);
            let text = pos2(badge.right() + ICON_GAP, item.center().y - galley.size().y / 2.0);
            painter.galley(text, galley, p.text);
            if response.clicked() {
                clicked = Some((state.path.clone(), crumb.selection_start));
            }
            self.drawn.push((crumb.selection_start, item));
            x = item.right();
        }
        clicked
    }
}

impl IdeView {
    pub(super) fn editor_with_breadcrumbs(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        highlighter: Option<Arc<dyn Highlighter>>,
        palette: &IdePalette,
    ) -> Option<(usize, usize)> {
        let height = if self.editor_view.passive {
            0.0
        } else {
            let shown = self.overview.settings.breadcrumbs && !self.appearance.hides_chrome();
            self.breadcrumbs.prepare(ui, registry, session, shown)
        };
        let full = ui.available_rect_before_wrap();
        let area = self.appearance.editor_rect(ui.ctx(), full, &self.editor_view.font);
        let (editor, bar) = area.split_top_bottom_at_y(area.bottom() - height);
        let mut child = ui.new_child(UiBuilder::new().id_salt("editor-area").max_rect(editor).layout(*ui.layout()));
        self.editor_view.square_bottom = height > 0.0;
        let caret = self.editor_view.show(&mut child, registry, session, highlighter, palette);
        ui.advance_cursor_after_rect(full);
        if height > 0.0
            && let Some((path, offset)) = self.breadcrumbs.show(ui, bar, palette)
        {
            self.dispatch(ui, registry, session, "navigate.place.goTo", json!({ "path": path, "offset": offset }));
            self.editor_view.focus();
        }
        caret
    }
}

#[cfg(test)]
mod tests {
    use egui::{Event, Modifiers, PointerButton, Pos2};
    use zen_syntax::{OutlineItem, Outliner, SymbolKind};
    use zen_workbench::language::structure::Outliners;

    use super::*;

    const SOURCE: &str = "package shop\n\ntype Cart struct {\n\titems []string\n}\n\nfunc (c *Cart) Add(item string) {\n\tc.items = append(c.items, item)\n}\n";

    fn at(needle: &str) -> usize {
        SOURCE.find(needle).unwrap()
    }

    struct CartOutliner;

    impl Outliner for CartOutliner {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".go")
        }

        fn outline(&self, _: &str) -> Vec<OutlineItem> {
            let method = OutlineItem {
                name: "Add".to_owned(),
                detail: None,
                kind: SymbolKind::Method,
                start: at("func (c"),
                end: SOURCE.len() - 1,
                selection_start: at("Add"),
                children: Vec::new(),
            };
            vec![OutlineItem {
                name: "Cart".to_owned(),
                detail: None,
                kind: SymbolKind::Struct,
                start: at("type Cart"),
                end: SOURCE.len() - 1,
                selection_start: at("Cart"),
                children: vec![method],
            }]
        }
    }

    struct Harness {
        ctx: egui::Context,
        registry: Registry,
        session: Session,
        bar: Breadcrumbs,
        height: f32,
        time: f64,
        _dir: tempfile::TempDir,
    }

    impl Harness {
        fn new() -> Self {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("cart.go");
            std::fs::write(&path, SOURCE).unwrap();
            let mut session = Session::new();
            session.state_mut::<Outliners>().set(vec![Arc::new(CartOutliner)]);
            registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
            Self { ctx, registry, session, bar: Breadcrumbs::default(), height: 0.0, time: 0.0, _dir: dir }
        }

        fn caret(&mut self, offset: usize) {
            self.registry
                .dispatch(
                    &mut self.session,
                    "editor.selection.set",
                    json!({ "selections": [{ "anchor": offset, "head": offset }] }),
                )
                .unwrap();
        }

        fn frame(&mut self, enabled: bool, events: Vec<Event>) -> Option<(PathBuf, usize)> {
            self.time += 0.1;
            let input = egui::RawInput {
                events,
                time: Some(self.time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
                ..Default::default()
            };
            let mut clicked = None;
            let mut output = self.ctx.run_ui(input, |ui| {
                self.height = self.bar.prepare(ui, &self.registry, &mut self.session, enabled);
                if self.height > 0.0 {
                    let bar = Rect::from_min_size(pos2(0.0, 400.0 - self.height), vec2(600.0, self.height));
                    clicked = self.bar.show(ui, bar, &IdePalette::zen_dark());
                }
            });
            output.textures_delta.clear();
            clicked
        }

        fn click(&mut self, at: Pos2) -> Option<(PathBuf, usize)> {
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(true, vec![Event::PointerMoved(at)]);
            self.frame(true, vec![button(true)]);
            self.frame(true, vec![button(false)])
        }

        fn view_frame(&mut self, view: &mut IdeView, events: Vec<Event>) {
            self.time += 0.1;
            let input = egui::RawInput {
                events,
                time: Some(self.time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
                ..Default::default()
            };
            let mut output = self.ctx.run_ui(input, |ui| {
                view.editor_with_breadcrumbs(ui, &self.registry, &mut self.session, None, &IdePalette::zen_dark());
            });
            output.textures_delta.clear();
        }

        fn names(&self) -> Vec<String> {
            self.bar.state.iter().flat_map(|state| state.crumbs.iter().map(|crumb| crumb.name.clone())).collect()
        }
    }

    #[test]
    fn the_bar_paints_the_crumbs_at_the_caret_and_a_click_moves_the_caret() {
        let mut harness = Harness::new();
        harness.caret(at("append"));
        harness.frame(true, Vec::new());
        assert_eq!(harness.height, metrics::BREADCRUMBS);
        assert_eq!(harness.names(), ["Cart", "Add"]);
        assert_eq!(harness.bar.drawn.len(), 2);
        let (first, second) = (harness.bar.drawn[0].1, harness.bar.drawn[1].1);
        assert!(first.right() < second.left(), "a chevron separates the crumbs");
        assert!(first.top() >= 400.0 - metrics::BREADCRUMBS);

        let (path, offset) = harness.click(first.center()).expect("the crumb is clicked");
        assert_eq!(offset, at("Cart"));
        harness
            .registry
            .dispatch(&mut harness.session, "navigate.place.goTo", json!({ "path": path, "offset": offset }))
            .unwrap();
        let caret =
            harness.session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary().head;
        assert_eq!(caret, at("Cart"));
        harness.frame(true, Vec::new());
        assert_eq!(harness.names(), ["Cart"], "the crumbs follow the caret");
    }

    #[test]
    fn an_edit_is_read_once_it_settles_and_the_setting_hides_the_bar() {
        let mut harness = Harness::new();
        harness.caret(at("append"));
        harness.frame(true, Vec::new());
        let read_at = harness.bar.read_at;
        harness.registry.dispatch(&mut harness.session, "editor.edit.type", json!({ "text": "x" })).unwrap();
        harness.frame(true, Vec::new());
        harness.frame(true, Vec::new());
        assert_eq!(harness.bar.read_at, read_at, "not while typing");
        for _ in 0..3 {
            harness.frame(true, Vec::new());
        }
        assert!(harness.bar.read_at > read_at, "once the edits settle");
        assert_eq!(harness.names(), ["Cart", "Add"]);

        harness.frame(false, Vec::new());
        assert_eq!(harness.height, 0.0);
        assert!(harness.bar.drawn.is_empty() && harness.bar.state.is_none());
        harness.frame(true, Vec::new());
        assert_eq!(harness.height, metrics::BREADCRUMBS);
    }

    #[test]
    fn a_click_on_a_crumb_gives_the_editor_the_keyboard_and_the_bar_takes_the_rounded_corners() {
        let mut harness = Harness::new();
        harness.caret(at("append"));
        let product = super::super::IdeProduct {
            name: "Test",
            id: "test",
            version: "0.0.0",
            highlighters: Vec::new(),
            plugins: None,
            mcp_port: 0,
            modules: Vec::new(),
            areas: Vec::new(),
            settings_pages: Vec::new(),
            code_styles: Vec::new(),
            folding_options: Vec::new(),
            debugger: None,
            project_models: Vec::new(),
            outliners: Vec::new(),
            run_providers: Vec::new(),
            language_servers: Vec::new(),
            templates: Vec::new(),
            inspectors: Vec::new(),
        };
        let mut view = IdeView::new(Arc::new(product), crate::plugins::Plugins::new(None));
        harness.view_frame(&mut view, Vec::new());
        harness.view_frame(&mut view, Vec::new());
        assert!(view.editor_view.square_bottom, "the editor leaves its bottom corners to the bar");
        assert!(view.editor_view.has_focus_in(&harness.ctx));
        let crumb = view.breadcrumbs.drawn[0].1;
        assert!(crumb.top() >= 400.0 - metrics::BREADCRUMBS);

        harness.ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        harness.view_frame(&mut view, Vec::new());
        assert!(!view.editor_view.has_focus_in(&harness.ctx));
        let button = |pressed| Event::PointerButton {
            pos: crumb.center(),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        harness.view_frame(&mut view, vec![Event::PointerMoved(crumb.center())]);
        harness.view_frame(&mut view, vec![button(true)]);
        harness.view_frame(&mut view, vec![button(false)]);
        harness.view_frame(&mut view, Vec::new());
        let caret =
            harness.session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary().head;
        assert_eq!(caret, at("Cart"));
        assert!(view.editor_view.has_focus_in(&harness.ctx), "the click gives the editor the keyboard");

        view.overview.settings.breadcrumbs = false;
        harness.view_frame(&mut view, Vec::new());
        assert!(!view.editor_view.square_bottom, "without the bar the editor rounds its corners");
        assert!(view.breadcrumbs.drawn.is_empty());
    }
}
