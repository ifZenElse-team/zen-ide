use egui::text::{LayoutJob, LayoutSection, TextFormat};
use egui::{FontId, Ui};
use zen_db::format::{Commas, FormatOptions, KeywordCase};
use zen_i18n::t;
use zen_project::settings::DEFAULT_LANGUAGE;
use zen_text::LineSeparator;
use zen_workbench::build_sync::ReloadAfter;
use zen_workbench::language::inlay_hints::HintKind;
use zen_workbench::line_numbers::LineNumbersMode;
use zen_workbench::on_save::TrailingSpaces;
use zen_workbench::visual_aids::TabRendering;

use super::code_style_schemes::{SchemeAction, SchemeEdit, guides_text, parsed_guides, parsed_patterns};
use super::model::{CodeStyleScheme, FoldArrows, IdeSettings, WheelFontSize};
use super::page::{PageId, SectionDef};
use super::{Dispatch, SettingsDialog, widgets};
use crate::ide::{IdePalette, metrics};
use crate::{fonts, theme};
use zen_project::editorconfig::FILE_NAME;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ide) struct ExportOutcome {
    pub(in crate::ide) message: String,
    pub(in crate::ide) failed: bool,
    pub(in crate::ide) replace: bool,
}

impl ExportOutcome {
    fn of(answer: Result<serde_json::Value, String>) -> Self {
        match answer {
            Ok(exported) => Self {
                message: t!("settings-code-style-exported", path = exported["path"].as_str().unwrap_or_default()),
                failed: false,
                replace: false,
            },
            Err(error) => Self { message: error, failed: true, replace: false },
        }
    }
}

const SAMPLE: &str = "package main\n\nimport \"fmt\"\n\n// main counts to three.\nfunc main() {\n\tfor i := 0; i != 3; i++ {\n\t\tfmt.Println(\"step\", i)\n\t}\n}";

pub(super) const WHEEL_FONT_SIZE: &str =
    if cfg!(target_os = "macos") { "settings-editor-wheel-font-size" } else { "settings-editor-wheel-font-size-ctrl" };

const OTHER_FILE_TYPES: &[(&str, &str)] = &[
    (DEFAULT_LANGUAGE, "settings-code-style-other"),
    ("json", "settings-code-style-json"),
    ("yaml", "settings-code-style-yaml"),
    ("toml", "settings-code-style-toml"),
    ("markdown", "settings-code-style-markdown"),
];

