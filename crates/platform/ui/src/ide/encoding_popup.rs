use egui::{Color32, Key, Modifiers, Pos2, Sense, Stroke, Ui, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::file_encodings::EncodingChange;

use super::{IdePalette, IdeView, dialog_frame};
use crate::theme;

const ROW: f32 = 22.0;
const VISIBLE: usize = 12;
const WIDTH: f32 = 240.0;

#[derive(Debug)]
pub(super) struct EncodingPopup {
    pub(super) query: String,
    charsets: Vec<String>,
    selected: usize,
    at: Pos2,
    focus: bool,
    fresh: bool,
}

impl EncodingPopup {
    fn new(charsets: Vec<String>, at: Pos2) -> Self {
        Self { query: String::new(), charsets, selected: 0, at, focus: true, fresh: true }
    }

    pub(super) fn matching(&self) -> Vec<&str> {
        let query = self.query.trim().to_lowercase();
        self.charsets.iter().map(String::as_str).filter(|charset| charset.to_lowercase().contains(&query)).collect()
    }
}

#[derive(Debug)]
pub(super) struct EncodingChoice {
    pub(super) charset: String,
    pub(super) change: EncodingChange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    Reload,
    Convert,
    Cancel,
}

impl IdeView {
    pub(super) fn open_encoding_popup(&mut self, registry: &Registry, session: &mut Session, at: Pos2) {
        let listed = registry.dispatch(session, "file.encoding.list", json!({})).ok();
        let charsets = listed
            .as_ref()
            .and_then(|answer| answer["charsets"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|charset| charset.as_str().map(str::to_owned))
            .collect();
        self.encoding_popup = Some(EncodingPopup::new(charsets, at));
    }

    pub(super) fn choose_encoding(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, charset: &str) {
        self.encoding_popup = None;
        let params = json!({ "charset": charset });
        match registry.dispatch(session, "file.encoding.set", params) {
            Ok(answer) => {
                if let Ok(change) = serde_json::from_value::<EncodingChange>(answer)
                    && !change.applied
                {
                    self.encoding_choice = Some(EncodingChoice { charset: charset.to_owned(), change });
                }
            }
            Err(error) => self.report(ui, error),
        }
    }

    pub(super) fn encoding_popups(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        self.encoding_list(ui, &p, registry, session);
        self.encoding_dialog(ui, &p, registry, session);
    }

    fn encoding_list(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session) {
        let Some(popup) = self.encoding_popup.as_mut() else { return };
        let (up, down, enter, escape) = ui.input_mut(|input| {
            (
                input.consume_key(Modifiers::NONE, Key::ArrowUp),
                input.consume_key(Modifiers::NONE, Key::ArrowDown),
                input.consume_key(Modifiers::NONE, Key::Enter),
                input.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        let shown: Vec<String> = popup.matching().into_iter().map(str::to_owned).collect();
        let count = shown.len();
        if count > 0 {
            popup.selected = popup.selected.min(count - 1);
            if up {
                popup.selected = popup.selected.checked_sub(1).unwrap_or(count - 1);
            }
            if down {
                popup.selected = (popup.selected + 1) % count;
            }
        }
        let height = ROW * VISIBLE.min(count.max(1)) as f32 + 40.0;
        let screen = ui.ctx().content_rect();
        let at = Pos2::new(
            popup.at.x.min(screen.right() - WIDTH - 12.0).max(screen.left() + 8.0),
            (popup.at.y - height - 8.0).max(screen.top() + 8.0),
        );
        let mut chosen = enter.then(|| shown.get(popup.selected).cloned()).flatten();
        let area = egui::Area::new(ui.scope_id().with("file-encoding-popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .show(ui.ctx(), |ui| {
                p.popup_frame().corner_radius(8).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                    ui.set_width(WIDTH);
                    ui.spacing_mut().item_spacing.y = 4.0;
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut popup.query)
                            .desired_width(f32::INFINITY)
                            .hint_text(t!("ide-encoding-search")),
                    );
                    crate::accessibility::name(&field, egui::Role::SearchInput, &t!("ide-encoding-search"));
                    if std::mem::take(&mut popup.focus) {
                        field.request_focus();
                    }
                    if field.changed() {
                        popup.selected = 0;
                    }
                    if shown.is_empty() {
                        ui.label(egui::RichText::new(t!("ide-encoding-nothing")).color(p.text_secondary));
                    }
                    egui::ScrollArea::vertical().max_height(ROW * VISIBLE as f32).auto_shrink([false, true]).show(
                        ui,
                        |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for (index, charset) in shown.iter().enumerate() {
                                let (rect, response) =
                                    ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                                if index == popup.selected {
                                    ui.painter().rect_filled(rect, 4, p.selection);
                                    if up || down {
                                        response.scroll_to_me(None);
                                    }
                                } else if response.hovered() {
                                    ui.painter().rect_filled(rect, 4, p.popup_hover);
                                }
                                crate::accessibility::name(&response, egui::Role::Button, charset);
                                ui.painter().text(
                                    egui::pos2(rect.left() + 8.0, rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    charset,
                                    theme::regular(13.0),
                                    p.text,
                                );
                                if response.clicked() {
                                    chosen = Some(charset.clone());
                                }
                            }
                        },
                    );
                });
            });
        let outside = !std::mem::take(&mut popup.fresh) && area.response.clicked_elsewhere();
        if let Some(charset) = chosen {
            self.choose_encoding(ui, registry, session, &charset);
        } else if escape || outside {
            self.encoding_popup = None;
        }
    }

    fn encoding_dialog(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session) {
        let Some(choice) = self.encoding_choice.as_ref() else { return };
        let mut answer = None;
        let modal =
            egui::Modal::new(ui.scope_id().with("file-encoding-choice")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
                ui.set_width(420.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let title = t!("ide-encoding-title", charset = choice.charset.as_str());
                ui.add(egui::Label::new(egui::RichText::new(title).font(theme::semibold(15.0)).color(p.text)).wrap());
                ui.add_space(10.0);
                let note = |ui: &mut Ui, text: String, color: Color32| {
                    ui.add(egui::Label::new(egui::RichText::new(text).font(theme::regular(13.0)).color(color)).wrap());
                    ui.add_space(6.0);
                };
                note(ui, t!("ide-encoding-reload-note"), p.text_secondary);
                note(ui, t!("ide-encoding-convert-note"), p.text_secondary);
                if let Some(reason) = &choice.change.unmappable {
                    note(ui, reason.clone(), p.error_text);
                }
                ui.add_space(14.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let button = |ui: &mut Ui, label: String, enabled: bool| {
                        let text = egui::RichText::new(label).font(theme::regular(13.0)).color(p.text);
                        let widget = egui::Button::new(text)
                            .min_size(vec2(84.0, 30.0))
                            .corner_radius(6)
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::new(1.0, p.button_border));
                        ui.add_enabled(enabled, widget).clicked()
                    };
                    if button(ui, t!("settings-cancel"), true) {
                        answer = Some(Answer::Cancel);
                    }
                    if button(ui, t!("ide-encoding-convert"), choice.change.can_convert) {
                        answer = Some(Answer::Convert);
                    }
                    if button(ui, t!("ide-encoding-reload"), choice.change.can_reload) {
                        answer = Some(Answer::Reload);
                    }
                });
            });
        let answer = answer.or_else(|| modal.should_close().then_some(Answer::Cancel));
        let Some(answer) = answer else { return };
        let charset = self.encoding_choice.take().map(|choice| choice.charset).unwrap_or_default();
        let action = match answer {
            Answer::Reload => "reload",
            Answer::Convert => "convert",
            Answer::Cancel => return,
        };
        self.dispatch(ui, registry, session, "file.encoding.set", json!({ "charset": charset, "action": action }));
    }
}
