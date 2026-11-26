use std::sync::Arc;

use egui::{Align2, Color32, Key, Modifiers, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde::Deserialize;
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_syntax::{Highlighter, Span};

use super::recent_files::check_option;
use super::search_everywhere::search_field;
use super::{IdePalette, dialog_frame};
use crate::{accessibility, theme};

const HEADER: f32 = 24.0;
const LINE: f32 = 17.0;

#[derive(Clone, Debug, Deserialize)]
struct SnippetLine {
    line: u32,
    text: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Location {
    path: String,
    relative: String,
    offset: usize,
    line: u32,
    time: u64,
    #[serde(default)]
    breadcrumbs: Vec<String>,
    #[serde(default)]
    snippet: Vec<SnippetLine>,
}

struct Entry {
    location: Location,
    spans: Vec<Span>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Pick {
    pub path: String,
    pub offset: usize,
}

#[derive(Default)]
pub(super) struct RecentLocations {
    open: bool,
    query: String,
    edited_only: bool,
    selected: usize,
    focus: bool,
    entries: Vec<Entry>,
    read_for: Option<(String, bool)>,
}

impl RecentLocations {
    #[cfg(test)]
    pub(super) fn typed(&self) -> &str {
        &self.query
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn edited_only(&self) -> bool {
        self.edited_only
    }

    pub(super) fn press(&mut self) {
        if self.open {
            self.edited_only = !self.edited_only;
            self.selected = 0;
        } else {
            self.open_with(false);
        }
    }

    pub(super) fn open_with(&mut self, edited_only: bool) {
        *self = Self { open: true, edited_only, focus: true, ..Self::default() };
    }

    fn read(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        highlighter: &dyn Fn(&std::path::Path) -> Option<Arc<dyn Highlighter>>,
    ) {
        let wanted = (self.query.clone(), self.edited_only);
        if self.read_for.as_ref() == Some(&wanted) {
            return;
        }
        let params = json!({ "query": self.query, "editedOnly": self.edited_only });
        let found = registry.dispatch(session, "navigate.recent.locations", params).ok().unwrap_or_default();
        let locations: Vec<Location> = serde_json::from_value(found).unwrap_or_default();
        self.entries = locations
            .into_iter()
            .map(|location| {
                let code = code_of(&location);
                let spans = highlighter(std::path::Path::new(&location.path))
                    .map(|highlighter| highlighter.highlight(&code))
                    .unwrap_or_default();
                Entry { location, spans }
            })
            .collect();
        self.read_for = Some(wanted);
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        palette: &IdePalette,
        highlighter: &dyn Fn(&std::path::Path) -> Option<Arc<dyn Highlighter>>,
    ) -> Option<Pick> {
        if !self.open {
            return None;
        }
        self.read(registry, session, highlighter);

        let (delete, backspace) = ui.input_mut(|input| {
            let backspace = self.query.is_empty() && input.consume_key(Modifiers::NONE, Key::Backspace);
            (input.consume_key(Modifiers::NONE, Key::Delete), backspace)
        });
        if (delete || backspace)
            && let Some(entry) = self.entries.get(self.selected)
        {
            let params = json!({ "path": entry.location.path, "line": entry.location.line });
            let _ = registry.dispatch(session, "navigate.recent.removeLocation", params);
            self.read_for = None;
            self.read(registry, session, highlighter);
        }

        let mut pick: Option<usize> = None;
        let mut close = false;
        let now = zen_workbench::recent::now();
        let screen = ui.ctx().content_rect();
        let width = (screen.width() * 0.55).clamp(560.0, 760.0);
        let list_height = (screen.height() * 0.6).clamp(240.0, 640.0);
        let area = egui::Area::new(ui.scope_id().with("recent-locations"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(screen.center().x - width / 2.0, screen.top() + screen.height() * 0.12));
        let response = area.show(ui.ctx(), |ui| {
            dialog_frame(palette).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.set_width(width - 20.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
                let title = t!("ide-recent-locations-title", count = self.entries.len());
                let shown = ui.painter().text(
                    pos2(bar.left(), bar.center().y),
                    Align2::LEFT_CENTER,
                    &title,
                    theme::semibold(13.0),
                    palette.text,
                );
                accessibility::heading(ui, ui.scope_id().with("recent-locations-title"), shown, || title.clone());
                let toggle = check_option(
                    ui,
                    bar.right_center(),
                    self.edited_only,
                    &t!("ide-recent-edited-only"),
                    "⇧⌘E",
                    palette,
                );
                if toggle.clicked() {
                    self.edited_only = !self.edited_only;
                    self.selected = 0;
                    self.focus = true;
                }
                ui.add_space(8.0);
                let hint = t!("ide-recent-locations-hint");
                let field = search_field(ui, &mut self.query, &hint, palette);
                if std::mem::take(&mut self.focus) {
                    field.request_focus();
                }
                if field.changed() {
                    self.selected = 0;
                }
                super::filter_popups::take_typing(ui);
                let (up, down, enter, escape) = ui.input_mut(|input| {
                    (
                        input.consume_key(Modifiers::NONE, Key::ArrowUp),
                        input.consume_key(Modifiers::NONE, Key::ArrowDown),
                        input.consume_key(Modifiers::NONE, Key::Enter),
                        input.consume_key(Modifiers::NONE, Key::Escape),
                    )
                });
                if up {
                    self.selected = self.selected.saturating_sub(1);
                }
                if down && self.selected + 1 < self.entries.len() {
                    self.selected += 1;
                }
                if enter && self.selected < self.entries.len() {
                    pick = Some(self.selected);
                }
                if escape {
                    close = true;
                }
                if !field.has_focus() && !close && pick.is_none() {
                    field.request_focus();
                }
                ui.add_space(6.0);
                if self.entries.is_empty() {
                    let message = if self.edited_only {
                        t!("ide-recent-locations-no-edited")
                    } else {
                        t!("ide-recent-locations-empty")
                    };
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 60.0), Sense::hover());
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        message,
                        theme::regular(13.0),
                        palette.text_secondary,
                    );
                    return;
                }
                super::filter_popups::list(ui, "recent-locations-rows", list_height, |ui| {
                    for (index, entry) in self.entries.iter().enumerate() {
                        let height = HEADER + LINE * entry.location.snippet.len().max(1) as f32 + 14.0;
                        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::CLICK);
                        let selected = index == self.selected;
                        if selected {
                            ui.painter().rect_filled(rect, 6, palette.selection);
                            if up || down {
                                response.scroll_to_me(None);
                            }
                        } else if response.hovered() {
                            ui.painter().rect_filled(rect, 6, palette.popup_hover);
                        }
                        let name = entry.location.relative.rsplit('/').next().unwrap_or(&entry.location.relative);
                        accessibility::item(&response, egui::Role::ListBoxOption, selected, name);
                        accessibility::describe(&response, &entry.location.breadcrumbs.join(" › "));
                        draw_entry(ui, rect, entry, now, palette);
                        if response.clicked() {
                            pick = Some(index);
                        }
                    }
                });
            });
        });
        if ui.input(|input| input.pointer.any_pressed())
            && let Some(pointer) = ui.input(|input| input.pointer.interact_pos())
            && !response.response.rect.contains(pointer)
        {
            close = true;
        }
        let picked = pick
            .and_then(|index| self.entries.get(index))
            .map(|entry| Pick { path: entry.location.path.clone(), offset: entry.location.offset });
        if close || picked.is_some() {
            self.open = false;
            self.entries.clear();
            self.read_for = None;
        }
        picked
    }
}