impl SettingsDialog {
    pub(super) fn editor_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        widgets::group_title(ui, p, &t!("settings-editor-mouse"), query);
        widgets::checkbox(ui, p, &mut draft.wheel_font_size, &t!(WHEEL_FONT_SIZE), query);
        ui.add_enabled_ui(draft.wheel_font_size, |ui| {
            for (scope, key) in [
                (WheelFontSize::ActiveEditor, "settings-editor-wheel-active"),
                (WheelFontSize::AllEditors, "settings-editor-wheel-all"),
            ] {
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.radio_value(&mut draft.wheel_font_size_in, scope, widgets::label_job(ui, p, &t!(key), query));
                });
            }
        });
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-editor-smart-keys"), query);
        widgets::checkbox(ui, p, &mut draft.camel_humps, &t!("settings-editor-camel-humps"), query);
        ui.add_enabled_ui(draft.camel_humps, |ui| {
            ui.indent("settings-editor-camel-humps-double-click", |ui| {
                let label = t!("settings-editor-camel-humps-double-click");
                widgets::checkbox(ui, p, &mut draft.camel_humps_on_double_click, &label, query);
            });
        });
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-editor-smart-keys-web"), query);
        widgets::checkbox(ui, p, &mut draft.closing_tags, &t!("settings-editor-closing-tags"), query);
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-editor-gutter"), query);
        widgets::checkbox(ui, p, &mut draft.gutter_run_icons, &t!("settings-editor-run-icons"), query);
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-editor-caret-movement"), query);
        widgets::checkbox(ui, p, &mut draft.highlight_usages_at_caret, &t!("settings-editor-highlight-usages"), query);
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-editor-soft-wraps"), query);
        widgets::text_field(ui, p, &t!("settings-editor-soft-wrap-files"), query, &mut draft.soft_wrap_files);
        ui.add_space(14.0);
        appearance_group(ui, p, draft, query);
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-editor-breadcrumbs"), query);
        widgets::checkbox(ui, p, &mut draft.breadcrumbs, &t!("settings-editor-breadcrumbs-show"), query);
        widgets::group_title(ui, p, &t!("settings-editor-sticky"), query);
        widgets::checkbox(ui, p, &mut draft.sticky_lines, &t!("settings-editor-sticky-show"), query);
        ui.add_enabled_ui(draft.sticky_lines, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add_space(24.0);
                let title = ui.label(label(ui, p, &t!("settings-editor-sticky-max"), query));
                ui.add(egui::DragValue::new(&mut draft.sticky_lines_max).range(1..=zen_workbench::sticky::MAX_LIMIT))
                    .labelled_by(title.id);
            });
        });
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-editor-limits"), query);
        widgets::number_field(
            ui,
            p,
            &t!("settings-editor-clipboard-limit"),
            query,
            &mut draft.clipboard_limit,
            1..=zen_workbench::clipboard_history::MAX_LIMIT,
        );
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-editor-console"), query);
        let override_label = t!("settings-editor-console-buffer-override");
        widgets::checkbox(ui, p, &mut draft.console_buffer_override, &override_label, query);
        ui.add_enabled_ui(draft.console_buffer_override, |ui| {
            ui.indent("settings-editor-console-buffer-kb", |ui| {
                widgets::number_field(
                    ui,
                    p,
                    &t!("settings-editor-console-buffer-kb"),
                    query,
                    &mut draft.console_buffer_kb,
                    1..=zen_run::MAX_BUFFER_KB,
                );
            });
        });
        ui.add_space(14.0);
        on_save_group(ui, p, draft, query);
    }

    pub(super) fn general_page(&mut self, ui: &mut Ui, p: &IdePalette, sections: &[SectionDef]) {
        let Some(at) = sections.iter().position(|section| section.page == PageId::EditorGeneral) else { return };
        let level = sections[at].level;
        for section in sections[at + 1..].iter().take_while(|section| section.level > level) {
            if section.level == level + 1 && ui.link(egui::RichText::new(t!(section.key)).color(p.link)).clicked() {
                self.section = section.page;
            }
            ui.add_space(4.0);
        }
    }

    pub(super) fn code_folding_page(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        query: Option<&str>,
        folding: &[zen_modules::folding::FoldingOptions],
    ) {
        let Some(draft) = self.draft.as_mut() else { return };
        ui.horizontal_wrapped(|ui| {
            widgets::checkbox(ui, p, &mut draft.show_fold_arrows, &t!("settings-folding-show-arrows"), query);
            ui.add_enabled_ui(draft.show_fold_arrows, |ui| {
                let name = |arrows: FoldArrows| match arrows {
                    FoldArrows::Always => t!("settings-folding-arrows-always"),
                    FoldArrows::OnHover => t!("settings-folding-arrows-hover"),
                };
                let combo = egui::ComboBox::from_id_salt("settings-folding-arrows")
                    .selected_text(name(draft.fold_arrows))
                    .show_ui(ui, |ui| {
                        for arrows in [FoldArrows::Always, FoldArrows::OnHover] {
                            ui.selectable_value(&mut draft.fold_arrows, arrows, name(arrows));
                        }
                    });
                crate::accessibility::name(&combo.response, egui::Role::ComboBox, &t!("settings-folding-show-arrows"));
            });
        });
        ui.add_space(4.0);
        widgets::checkbox(ui, p, &mut draft.fold_bottom_arrows, &t!("settings-folding-bottom-arrows"), query);
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-folding-by-default"), query);
        let general = [
            (&mut draft.fold_file_header, "settings-folding-file-header"),
            (&mut draft.fold_imports, "settings-folding-imports"),
            (&mut draft.fold_doc_comments, "settings-folding-doc-comments"),
            (&mut draft.fold_method_bodies, "settings-folding-method-bodies"),
            (&mut draft.fold_custom_regions, "settings-folding-custom-regions"),
        ];
        options_group(ui, p, query, "settings-folding-general", |ui| {
            for (value, label) in general {
                widgets::checkbox(ui, p, value, &t!(label), query);
                ui.add_space(2.0);
            }
        });
        for group in folding {
            let declared = &mut draft.declared_settings;
            options_group(ui, p, query, group.title, |ui| {
                for option in group.options {
                    let mut value = declared.get(option.key).copied().unwrap_or(option.default);
                    if widgets::checkbox(ui, p, &mut value, &t!(option.label), query).changed() {
                        declared.insert(option.key.to_owned(), value);
                    }
                    ui.add_space(2.0);
                }
            });
        }
    }

    pub(super) fn inlay_hints_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        let hints = &mut draft.inlay_hints;
        widgets::checkbox(ui, p, &mut hints.enabled, &t!("settings-inlay-hints-show"), query);
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-inlay-hints-kinds"), query);
        let enabled = hints.enabled;
        ui.add_enabled_ui(enabled, |ui| {
            for kind in HintKind::ALL {
                widgets::checkbox(ui, p, hints.kind_mut(kind), &t!(kind_key(kind)), query);
                ui.add_space(4.0);
            }
        });
        ui.add_space(10.0);
        ui.label(egui::RichText::new(t!("settings-inlay-hints-note")).color(p.text_secondary));
    }

    pub(super) fn code_vision_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        let vision = &mut draft.code_vision;
        widgets::checkbox(ui, p, &mut vision.enabled, &t!("settings-code-vision-show"), query);
        ui.add_space(10.0);
        ui.add_enabled_ui(vision.enabled, |ui| {
            widgets::checkbox(ui, p, &mut vision.usages, &t!("settings-code-vision-usages"), query);
            ui.add_space(4.0);
            widgets::checkbox(ui, p, &mut vision.authors, &t!("settings-code-vision-authors"), query);
        });
        ui.add_space(10.0);
        ui.label(egui::RichText::new(t!("settings-code-vision-note")).color(p.text_secondary));
    }

    pub(super) fn vcs_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        widgets::group_title(ui, p, &t!("settings-vcs-workspace"), query);
        widgets::checkbox(ui, p, &mut draft.git_avatars, &t!("settings-vcs-avatars"), query);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            let hosts = zen_workbench::vcs::avatars::AVATAR_HOSTS.join(", ");
            let note = egui::RichText::new(t!("settings-vcs-avatars-note", hosts = hosts)).color(p.text_secondary);
            ui.add(egui::Label::new(note).wrap());
        });
    }

    pub(super) fn build_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        widgets::group_title(ui, p, &t!("settings-build-tools"), query);
        ui.label(widgets::label_job(ui, p, &t!("settings-build-reload-after"), query));
        for (choice, key) in
            [(ReloadAfter::Any, "settings-build-reload-any"), (ReloadAfter::External, "settings-build-reload-external")]
        {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.radio_value(&mut draft.build_reload_after, choice, widgets::label_job(ui, p, &t!(key), query));
            });
        }
    }

    pub(super) fn advanced_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        widgets::group_title(ui, p, &t!("settings-advanced-editor"), query);
        widgets::checkbox(ui, p, &mut draft.zoom_indicator, &t!("settings-advanced-zoom-indicator"), query);
        let rendering = &mut draft.editor_appearance.tab_rendering;
        ui.horizontal_wrapped(|ui| {
            let title = ui.label(label(ui, p, &t!("settings-advanced-tab-rendering"), query));
            let combo = egui::ComboBox::from_id_salt("settings-tab-rendering")
                .selected_text(tab_rendering_name(*rendering))
                .width(widgets::field_width(ui, 140.0, 240.0))
                .show_ui(ui, |ui| {
                    for choice in [TabRendering::Line, TabRendering::Arrow] {
                        ui.selectable_value(rendering, choice, tab_rendering_name(choice));
                    }
                });
            combo.response.labelled_by(title.id);
        });
        let whitespace = t!("settings-advanced-extend-whitespace");
        widgets::checkbox(ui, p, &mut draft.extend_selection_whitespace, &whitespace, query);
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-advanced-editor-tabs"), query);
        let detached = t!("settings-advanced-detached-open-in-main");
        widgets::checkbox(ui, p, &mut draft.open_declaration_in_main, &detached, query);
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-advanced-bookmarks"), query);
        widgets::checkbox(ui, p, &mut draft.bookmarks_popup_line_only, &t!("settings-advanced-popup-line-only"), query);
        ui.add_space(12.0);
        widgets::group_title(ui, p, &t!("settings-advanced-debugger"), query);
        widgets::number_field(
            ui,
            p,
            &t!("settings-advanced-recent-expressions"),
            query,
            &mut draft.recent_expressions,
            1..=1000,
        );
        widgets::checkbox(ui, p, &mut draft.run_to_cursor_inlay, &t!("settings-advanced-run-to-cursor-inlay"), query);
        ui.add_space(12.0);
        widgets::group_title(ui, p, &t!("settings-advanced-search-everywhere"), query);
        widgets::checkbox(ui, p, &mut draft.search_wait_for_all, &t!("settings-advanced-wait-for-all"), query);
        widgets::checkbox(ui, p, &mut draft.search_text_on_all, &t!("settings-advanced-text-on-all"), query);
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-advanced-editor-tabs"), query);
        let same_tab = t!("settings-advanced-declaration-same-tab");
        widgets::checkbox(ui, p, &mut draft.editor_tabs.declaration_in_same_tab, &same_tab, query);
        widgets::checkbox(ui, p, &mut draft.editor_tabs.keep_pinned_left, &t!("settings-advanced-pinned-left"), query);
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-advanced-find-in-files"), query);
        widgets::number_field(
            ui,
            p,
            &t!("settings-advanced-fif-limit"),
            query,
            &mut draft.find_in_files_limit,
            1..=zen_workbench::search::MAX_POPUP_LIMIT,
        );
        ui.add_space(6.0);
        widgets::number_field(
            ui,
            p,
            &t!("settings-advanced-fif-too-many"),
            query,
            &mut draft.find_in_files_too_many,
            1..=zen_workbench::search::MAX_TOO_MANY,
        );
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-advanced-run-debug"), query);
        widgets::checkbox(ui, p, &mut draft.run_confirm_rerun, &t!("settings-advanced-confirm-rerun"), query);
        ui.add_space(6.0);
        widgets::number_field(
            ui,
            p,
            &t!("settings-advanced-temporary-limit"),
            query,
            &mut draft.run_temporary_limit,
            1..=zen_workbench::run::MAX_TEMPORARY_LIMIT,
        );
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-advanced-project-view"), query);
        widgets::checkbox(ui, p, &mut draft.project_view_scratches, &t!("settings-advanced-scratches"), query);
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-advanced-markdown"), query);
        let hide = t!("settings-advanced-markdown-hide-toolbar");
        widgets::checkbox(ui, p, &mut draft.markdown_hide_floating_toolbar, &hide, query);
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-advanced-user-interface"), query);
        let double = t!("settings-advanced-double-modifiers");
        widgets::checkbox(ui, p, &mut draft.disable_double_modifiers, &double, query);
        ui.label(egui::RichText::new(t!("settings-advanced-double-modifiers-note")).small().color(p.text_secondary));
    }

    pub(super) fn font_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        let font = &mut draft.editor_font;
        ui.horizontal_wrapped(|ui| {
            let title = ui.label(label(ui, p, &t!("settings-font-family"), query));
            let shown = font.family.clone().unwrap_or_else(|| t!("settings-font-bundled", name = fonts::BUNDLED));
            let combo = egui::ComboBox::from_id_salt("settings-font-family")
                .selected_text(shown)
                .width(widgets::field_width(ui, 140.0, 240.0))
                .truncate()
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(font.family.is_none(), t!("settings-font-bundled", name = fonts::BUNDLED))
                        .clicked()
                    {
                        font.family = None;
                    }
                    match fonts::installed(ui.ctx()) {
                        None => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(egui::RichText::new(t!("settings-font-loading")).color(p.text_secondary));
                            });
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
                        }
                        Some(installed) => {
                            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                                for name in installed {
                                    let selected = font.family.as_deref() == Some(name.as_str());
                                    if ui.selectable_label(selected, &name).clicked() {
                                        font.family = Some(name);
                                    }
                                }
                            });
                        }
                    }
                });
            let _ = combo.response.labelled_by(title.id);
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let size = ui.label(label(ui, p, &t!("settings-font-size"), query));
            ui.add(
                egui::DragValue::new(&mut font.size)
                    .range(zen_workbench::font_size::SIZES)
                    .speed(0.5)
                    .fixed_decimals(1),
            )
            .labelled_by(size.id);
            ui.add_space(16.0);
            let spacing = ui.label(label(ui, p, &t!("settings-font-line-height"), query));
            ui.add(egui::DragValue::new(&mut font.line_spacing).range(0.6..=3.0).speed(0.05).fixed_decimals(1))
                .labelled_by(spacing.id);
        });
        ui.add_space(10.0);
        widgets::checkbox(ui, p, &mut draft.ligatures, &t!("settings-editor-ligatures"), query);
        ui.label(egui::RichText::new(t!("settings-editor-ligatures-note")).color(p.text_secondary));
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-font-preview"), query);
        fonts::use_preview_font(ui.ctx(), draft.editor_font.family.as_deref());
        let job = preview(ui.ctx(), draft, p);
        egui::Frame::new().fill(p.editor_background).corner_radius(6).inner_margin(10).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::both().max_height(ui.available_height().max(80.0)).show(ui, |ui| {
                ui.add(egui::Label::new(job).extend());
            });
        });
    }

    pub(super) fn code_style_general_page(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        query: Option<&str>,
        dispatch: Option<&mut Dispatch<'_>>,
    ) {
        self.scheme_actions(ui, p, dispatch);
        self.scheme_tabs(ui, p);
        ui.add_space(10.0);
        if self.scheme.tab == 1 {
            self.formatter_tab(ui, p, query);
            return;
        }
        self.scheme_general_fields(ui, p, query);
        let Some(draft) = self.draft.as_mut() else { return };
        ui.horizontal_wrapped(|ui| {
            let title = ui.label(label(ui, p, &t!("settings-code-style-line-separator"), query));
            let combo = egui::ComboBox::from_id_salt("settings-line-separator")
                .selected_text(line_separator_name(draft.line_separator))
                .width(widgets::field_width(ui, 160.0, 260.0))
                .show_ui(ui, |ui| {
                    for choice in [None, Some(LineSeparator::Lf), Some(LineSeparator::Crlf), Some(LineSeparator::Cr)] {
                        ui.selectable_value(&mut draft.line_separator, choice, line_separator_name(choice));
                    }
                });
            crate::accessibility::name(
                &combo.response.labelled_by(title.id),
                egui::Role::ComboBox,
                &t!("settings-code-style-line-separator"),
            );
        });
        ui.add_space(6.0);
        ui.label(egui::RichText::new(t!("settings-code-style-line-separator-note")).color(p.text_secondary));
        ui.add_space(12.0);
        widgets::checkbox(ui, p, &mut draft.editor_config, &t!("settings-code-style-editorconfig"), query);
        ui.label(egui::RichText::new(t!("settings-code-style-editorconfig-note")).color(p.text_secondary));
        ui.add_space(14.0);
        widgets::group_title(ui, p, &t!("settings-code-style-indents-detection"), query);
        widgets::checkbox(ui, p, &mut draft.detect_indents, &t!("settings-code-style-detect-indents"), query);
        ui.add_space(10.0);
        ui.label(egui::RichText::new(t!("settings-code-style-detect-indents-note")).color(p.text_secondary));
    }

    fn scheme_tabs(&mut self, ui: &mut Ui, p: &IdePalette) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (index, key) in ["settings-code-style-general", "settings-code-style-formatter"].into_iter().enumerate()
            {
                let selected = self.scheme.tab == index;
                let text = egui::RichText::new(t!(key)).color(if selected { p.text } else { p.text_secondary });
                let response = ui.add(egui::Button::selectable(selected, text));
                crate::accessibility::edit(&response, |node| {
                    node.set_role(egui::accesskit::Role::Tab);
                    node.set_selected(selected);
                });
                if response.clicked() {
                    self.scheme.tab = index;
                }
            }
        });
    }

    fn scheme_general_fields(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        let page = &mut self.scheme;
        let mut edit = SchemeEdit::new(draft, page.project.as_mut());
        let mut general = edit.general();
        let mut changed = widgets::number_field(
            ui,
            p,
            &t!("settings-code-style-hard-wrap"),
            query,
            &mut general.hard_wrap_at,
            1..=10_000,
        )
        .changed();
        ui.add_space(4.0);
        changed |=
            widgets::checkbox(ui, p, &mut general.wrap_on_typing, &t!("settings-code-style-wrap-on-typing"), query)
                .changed();
        ui.add_space(4.0);
        let text = page.guides.get_or_insert_with(|| guides_text(&general.visual_guides));
        if widgets::text_field(ui, p, &t!("settings-code-style-visual-guides"), query, text).changed() {
            general.visual_guides = parsed_guides(text);
            changed = true;
        }
        if changed {
            edit.set_general(general);
        }
        ui.add_space(12.0);
    }

    fn formatter_tab(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        let page = &mut self.scheme;
        let mut edit = SchemeEdit::new(draft, page.project.as_mut());
        let mut general = edit.general();
        let text = page.do_not_format.get_or_insert_with(|| general.do_not_format.join(";"));
        let mut changed = widgets::text_field(ui, p, &t!("settings-code-style-do-not-format"), query, text).changed();
        if changed {
            general.do_not_format = parsed_patterns(text);
        }
        ui.label(egui::RichText::new(t!("settings-code-style-do-not-format-note")).color(p.text_secondary));
        ui.add_space(12.0);
        changed |= widgets::checkbox(
            ui,
            p,
            &mut general.formatter_markers,
            &t!("settings-code-style-formatter-markers"),
            query,
        )
        .changed();
        ui.label(egui::RichText::new(t!("settings-code-style-formatter-markers-note")).color(p.text_secondary));
        if changed {
            edit.set_general(general);
        }
    }

    fn scheme_actions(&mut self, ui: &mut Ui, p: &IdePalette, mut dispatch: Option<&mut Dispatch<'_>>) {
        let mut export = None;
        let mut action = None;
        ui.horizontal(|ui| {
            let title = ui.label(egui::RichText::new(t!("settings-code-style-scheme")).color(p.text));
            if let Some(draft) = self.draft.as_mut() {
                let page = &mut self.scheme;
                let copies: Vec<String> = draft.code_style_schemes.keys().cloned().collect();
                let combo = egui::ComboBox::from_id_salt("settings-code-style-scheme")
                    .selected_text(page.selected_name(draft))
                    .width((ui.available_width() - 30.0).clamp(80.0, 220.0))
                    .truncate()
                    .show_ui(ui, |ui| {
                        let project = draft.code_style_scheme == CodeStyleScheme::Project;
                        let default = t!("settings-code-style-scheme-default");
                        if ui.selectable_label(!project && draft.selected_copy().is_none(), default).clicked() {
                            page.choose_ide(draft, None);
                        }
                        for copy in copies {
                            if ui.selectable_label(draft.selected_copy() == Some(copy.as_str()), &copy).clicked() {
                                page.choose_ide(draft, Some(copy));
                            }
                        }
                        if ui.selectable_label(project, t!("settings-code-style-scheme-project")).clicked() {
                            page.choose_project(draft);
                        }
                    });
                crate::accessibility::name(
                    &combo.response.labelled_by(title.id),
                    egui::Role::ComboBox,
                    &t!("settings-code-style-scheme"),
                );
            }
            let (rect, response) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(rect, 4, p.hover);
            }
            crate::icons::gear(ui.painter(), rect.shrink(4.0), p.text_secondary);
            let name = t!("settings-code-style-scheme-actions");
            crate::accessibility::button(&response, &name);
            let response = response.on_hover_text(&name);
            crate::components::popup_menu(&response, |ui| {
                if let Some(draft) = self.draft.as_ref() {
                    self.scheme.menu(ui, draft, &mut action);
                }
                ui.menu_button(t!("settings-code-style-export"), |ui| {
                    if crate::components::menu_item(ui, t!("settings-code-style-export-xml")) {
                        action = Some(SchemeAction::ExportXml);
                        ui.close();
                    }
                    if crate::components::menu_item(ui, t!("settings-code-style-export-editorconfig")) {
                        export = Some(false);
                        ui.close();
                    }
                });
                if crate::components::menu_item(ui, t!("settings-code-style-import")) {
                    action = Some(SchemeAction::Import);
                    ui.close();
                }
            });
        });
        if let (Some(action), Some(draft)) = (action, self.draft.as_mut()) {
            self.scheme.run(draft, action, dispatch.as_deref_mut());
        }
        self.scheme_prompts(ui, p);
        if let Some(outcome) = &self.editor_config_export {
            let color = match (outcome.replace, outcome.failed) {
                (true, _) => p.warning_text,
                (false, true) => p.error_text,
                (false, false) => p.text_secondary,
            };
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(&outcome.message).color(color));
                if outcome.replace && ui.button(t!("settings-code-style-export-replace")).clicked() {
                    export = Some(true);
                }
            });
        }
        ui.add_space(6.0);
        ui.label(egui::RichText::new(t!("settings-code-style-scheme-note")).color(p.text_secondary));
        ui.add_space(10.0);
        if let (Some(overwrite), Some(dispatch)) = (export, dispatch) {
            let listed = dispatch("project.editorConfig.list", serde_json::json!({})).unwrap_or_default();
            let exists = listed.as_array().into_iter().flatten().any(|file| file["relative"] == FILE_NAME);
            self.editor_config_export = Some(if exists && !overwrite {
                ExportOutcome { message: t!("settings-code-style-export-exists"), failed: true, replace: true }
            } else {
                ExportOutcome::of(dispatch(
                    "project.editorConfig.export",
                    serde_json::json!({ "overwrite": overwrite }),
                ))
            });
        }
    }

    fn scheme_prompts(&mut self, ui: &mut Ui, p: &IdePalette) {
        let Some(draft) = self.draft.as_mut() else { return };
        let page = &mut self.scheme;
        if let Some(naming) = page.naming.as_mut() {
            let name = t!("settings-code-style-scheme-name");
            let (mut finish, mut cancel) = (false, false);
            ui.horizontal(|ui| {
                let title = ui.label(egui::RichText::new(&name).color(p.text));
                let field = ui.add(egui::TextEdit::singleline(&mut naming.name).desired_width(200.0));
                let field = field.labelled_by(title.id);
                finish = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                finish |= ui.button(t!("settings-code-style-scheme-name-ok")).clicked();
                cancel = ui.button(t!("settings-cancel")).clicked();
            });
            if finish {
                page.finish_naming(draft);
            } else if cancel {
                page.naming = None;
                page.message = None;
            }
        }
        if page.ask_replace {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(t!("settings-code-style-project-exists")).color(p.warning_text));
                if ui.button(t!("settings-code-style-export-replace")).clicked() {
                    page.replace_project(draft);
                }
                if ui.button(t!("settings-cancel")).clicked() {
                    page.ask_replace = false;
                }
            });
        }
        if let Some((message, failed)) = &page.message {
            let color = if *failed { p.error_text } else { p.text_secondary };
            ui.label(egui::RichText::new(message).color(color));
        }
    }

    pub(super) fn code_style_language_page(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        query: Option<&str>,
        language: &str,
    ) {
        let Some(draft) = self.draft.as_mut() else { return };
        let project = &mut self.scheme.project;
        if self.code_styles.declared(language).is_some() {
            self.code_styles.show(ui, p, query, draft, language, |ui, draft| {
                indent_fields(ui, p, query, &mut SchemeEdit::new(draft, project.as_mut()), language);
            });
            return;
        }
        let mut edit = SchemeEdit::new(draft, project.as_mut());
        indent_fields(ui, p, query, &mut edit, language);
        if language == super::page::SQL {
            ui.add_space(14.0);
            sql_format_group(ui, p, &mut edit.draft.sql_format, query);
        }
    }

    pub(super) fn code_style_other_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        let mut edit = SchemeEdit::new(draft, self.scheme.project.as_mut());
        widgets::group_title(ui, p, &t!("settings-code-style-tabs"), query);
        egui::ScrollArea::horizontal().id_salt("settings-code-style-other-table").show(ui, |ui| {
            egui::Grid::new("settings-code-style-other").num_columns(4).spacing([18.0, 8.0]).show(ui, |ui| {
                for key in [
                    "settings-code-style-file-type",
                    "settings-code-style-use-tabs",
                    "settings-code-style-tab-size",
                    "settings-code-style-indent",
                ] {
                    ui.label(egui::RichText::new(t!(key).trim_end_matches(':').to_owned()).color(p.text_secondary));
                }
                ui.end_row();
                for (language, key) in OTHER_FILE_TYPES {
                    let mut style = edit.style_for(language);
                    let title = ui.label(label(ui, p, &t!(key), query));
                    let changed = ui.label_inputs_by(title.id, |ui| {
                        ui.checkbox(&mut style.use_tabs, "").changed()
                            | ui.add(egui::DragValue::new(&mut style.tab_size).range(1..=16)).changed()
                            | ui.add(egui::DragValue::new(&mut style.indent).range(1..=16)).changed()
                    });
                    if changed {
                        edit.set_style(language, style);
                    }
                    ui.end_row();
                }
            });
        });
        ui.add_space(10.0);
        ui.label(egui::RichText::new(t!("settings-code-style-other-note")).color(p.text_secondary));
    }
}

fn indent_fields(ui: &mut Ui, p: &IdePalette, query: Option<&str>, edit: &mut SchemeEdit<'_>, language: &str) {
    widgets::group_title(ui, p, &t!("settings-code-style-tabs"), query);
    let mut style = edit.style_for(language);
    let mut changed =
        widgets::checkbox(ui, p, &mut style.use_tabs, &t!("settings-code-style-use-tabs"), query).changed();
    ui.add_space(4.0);
    changed |=
        widgets::number_field(ui, p, &t!("settings-code-style-tab-size"), query, &mut style.tab_size, 1..=16).changed();
    ui.add_space(4.0);
    changed |=
        widgets::number_field(ui, p, &t!("settings-code-style-indent"), query, &mut style.indent, 1..=16).changed();
    if changed {
        edit.set_style(language, style);
    }
}

fn keyword_case_name(case: KeywordCase) -> String {
    match case {
        KeywordCase::AsIs => t!("settings-code-style-sql-as-is"),
        KeywordCase::Upper => t!("settings-code-style-sql-upper"),
        KeywordCase::Lower => t!("settings-code-style-sql-lower"),
    }
}

fn commas_name(commas: Commas) -> String {
    match commas {
        Commas::Trailing => t!("settings-code-style-sql-trailing"),
        Commas::Leading => t!("settings-code-style-sql-leading"),
    }
}