fn code_of(location: &Location) -> String {
    location.snippet.iter().map(|line| line.text.as_str()).collect::<Vec<_>>().join("\n")
}

fn draw_entry(ui: &Ui, rect: Rect, entry: &Entry, now: u64, palette: &IdePalette) {
    let location = &entry.location;
    let header = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER));
    let name = location.relative.rsplit('/').next().unwrap_or(&location.relative);
    crate::file_icons::file_icon_at(
        ui.painter(),
        Rect::from_center_size(pos2(header.left() + 16.0, header.center().y), Vec2::splat(16.0)),
        std::path::Path::new(&location.path),
    );
    let ago = ui.painter().text(
        pos2(header.right() - 10.0, header.center().y),
        Align2::RIGHT_CENTER,
        ago(location.time, now),
        theme::regular(12.0),
        palette.text_secondary,
    );
    let painter = ui.painter().with_clip_rect(Rect::from_min_max(header.min, pos2(ago.left() - 10.0, header.bottom())));
    let label = painter.text(
        pos2(header.left() + 32.0, header.center().y),
        Align2::LEFT_CENTER,
        name,
        theme::regular(13.5),
        palette.text,
    );
    if !location.breadcrumbs.is_empty() {
        painter.text(
            pos2(label.right() + 10.0, header.center().y),
            Align2::LEFT_CENTER,
            location.breadcrumbs.join(" › "),
            theme::regular(12.5),
            palette.text_secondary,
        );
    }
    let code =
        Rect::from_min_max(pos2(rect.left() + 8.0, header.bottom()), pos2(rect.right() - 8.0, rect.bottom() - 6.0));
    ui.painter().rect_filled(code, 5, palette.editor_background);
    let number_font = egui::FontId::monospace(12.0);
    let digits = location.snippet.last().map_or(1, |line| line.line.to_string().len());
    let gutter = ui.fonts_mut(|fonts| fonts.glyph_width(&number_font, '0')) * digits as f32 + 16.0;
    let clip = ui.painter().with_clip_rect(code);
    let mut start = 0;
    for (row, line) in location.snippet.iter().enumerate() {
        let y = code.top() + 4.0 + row as f32 * LINE;
        let number_color = if line.line == location.line { palette.text } else { palette.gutter_text };
        clip.text(
            pos2(code.left() + gutter - 8.0, y + LINE / 2.0),
            Align2::RIGHT_CENTER,
            line.line.to_string(),
            number_font.clone(),
            number_color,
        );
        let job = super::diff_view::line_job(&line.text, start, &entry.spans, &[], palette, Color32::TRANSPARENT);
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        clip.galley(pos2(code.left() + gutter, y + (LINE - galley.size().y) / 2.0), galley, palette.code);
        start += line.text.len() + 1;
    }
}