fn sql_format_group(ui: &mut Ui, p: &IdePalette, options: &mut FormatOptions, query: Option<&str>) {
    widgets::group_title(ui, p, &t!("settings-code-style-sql-formatting"), query);
    let case_title = t!("settings-code-style-sql-keyword-case");
    ui.horizontal_wrapped(|ui| {
        let title = ui.label(label(ui, p, &case_title, query));
        let combo = egui::ComboBox::from_id_salt("settings-code-style-sql-keyword-case")
            .selected_text(keyword_case_name(options.keyword_case))
            .width(widgets::field_width(ui, 120.0, 200.0))
            .show_ui(ui, |ui| {
                for case in [KeywordCase::AsIs, KeywordCase::Upper, KeywordCase::Lower] {
                    ui.selectable_value(&mut options.keyword_case, case, keyword_case_name(case));
                }
            });
        crate::accessibility::name(&combo.response.labelled_by(title.id), egui::Role::ComboBox, &case_title);
    });
    ui.add_space(4.0);
    let commas_title = t!("settings-code-style-sql-commas");
    ui.horizontal_wrapped(|ui| {
        let title = ui.label(label(ui, p, &commas_title, query));
        let combo = egui::ComboBox::from_id_salt("settings-code-style-sql-commas")
            .selected_text(commas_name(options.commas))
            .width(widgets::field_width(ui, 120.0, 200.0))
            .show_ui(ui, |ui| {
                for commas in [Commas::Trailing, Commas::Leading] {
                    ui.selectable_value(&mut options.commas, commas, commas_name(commas));
                }
            });
        crate::accessibility::name(&combo.response.labelled_by(title.id), egui::Role::ComboBox, &commas_title);
    });
    ui.add_space(4.0);
    widgets::checkbox(ui, p, &mut options.clause_on_own_line, &t!("settings-code-style-sql-clause-own-line"), query);
    widgets::checkbox(ui, p, &mut options.item_per_line, &t!("settings-code-style-sql-item-per-line"), query);
    ui.add_space(6.0);
    ui.label(egui::RichText::new(t!("settings-code-style-sql-note")).color(p.text_secondary));
}

fn options_group(ui: &mut Ui, p: &IdePalette, query: Option<&str>, title: &str, add: impl FnOnce(&mut Ui)) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        widgets::note_match(ui, &t!(title), query);
        ui.label(widgets::highlighted(&t!(title), query, theme::semibold(13.0), p.text_secondary, p.search_highlight));
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(32.0);
        ui.vertical(add);
    });
    ui.add_space(8.0);
}

fn lines_name(lines: TrailingSpaces) -> String {
    match lines {
        TrailingSpaces::Modified => t!("settings-editor-on-save-modified-lines"),
        TrailingSpaces::All => t!("settings-editor-on-save-all-lines"),
    }
}

fn on_save_group(ui: &mut Ui, p: &IdePalette, draft: &mut IdeSettings, query: Option<&str>) {
    widgets::group_title(ui, p, &t!("settings-editor-on-save"), query);
    let title = t!("settings-editor-on-save-trailing-spaces");
    ui.horizontal_wrapped(|ui| {
        widgets::checkbox(ui, p, &mut draft.remove_trailing_spaces, &title, query);
        ui.add_enabled_ui(draft.remove_trailing_spaces, |ui| {
            let combo = egui::ComboBox::from_id_salt("settings-editor-on-save-trailing-spaces")
                .selected_text(lines_name(draft.remove_trailing_spaces_on))
                .width(widgets::field_width(ui, 130.0, 180.0))
                .show_ui(ui, |ui| {
                    for lines in [TrailingSpaces::Modified, TrailingSpaces::All] {
                        ui.selectable_value(&mut draft.remove_trailing_spaces_on, lines, lines_name(lines));
                    }
                });
            crate::accessibility::name(&combo.response, egui::Role::ComboBox, &title);
        });
    });
    ui.add_enabled_ui(draft.remove_trailing_spaces, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            let label = t!("settings-editor-on-save-keep-caret-line");
            widgets::checkbox(ui, p, &mut draft.keep_trailing_spaces_on_caret_line, &label, query);
        });
    });
    let label = t!("settings-editor-on-save-blank-lines");
    widgets::checkbox(ui, p, &mut draft.remove_trailing_blank_lines, &label, query);
    let label = t!("settings-editor-on-save-line-break");
    widgets::checkbox(ui, p, &mut draft.ensure_line_break_at_end, &label, query);
}

fn line_separator_name(separator: Option<LineSeparator>) -> String {
    match separator {
        None => t!("settings-line-separator-system"),
        Some(LineSeparator::Lf) => t!("settings-line-separator-lf"),
        Some(LineSeparator::Crlf) => t!("settings-line-separator-crlf"),
        Some(LineSeparator::Cr) => t!("settings-line-separator-cr"),
    }
}

fn kind_key(kind: HintKind) -> &'static str {
    match kind {
        HintKind::ParameterNames => "settings-inlay-hints-parameter-names",
        HintKind::FieldNames => "settings-inlay-hints-field-names",
        HintKind::VariableTypes => "settings-inlay-hints-variable-types",
        HintKind::LiteralTypes => "settings-inlay-hints-literal-types",
        HintKind::TypeArguments => "settings-inlay-hints-type-arguments",
        HintKind::ConstantValues => "settings-inlay-hints-constant-values",
    }
}