pub(super) fn ago(time: u64, now: u64) -> String {
    let seconds = now.saturating_sub(time);
    match seconds {
        0..60 => t!("ide-recent-moments"),
        60..3_600 => t!("ide-recent-minutes", count = seconds / 60),
        3_600..86_400 => t!("ide-recent-hours", count = seconds / 3_600),
        _ => t!("ide-recent-days", count = seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use egui::{Event, Pos2};
    use zen_workbench::Workbench;

    use super::*;

    const MAIN: &str = "package main\n\nfunc main() {\n\tprintln(\"hi\")\n}\n\nfunc helper() int {\n\treturn 1\n}\n";

    struct Harness {
        ctx: egui::Context,
        registry: Registry,
        session: Session,
        popup: RecentLocations,
        picked: Vec<Pick>,
        left: Vec<Key>,
        dir: tempfile::TempDir,
    }

    impl Harness {
        fn new() -> Self {
            zen_i18n::register(crate::toolbox::LOCALES);
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("main.go"), MAIN).unwrap();
            std::fs::write(dir.path().join("util.go"), "package main\n\nvar answer = 42\n").unwrap();
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let mut session = Session::new();
            session.set_state(Workbench::default());
            registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
            let main = dir.path().join("main.go");
            let helper = MAIN.find("return 1").unwrap();
            registry.dispatch(&mut session, "navigate.place.goTo", json!({ "path": main, "offset": helper })).unwrap();
            let util = dir.path().join("util.go");
            registry.dispatch(&mut session, "navigate.place.goTo", json!({ "path": util, "offset": 15 })).unwrap();
            Self {
                ctx,
                registry,
                session,
                popup: RecentLocations::default(),
                picked: Vec::new(),
                left: Vec::new(),
                dir,
            }
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = egui::RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))),
                ..Default::default()
            };
            let mut output = self.ctx.run_ui(input, |ui| {
                self.picked.extend(self.popup.show(
                    ui,
                    &self.registry,
                    &mut self.session,
                    &IdePalette::zen_dark(),
                    &|_| None,
                ));
                let keys = [Key::Enter, Key::Escape, Key::ArrowUp, Key::ArrowDown];
                self.left.extend(keys.into_iter().filter(|key| ui.input(|input| input.key_pressed(*key))));
            });
            output.textures_delta.clear();
        }

        fn key(&mut self, key: Key) {
            self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }]);
        }

        fn names(&self) -> Vec<String> {
            self.popup.entries.iter().map(|entry| entry.location.relative.clone()).collect()
        }
    }

    #[test]
    fn lists_the_places_newest_first_and_enter_goes_to_the_selected_one() {
        let mut harness = Harness::new();
        harness.popup.press();
        harness.frame(Vec::new());
        assert_eq!(harness.names(), ["util.go", "main.go"]);
        assert_eq!(harness.popup.entries[1].location.snippet.len(), 5);
        harness.key(Key::ArrowDown);
        harness.key(Key::Enter);
        let main = harness.dir.path().join("main.go").to_string_lossy().into_owned();
        assert_eq!(harness.picked, [Pick { path: main, offset: MAIN.find("return 1").unwrap() }]);
        assert!(!harness.popup.is_open());
    }

    #[test]
    fn the_keys_it_handles_never_reach_the_editor() {
        let mut harness = Harness::new();
        harness.popup.press();
        harness.frame(Vec::new());
        harness.key(Key::ArrowDown);
        harness.key(Key::ArrowUp);
        harness.key(Key::Enter);
        assert_eq!(harness.picked.len(), 1);
        harness.popup.press();
        harness.frame(Vec::new());
        harness.key(Key::Escape);
        assert!(!harness.popup.is_open());
        assert_eq!(harness.left, [], "the editor drawn after the popup would break the line at the place");
    }

    #[test]
    fn typing_filters_and_backspace_with_nothing_typed_removes_the_place() {
        let mut harness = Harness::new();
        harness.popup.press();
        harness.frame(Vec::new());
        harness.frame(vec![Event::Text("answer".to_owned())]);
        harness.frame(Vec::new());
        assert_eq!(harness.names(), ["util.go"]);
        harness.popup.query.clear();
        harness.frame(Vec::new());
        assert_eq!(harness.names().len(), 2);
        harness.key(Key::Backspace);
        assert_eq!(harness.names(), ["main.go"], "the selected place is gone");
        let history = harness.registry.dispatch(&mut harness.session, "navigate.history.list", json!({})).unwrap();
        assert!(history["back"].as_array().unwrap().iter().all(|spot| spot["line"] != 3), "{history}");
    }

    #[test]
    fn shift_command_e_again_shows_the_edited_places_only() {
        let mut harness = Harness::new();
        harness.registry.dispatch(&mut harness.session, "editor.edit.insert", json!({ "text": "x" })).unwrap();
        harness.popup.press();
        harness.frame(Vec::new());
        assert_eq!(harness.names().len(), 2, "one place per line: the edit is on the line visited");
        harness.popup.press();
        assert!(harness.popup.edited_only());
        harness.frame(Vec::new());
        assert_eq!(harness.names(), ["util.go"]);
    }

    #[test]
    fn times_read_as_moments_minutes_hours_and_days_ago() {
        zen_i18n::register(crate::toolbox::LOCALES);
        assert_eq!(ago(100, 130), t!("ide-recent-moments"));
        assert_eq!(ago(0, 180), t!("ide-recent-minutes", count = 3));
        assert_eq!(ago(0, 7_200), t!("ide-recent-hours", count = 2));
        assert_eq!(ago(0, 86_400 * 3), t!("ide-recent-days", count = 3));
    }
}