fn appearance_group(ui: &mut Ui, p: &IdePalette, draft: &mut IdeSettings, query: Option<&str>) {
    widgets::group_title(ui, p, &t!("settings-editor-appearance"), query);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let code_structure = t!("settings-editor-code-structure");
    widgets::note_match(ui, &code_structure, query);
    ui.label(widgets::highlighted(&code_structure, query, font.clone(), p.text_secondary, p.search_highlight));
    ui.add_space(4.0);
    widgets::checkbox(
        ui,
        p,
        &mut draft.editor_appearance.hard_wrap_guides,
        &t!("settings-editor-hard-wrap-guides"),
        query,
    );
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        let title = widgets::checkbox(ui, p, &mut draft.line_numbers, &t!("settings-editor-line-numbers"), query);
        ui.add_enabled_ui(draft.line_numbers, |ui| {
            let combo = egui::ComboBox::from_id_salt("settings-line-numbers-mode")
                .selected_text(t!(mode_key(draft.line_numbers_mode)))
                .width(widgets::field_width(ui, 120.0, 160.0))
                .show_ui(ui, |ui| {
                    for mode in LineNumbersMode::ALL {
                        ui.selectable_value(&mut draft.line_numbers_mode, mode, t!(mode_key(mode)));
                    }
                });
            let _ = combo.response.labelled_by(title.id);
        });
    });
    ui.add_space(4.0);
    widgets::checkbox(ui, p, &mut draft.method_separators, &t!("settings-editor-method-separators"), query);
    ui.add_space(4.0);
    let aids = &mut draft.editor_appearance;
    widgets::checkbox(ui, p, &mut aids.indent_guides, &t!("settings-editor-indent-guides"), query);
    ui.add_space(8.0);
    let presentation = t!("settings-editor-presentation");
    ui.label(widgets::highlighted(&presentation, query, font, p.text_secondary, p.search_highlight));
    ui.add_space(4.0);
    widgets::checkbox(ui, p, &mut aids.whitespaces, &t!("settings-editor-whitespaces"), query);
    let kinds = &mut aids.whitespace_kinds;
    ui.add_enabled_ui(aids.whitespaces, |ui| {
        for (value, key) in [
            (&mut kinds.leading, "settings-editor-whitespaces-leading"),
            (&mut kinds.inner, "settings-editor-whitespaces-inner"),
            (&mut kinds.trailing, "settings-editor-whitespaces-trailing"),
            (&mut kinds.selection, "settings-editor-whitespaces-selection"),
        ] {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                widgets::checkbox(ui, p, value, &t!(key), query);
            });
        }
    });
}

fn mode_key(mode: LineNumbersMode) -> &'static str {
    match mode {
        LineNumbersMode::Absolute => "settings-editor-line-numbers-absolute",
        LineNumbersMode::Relative => "settings-editor-line-numbers-relative",
        LineNumbersMode::Hybrid => "settings-editor-line-numbers-hybrid",
    }
}

fn label(ui: &Ui, p: &IdePalette, text: &str, query: Option<&str>) -> LayoutJob {
    widgets::label_job(ui, p, text, query)
}

fn preview(ctx: &egui::Context, draft: &IdeSettings, p: &IdePalette) -> LayoutJob {
    let (size, line_height) =
        metrics::scaled(draft.editor_font.size, draft.editor_font.line_spacing, crate::ide::ide_zoom::font_zoom(ctx));
    let family = theme::family::editor_preview();
    let family = if ctx.fonts(|fonts| fonts.definitions().families.contains_key(&family)) {
        family
    } else {
        egui::FontFamily::Monospace
    };
    let tab = usize::from(draft.code_style_for("go").tab_size.max(1));
    let text = SAMPLE.replace('\t', &" ".repeat(tab));
    let format = TextFormat {
        font_id: FontId::new(size, family),
        color: p.code,
        line_height: Some(line_height),
        ..TextFormat::default()
    };
    let mut job = LayoutJob::single_section(text, format);
    if !draft.ligatures {
        let format = job.sections[0].format.clone();
        job.sections = job
            .text
            .char_indices()
            .map(|(at, c)| LayoutSection {
                leading_space: 0.0,
                byte_range: egui::text::ByteIndex(at)..egui::text::ByteIndex(at + c.len_utf8()),
                format: format.clone(),
            })
            .collect();
    }
    job
}

fn tab_rendering_name(rendering: TabRendering) -> String {
    match rendering {
        TabRendering::Line => t!("settings-advanced-tab-line"),
        TabRendering::Arrow => t!("settings-advanced-tab-arrow"),
    }
}

#[cfg(test)]
mod tests {
    use zen_project::settings::CodeStyle;

    use super::*;

    fn draw_general(dialog: &mut SettingsDialog, palette: &IdePalette, ui: &mut Ui) {
        ui.painter().rect_filled(ui.max_rect(), 0, palette.popup);
        egui::Frame::new().inner_margin(16).show(ui, |ui| dialog.code_style_general_page(ui, palette, None, None));
    }

    struct ExportHarness {
        registry: zen_core::Registry,
        session: zen_core::Session,
        dialog: SettingsDialog,
        palette: IdePalette,
        screen: crate::offscreen::Offscreen,
    }

    impl ExportHarness {
        fn new(project: &std::path::Path) -> Self {
            let mut registry = zen_core::Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let mut session = zen_core::Session::new();
            registry.dispatch(&mut session, "file.project.open", serde_json::json!({ "path": project })).unwrap();
            let mut dialog = SettingsDialog::default();
            dialog.open();
            dialog.draft = Some(IdeSettings::default());
            let palette = IdePalette::zen_dark();
            let screen = crate::offscreen::Offscreen::new(egui::vec2(760.0, 420.0), 2.0);
            palette.apply(screen.context());
            Self { registry, session, dialog, palette, screen }
        }

        fn run<R>(&mut self, then: impl FnOnce(&mut crate::offscreen::Offscreen, &mut dyn FnMut(&mut Ui)) -> R) -> R {
            let (registry, session, dialog, palette) =
                (&self.registry, &mut self.session, &mut self.dialog, &self.palette);
            let mut ui = |ui: &mut Ui| {
                let mut dispatch =
                    |id: &str, params| registry.dispatch(session, id, params).map_err(|error| error.to_string());
                ui.painter().rect_filled(ui.max_rect(), 0, palette.popup);
                egui::Frame::new().inner_margin(16).show(ui, |ui| {
                    dialog.code_style_general_page(ui, palette, None, Some(&mut dispatch as &mut Dispatch<'_>))
                });
            };
            then(&mut self.screen, &mut ui)
        }

        fn image(&mut self) -> crate::offscreen::Frame {
            self.run(|screen, ui| screen.render(ui))
        }

        fn click(&mut self, role: egui::accesskit::Role, name: &str) {
            let tree = self.run(|screen, ui| screen.accessibility_tree(ui));
            let node = crate::accessibility::testing::find(&tree, role, name)
                .unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
            let bounds = node.bounds().expect("the node has bounds");
            let at = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
                self.run(|screen, ui| screen.step(events, ui));
            }
        }
    }

    #[test]
    fn show_scheme_actions_exports_the_scheme_as_an_editorconfig_file() {
        use egui::accesskit::Role;
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(std::path::PathBuf::from);
        let languages: &[&str] = if snapshots.is_some() { &["en", "ru"] } else { &["en"] };
        for language in languages {
            zen_i18n::set_language(Some(language));
            let project = tempfile::tempdir().unwrap();
            let file = project.path().join(".editorconfig");
            let mut harness = ExportHarness::new(project.path());
            harness.image();
            harness.click(Role::Button, &t!("settings-code-style-scheme-actions"));
            harness.click(Role::Button, &t!("settings-code-style-export"));
            if let Some(dir) = &snapshots {
                let image = harness.image();
                crate::offscreen::write_png(&dir.join(format!("code-style-export-menu-{language}.png")), &image)
                    .unwrap();
            }
            harness.click(Role::Button, &t!("settings-code-style-export-editorconfig"));
            let written = std::fs::read_to_string(&file).unwrap();
            assert!(written.starts_with("root = true\n\n[*]\nindent_size = 4\n"), "{written}");
            std::fs::write(&file, "root = true\n").unwrap();
            harness.click(Role::Button, &t!("settings-code-style-scheme-actions"));
            harness.click(Role::Button, &t!("settings-code-style-export"));
            harness.click(Role::Button, &t!("settings-code-style-export-editorconfig"));
            assert_eq!(std::fs::read_to_string(&file).unwrap(), "root = true\n", "not replaced without asking");
            assert!(harness.dialog.editor_config_export.as_ref().is_some_and(|outcome| outcome.replace));
            if let Some(dir) = &snapshots {
                let image = harness.image();
                crate::offscreen::write_png(&dir.join(format!("code-style-export-exists-{language}.png")), &image)
                    .unwrap();
            }
            harness.click(Role::Button, &t!("settings-code-style-export-replace"));
            assert!(std::fs::read_to_string(&file).unwrap().contains("[*.go]"));
            assert!(harness.dialog.editor_config_export.as_ref().is_some_and(|outcome| !outcome.failed));
            if let Some(dir) = &snapshots {
                let image = harness.image();
                crate::offscreen::write_png(&dir.join(format!("code-style-exported-{language}.png")), &image).unwrap();
            }
        }
        zen_i18n::set_language(Some("en"));
    }

    #[test]
    fn the_scheme_actions_write_the_project_files_and_exported_schemes() {
        use egui::accesskit::Role;
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::set_language(Some("en"));
        let project = tempfile::tempdir().unwrap();
        let mut harness = ExportHarness::new(project.path());
        harness
            .session
            .state_mut::<zen_workbench::project_settings::ProjectSettingsState>()
            .set_providers(vec![std::sync::Arc::new(zen_jb_import::IdeaProject::with_user_home(None))]);
        let begin = |harness: &mut ExportHarness| {
            let (registry, session) = (&harness.registry, &mut harness.session);
            let mut dispatch =
                |id: &str, params| registry.dispatch(session, id, params).map_err(|error| error.to_string());
            harness.dialog.scheme.begin(&mut dispatch as &mut Dispatch<'_>);
        };
        let commit = |harness: &mut ExportHarness| {
            let (registry, session) = (&harness.registry, &mut harness.session);
            let mut dispatch =
                |id: &str, params| registry.dispatch(session, id, params).map_err(|error| error.to_string());
            harness.dialog.scheme.commit(&mut dispatch as &mut Dispatch<'_>);
        };
        begin(&mut harness);
        let draft = harness.dialog.draft.as_mut().unwrap();
        draft.code_style_scheme = CodeStyleScheme::Default;
        draft.code_style_general.hard_wrap_at = 90;
        harness.image();
        harness.click(Role::Button, &t!("settings-code-style-scheme-actions"));
        harness.click(Role::Button, &t!("settings-code-style-copy-to-project"));
        commit(&mut harness);
        let scheme = std::fs::read_to_string(project.path().join(".idea/codeStyles/Project.xml")).unwrap();
        assert!(scheme.contains("<option name=\"RIGHT_MARGIN\" value=\"90\" />"), "{scheme}");
        let config = std::fs::read_to_string(project.path().join(".idea/codeStyles/codeStyleConfig.xml")).unwrap();
        assert!(config.contains("USE_PER_PROJECT_SETTINGS"), "{config}");

        begin(&mut harness);
        assert!(harness.dialog.scheme.project.is_some(), "read back from the project");
        let exported = project.path().join("scheme.xml");
        harness.dialog.scheme.picked = Some(exported.clone());
        harness.image();
        harness.click(Role::Button, &t!("settings-code-style-scheme-actions"));
        harness.click(Role::Button, &t!("settings-code-style-export"));
        harness.click(Role::Button, &t!("settings-code-style-export-xml"));
        let text = std::fs::read_to_string(&exported).unwrap();
        assert!(text.starts_with("<code_scheme name=\"Project\""), "{text}");

        harness.dialog.scheme.picked = Some(exported);
        harness.image();
        harness.click(Role::Button, &t!("settings-code-style-scheme-actions"));
        harness.click(Role::Button, &t!("settings-code-style-import"));
        let draft = harness.dialog.draft.as_ref().unwrap();
        assert_eq!(draft.selected_copy(), Some("Project 2"), "imported under a free name");
        assert_eq!(draft.ide_scheme().1.hard_wrap_at, 90);
        zen_i18n::set_language(Some("en"));
    }

    #[test]
    fn the_formatter_tab_edits_do_not_format_and_the_markers() {
        use egui::accesskit::Role;
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let project = tempfile::tempdir().unwrap();
        let mut harness = ExportHarness::new(project.path());
        harness.image();
        harness.click(Role::Tab, &t!("settings-code-style-formatter"));
        harness.click(Role::CheckBox, &t!("settings-code-style-formatter-markers"));
        let draft = harness.dialog.draft.as_ref().unwrap();
        assert!(draft.ide_scheme().1.formatter_markers);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(std::path::PathBuf::from);
        if let Some(dir) = &snapshots {
            let image = harness.image();
            crate::offscreen::write_png(&dir.join("code-style-formatter-en.png"), &image).unwrap();
            harness.click(Role::Tab, &t!("settings-code-style-general"));
            let image = harness.image();
            crate::offscreen::write_png(&dir.join("code-style-general-en.png"), &image).unwrap();
        }
    }

    fn click_general(
        screen: &mut crate::offscreen::Offscreen,
        dialog: &mut SettingsDialog,
        palette: &IdePalette,
        role: egui::accesskit::Role,
        name: &str,
    ) {
        let tree = screen.accessibility_tree(|ui| draw_general(dialog, palette, ui));
        let node = crate::accessibility::testing::find(&tree, role, name)
            .unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
        let bounds = node.bounds().expect("the node has bounds");
        let at = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
            screen.step(events, |ui| draw_general(dialog, palette, ui));
        }
    }

    #[test]
    fn the_file_encodings_page_loads_and_commits_through_its_commands() {
        use crate::ide::settings::file_encodings::FileEncodingsPage;
        use serde_json::json;
        let calls = std::cell::RefCell::new(Vec::new());
        let mut dispatch = |command: &str, params: serde_json::Value| -> Result<serde_json::Value, String> {
            calls.borrow_mut().push((command.to_owned(), params));
            Ok(match command {
                "file.encoding.list" => json!({ "charsets": ["UTF-8", "windows-1251"] }),
                "project.structure.get" => json!({ "root": "/p" }),
                _ => json!({ "project": "windows-1251", "files": [{ "path": "/p/a", "charset": "KOI8-R" }] }),
            })
        };
        let mut page = FileEncodingsPage::default();
        page.begin(&mut dispatch);
        assert!(page.has_project);
        assert_eq!(page.draft.project.as_deref(), Some("windows-1251"));
        page.commit(&mut dispatch);
        assert_eq!(calls.borrow().len(), 3, "nothing changed, nothing is written");
        page.draft.files.clear();
        page.draft.bom_for_new_files = Some(zen_project::settings::NewFileBom::With);
        page.commit(&mut dispatch);
        let calls = calls.borrow();
        let (command, params) = calls.last().unwrap();
        assert_eq!(command, "project.encodings.set");
        assert_eq!(params["bomForNewFiles"], "with");
        assert_eq!(params["files"], json!([]));
    }

    #[test]
    fn the_general_page_chooses_the_line_separator_of_new_files() {
        use egui::accesskit::Role;
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut dialog = SettingsDialog::default();
        dialog.open();
        dialog.draft = Some(IdeSettings::default());
        let palette = IdePalette::zen_dark();
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(760.0, 360.0), 2.0);
        palette.apply(screen.context());
        screen.render(|ui| draw_general(&mut dialog, &palette, ui));
        click_general(&mut screen, &mut dialog, &palette, Role::ComboBox, &t!("settings-code-style-line-separator"));
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            let image = screen.render(|ui| draw_general(&mut dialog, &palette, ui));
            crate::offscreen::write_png(&std::path::Path::new(&dir).join("code-style-general.png"), &image).unwrap();
        }
        click_general(&mut screen, &mut dialog, &palette, Role::Button, &t!("settings-line-separator-crlf"));
        assert_eq!(dialog.draft.as_ref().unwrap().line_separator, Some(LineSeparator::Crlf));
        let mut session = zen_core::Settings::default();
        dialog.draft.as_ref().unwrap().apply_to_session(&mut session);
        assert_eq!(session.get::<String>(zen_workbench::line_separators::NEW_FILES).as_deref(), Some("crlf"));
    }

    #[test]
    fn the_general_page_turns_editorconfig_support_off() {
        use egui::accesskit::Role;
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut dialog = SettingsDialog::default();
        dialog.open();
        dialog.draft = Some(IdeSettings::default());
        assert!(dialog.draft.as_ref().unwrap().editor_config, "on by default");
        let palette = IdePalette::zen_dark();
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(760.0, 360.0), 2.0);
        palette.apply(screen.context());
        let image = screen.render(|ui| draw_general(&mut dialog, &palette, ui));
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            crate::offscreen::write_png(&std::path::Path::new(&dir).join("code-style-editorconfig.png"), &image)
                .unwrap();
        }
        let mut session = zen_core::Settings::default();
        dialog.draft.as_ref().unwrap().apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(zen_workbench::editor_config::ENABLED), Some(true));
        click_general(&mut screen, &mut dialog, &palette, Role::CheckBox, &t!("settings-code-style-editorconfig"));
        assert!(!dialog.draft.as_ref().unwrap().editor_config);
        dialog.draft.as_ref().unwrap().apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(zen_workbench::editor_config::ENABLED), Some(false));
        assert!(!zen_workbench::editor_config::enabled(&session));
    }

    #[test]
    fn a_changed_row_becomes_an_entry_of_its_own() {
        let mut draft = IdeSettings::default();
        let mut style = draft.code_style_for("json");
        style.indent = 4;
        SchemeEdit::new(&mut draft, None).set_style("json", style);
        assert_eq!(draft.code_style_for("json"), CodeStyle { use_tabs: false, tab_size: 2, indent: 4 });
        assert_eq!(draft.code_style_for("yaml"), CodeStyle::default_for("yaml"));
        let mut session = zen_core::Settings::default();
        draft.apply_to_session(&mut session);
        assert_eq!(session.get::<u8>("editor.codeStyle.json.indent"), Some(4));
        assert_eq!(session.get::<u8>("editor.codeStyle.yaml.indent"), None, "untouched languages keep the guess");
    }

    #[test]
    fn the_inlay_hint_kinds_reach_the_session() {
        let mut draft = IdeSettings::default();
        *draft.inlay_hints.kind_mut(HintKind::VariableTypes) = true;
        *draft.inlay_hints.kind_mut(HintKind::ParameterNames) = false;
        let mut session = zen_core::Settings::default();
        draft.apply_to_session(&mut session);
        let switches = zen_workbench::language::inlay_hints::HintSwitches::from_settings(&session);
        assert!(switches.enabled);
        assert_eq!(switches.kinds.into_iter().collect::<Vec<_>>(), [HintKind::VariableTypes]);
        draft.inlay_hints.enabled = false;
        draft.apply_to_session(&mut session);
        assert!(!zen_workbench::language::inlay_hints::HintSwitches::from_settings(&session).enabled);
        let json = serde_json::to_value(&draft).unwrap();
        assert_eq!(json["inlayHints"]["variableTypes"], true);
        assert!(kind_key(HintKind::ConstantValues).starts_with("settings-inlay-hints-"));
    }

    fn shown_texts(shape: &egui::Shape, found: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(text) => found.push((text.galley.text().to_owned(), text.visual_bounding_rect())),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| shown_texts(shape, found)),
            _ => {}
        }
    }

    fn editor_frame(
        ctx: &egui::Context,
        dialog: &mut SettingsDialog,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        let input = egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 1400.0))),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| dialog.editor_page(ui, &IdePalette::zen_dark(), None));
        output.textures_delta.clear();
        let mut found = Vec::new();
        for clipped in &output.shapes {
            shown_texts(&clipped.shape, &mut found);
        }
        found
    }

    fn click_label(ctx: &egui::Context, dialog: &mut SettingsDialog, label: &str) {
        let texts = editor_frame(ctx, dialog, Vec::new());
        let at = texts
            .iter()
            .find(|(text, _)| text == label)
            .map(|(_, rect)| rect.center())
            .unwrap_or_else(|| panic!("no {label:?} among {texts:?}"));
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        editor_frame(ctx, dialog, vec![egui::Event::PointerMoved(at)]);
        editor_frame(ctx, dialog, vec![button(true)]);
        editor_frame(ctx, dialog, vec![button(false)]);
    }

    #[test]
    fn the_double_click_check_box_works_only_with_camel_humps() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut dialog = SettingsDialog::default();
        dialog.open();
        dialog.draft = Some(IdeSettings::default());
        let camel_humps = t!("settings-editor-camel-humps");
        let double_click = t!("settings-editor-camel-humps-double-click");
        let draft = |dialog: &SettingsDialog| {
            let draft = dialog.draft.as_ref().unwrap();
            (draft.camel_humps, draft.camel_humps_on_double_click)
        };
        assert_eq!(draft(&dialog), (false, true));
        click_label(&ctx, &mut dialog, &double_click);
        assert_eq!(draft(&dialog), (false, true), "disabled while camel humps are off");
        click_label(&ctx, &mut dialog, &camel_humps);
        assert_eq!(draft(&dialog), (true, true));
        click_label(&ctx, &mut dialog, &double_click);
        assert_eq!(draft(&dialog), (true, false));
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            let palette = IdePalette::zen_dark();
            for language in ["en", "ru"] {
                zen_i18n::set_language(Some(language));
                for on in [false, true] {
                    let mut screen = crate::offscreen::Offscreen::new(egui::vec2(760.0, 420.0), 2.0);
                    palette.apply(screen.context());
                    let mut dialog = SettingsDialog::default();
                    dialog.open();
                    dialog.draft = Some(IdeSettings { camel_humps: on, ..IdeSettings::default() });
                    let image = screen.render(|ui| {
                        ui.painter().rect_filled(ui.max_rect(), 0, palette.popup);
                        egui::Frame::new().inner_margin(16).show(ui, |ui| dialog.editor_page(ui, &palette, None));
                    });
                    let name = format!("settings-editor-smart-keys-{language}-{}.png", if on { "on" } else { "off" });
                    crate::offscreen::write_png(&std::path::Path::new(&dir).join(name), &image).unwrap();
                }
            }
            zen_i18n::set_language(Some("en"));
        }
    }

    #[test]
    fn the_preview_uses_the_drafts_metrics() {
        let ctx = egui::Context::default();
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        let mut draft = IdeSettings::default();
        draft.editor_font.size = 16.0;
        draft.editor_font.line_spacing = 1.5;
        let job = preview(&ctx, &draft, &IdePalette::zen_dark());
        assert_eq!(job.sections[0].format.font_id.size, 16.0);
        assert_eq!(job.sections[0].format.line_height, Some(34.0));
        assert!(!job.text.contains('\t') && job.text.contains("    for"));
        assert!(job.sections.len() > 1, "without ligatures every character is shaped on its own");
        draft.ligatures = true;
        assert_eq!(preview(&ctx, &draft, &IdePalette::zen_dark()).sections.len(), 1);
    }
}
