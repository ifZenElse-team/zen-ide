mod activity_monitor;
mod agent;
mod ant_tool_window;
mod app;
mod appearance;
mod areas_dialog;
mod artifacts_popup;
mod associate_dialog;
mod automation;
mod autosave;
mod background_tasks;
mod backup_dialog;
mod bookmark_dialogs;
mod bookmarks_window;
mod branches_popup;
mod breadcrumbs;
mod breakpoints_dialog;
mod build_sync_window;
mod build_tool_window;
mod bundle_view;
mod caffeine;
mod caret_popup;
mod certificate_dialog;
mod clone_dialog;
mod code_vision;
mod color_swatches;
mod commit_window;
mod compare;
mod connection_dialog;
mod convert_paste;
mod coverage_window;
mod create_test_dialog;
mod csv_import_dialog;
pub(super) mod customization;
mod data_grid;
mod database_navigator;
mod database_view;
mod debug_coroutines;
mod debug_window;
mod debugger_notice;
mod dependencies_tool_window;
mod dependency_analyzer;
mod diagnostics_dialogs;
mod diff_view;
mod doc_popups;
mod editor_access;
mod editor_config_view;
mod editor_groups;
mod editor_view;
mod editor_windows;
mod encoding_popup;
mod endpoints_window;
mod evaluate;
mod execute_dialog;
mod exit_dialog;
mod export_dialog;
mod external_tools;
mod extract_popup;
mod file_structure;
mod file_template_view;
mod filter_popups;
mod find_bar;
mod find_history;
mod find_in_files;
mod find_results;
mod find_usages_settings;
mod find_window;
mod focus_return;
mod folder_diff;
mod font_zoom;
mod form;
mod front;
mod generate_popup;
mod git_window;
mod git_workspace;
mod glyphs;
#[cfg(not(target_arch = "wasm32"))]
mod headless;
mod hierarchy_window;
mod ide_zoom;
mod image_view;
mod import_dialog;
mod indent_widget;
#[cfg(test)]
mod injection_tests;
mod inlay_hints;
mod inline_values;
mod inspect_window;
mod intentions_popup;
mod internal_errors;
mod invalidate_dialog;
mod keymap_page;
mod language_servers;
#[cfg(not(target_arch = "wasm32"))]
mod launch;
mod launcher_dialog;
mod libraries_tree;
mod line_markers;
mod line_numbers;
mod local_history;
mod macro_dialogs;
mod main_menu;
mod manage_settings;
mod markdown_math;
mod markdown_mermaid;
mod markdown_toolbar;
mod markdown_view;
mod navigation_bar;
mod new_popup;
mod new_project_dialog;
mod notifications;
mod npm_tool_window;
#[cfg(test)]
mod palette_pin;
mod parameters_dialog;
mod paste_history;
mod plugin_index_view;
mod plugin_projects;
mod plugin_tool_inputs;
mod plugin_tool_window;
mod popup_size;
mod preview_pane;
mod problem_popup;
mod productivity;
mod projects_popup;
mod quick_switch;
mod read_only;
mod recent_files;
mod recent_locations;
mod recent_usages;
mod redact;
mod refactoring_dialog;
mod refactoring_preview;
mod regions_popup;
mod rename_dialog;
mod run_anything;
mod run_configurations;
mod run_launch;
mod run_widget;
mod run_window;
mod scope_row;
mod scopes_dialog;
mod scratch_popups;
mod scratches_tree;
mod search_everywhere;
mod search_filter;
mod select_in;
mod selection;
mod services_window;
mod settings;
mod several_dialog;
mod shortcuts_reference;
mod smart_step;
mod soft_wrap;
#[cfg(test)]
mod spelling_tests;
mod splash;
mod sql_connection_popup;
mod sql_script_bar;
mod ssh_dialog;
mod starters_dialog;
mod status_widgets;
mod sticky_lines;
mod stream_trace;
mod structure_window;
mod surround_popup;
mod system_clipboard;
mod tab_drag;
mod tab_strip;
mod targets_popup;
#[cfg(not(target_arch = "wasm32"))]
mod terminal_link_dialog;
mod terminal_window;
mod test_history_popup;
mod tips;
mod todo_marks;
mod todo_window;
mod tool_stripes;
mod tool_windows;
mod toolchain_watch;
mod transaction_dialog;
mod transfer_dialog;
mod trust_dialog;
mod unwrap_popup;
mod updates;
mod usages_popup;
mod vcs;
mod view_modes;
mod visual_aids;
mod welcome;
mod whats_new;
mod workspace;
mod workspace_host;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{
    Align2, Color32, CornerRadius, FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2, vec2,
};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_syntax::{HighlightKind, Highlighter};
use zen_theme::{EditorKey, ResolvedTheme, SyntaxKey, UiKey};
use zen_workbench::file_clipboard::FileClipboard;
use zen_workbench::notifications::{Action, Group, Kind, NewNotification};
use zen_workbench::tabs::TabPlacement;
use zen_workbench::{Document, Workbench};

use crate::accessibility;
use crate::plugins::{Offer, PluginConfig, Plugins};
use crate::theme;
use crate::theme_bridge::{CodeStyle, color};
pub use app::IdeApp;
#[cfg(not(target_arch = "wasm32"))]
pub use app::run_mcp_bridge;
use app::{Overview, Request, RestartAsk};
use front::FrontKind;
use glyphs::*;
#[cfg(not(target_arch = "wasm32"))]
pub use headless::{
    EXIT_ALREADY_RUNNING, EXIT_USAGE, HeadlessOptions, automation_port, run_headless, usage as headless_usage,
};
#[cfg(not(target_arch = "wasm32"))]
pub use launch::{HANDOFF_ONLY, Program, Start, read_command_line};
pub use settings::{
    CodeStyleScheme, DEFAULT_UI_FONT_SIZE, EditorFont, FoldArrows, IdeSettings, NavigationBarPlacement, UI_FONT_SIZES,
    WheelFontSize,
};
use status_widgets::StatusWidget;
pub use workspace_host::WorkspaceView;

const PALETTE_ID: &str = "zen-ide-palette";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IdePalette {
    pub frame: Color32,
    pub island: Color32,
    pub text: Color32,
    pub text_secondary: Color32,
    pub gutter_text: Color32,
    pub current_line: Color32,
    pub hover: Color32,
    pub popup_hover: Color32,
    pub selection: Color32,
    pub caret: Color32,
    pub search: Color32,
    pub search_mark: Color32,
    pub search_highlight: Color32,
    pub accent: Color32,
    pub excluded: Color32,
    pub tab_border: Color32,
    pub package: Color32,
    pub semantic_type: Color32,
    pub call: Color32,
    pub doc_name: Color32,
    pub active_tab: Color32,
    pub active_tab_border: Color32,
    pub test_row: Color32,
    pub fold_mark: Color32,
    pub fold_arrow: Color32,
    pub fold_text: Color32,
    pub inlay_hint_background: Color32,
    pub inlay_hint_foreground: Color32,
    pub gutter_separator: Color32,
    pub method_separator: Color32,
    pub sticky_background: Color32,
    pub sticky_border: Color32,
    pub code: Color32,
    pub keyword: Color32,
    pub string: Color32,
    pub number: Color32,
    pub comment: Color32,
    pub doc_comment: Color32,
    pub todo: Color32,
    pub macro_call: Color32,
    pub function: Color32,
    pub type_name: Color32,
    pub lifetime: Color32,
    pub attribute: Color32,
    pub constant: Color32,
    pub heading: Color32,
    pub run: Color32,
    pub banner: Color32,
    pub link: Color32,
    pub popup: Color32,
    pub popup_border: Color32,
    pub popup_shadow: Color32,
    pub popup_code: Color32,
    pub text_field: Color32,
    pub text_field_border: Color32,
    pub text_field_selection: Color32,
    pub text_field_caret: Color32,
    pub button_border: Color32,
    pub accent_text: Color32,
    pub stripe_active_icon: Color32,
    pub excluded_background: Color32,
    pub modified_tab: Color32,
    pub error_text: Color32,
    pub warning_text: Color32,
    pub checker_light: Color32,
    pub checker_dark: Color32,
    pub projects: [Color32; UiKey::PROJECTS.len()],
    pub project_text: Color32,
    pub editor_background: Color32,
    pub editor_selection: Color32,
    pub symbol_highlight: Color32,
    pub symbol_highlight_write: Color32,
    pub bracket_matched_background: Color32,
    pub bracket_matched: Color32,
    pub bracket_unmatched_background: Color32,
    pub bracket_unmatched: Color32,
    pub diagnostic_error: Color32,
    pub diagnostic_warning: Color32,
    pub diagnostic_info: Color32,
    pub diagnostic_hint: Color32,
    pub diagnostic_typo: Color32,
    pub completion_function: Color32,
    pub completion_variable: Color32,
    pub completion_field: Color32,
    pub completion_constant: Color32,
    pub completion_type: Color32,
    pub completion_interface: Color32,
    pub completion_module: Color32,
    pub completion_keyword: Color32,
    pub completion_snippet: Color32,
    pub completion_other: Color32,
    pub vcs_modified: Color32,
    pub vcs_added: Color32,
    pub vcs_deleted: Color32,
    pub vcs_unversioned: Color32,
    pub vcs_ignored: Color32,
    pub vcs_conflict: Color32,
    pub vcs_incoming: Color32,
    pub vcs_outgoing: Color32,
    pub vcs_current_branch: Color32,
    pub gutter_added: Color32,
    pub gutter_modified: Color32,
    pub gutter_deleted: Color32,
    pub diff_inserted: Color32,
    pub diff_deleted: Color32,
    pub diff_modified: Color32,
    pub diff_inserted_word: Color32,
    pub diff_deleted_word: Color32,
    pub diff_modified_word: Color32,
    pub diff_filler: Color32,
    pub breakpoint: Color32,
    pub breakpoint_disabled: Color32,
    pub execution_line: Color32,
    pub execution_arrow: Color32,
    pub annotation_recent: Color32,
    pub annotation_old: Color32,
    pub coverage_covered: Color32,
    pub coverage_partly: Color32,
    pub coverage_not_covered: Color32,
    pub unused: Color32,
    pub whitespace: Color32,
    pub indent_guide: Color32,
    pub indent_guide_selected: Color32,
    pub right_margin: Color32,
    pub graph: [Color32; 6],
    pub dark: bool,
    pub styles: [CodeStyle; SyntaxKey::COUNT],
}

impl IdePalette {
    pub fn from_theme(theme: &ResolvedTheme) -> IdePalette {
        let ui = |key: UiKey| color(theme.ui(key));
        let editor = |key: EditorKey| color(theme.editor(key));
        let styles: [CodeStyle; SyntaxKey::COUNT] =
            std::array::from_fn(|index| CodeStyle::from_style(theme.syntax(<SyntaxKey as zen_theme::Key>::ALL[index])));
        let syntax = |key: SyntaxKey| styles[zen_theme::Key::index(key)].color;
        IdePalette {
            frame: ui(UiKey::WindowBackground),
            island: ui(UiKey::IslandBackground),
            text: ui(UiKey::TextPrimary),
            text_secondary: ui(UiKey::TextSecondary),
            gutter_text: editor(EditorKey::LineNumber),
            current_line: editor(EditorKey::CurrentLine),
            hover: ui(UiKey::ListHover),
            popup_hover: ui(UiKey::PopupHover),
            selection: ui(UiKey::ListSelection),
            caret: editor(EditorKey::Caret),
            search: editor(EditorKey::SearchMatch),
            search_mark: editor(EditorKey::SearchMark),
            search_highlight: ui(UiKey::SearchHighlight),
            accent: ui(UiKey::Accent),
            excluded: ui(UiKey::TreeExcludedText),
            tab_border: ui(UiKey::TabBorder),
            package: syntax(SyntaxKey::SemanticPackage),
            semantic_type: syntax(SyntaxKey::SemanticType),
            call: syntax(SyntaxKey::SemanticCall),
            doc_name: syntax(SyntaxKey::DocName),
            active_tab: ui(UiKey::TabActiveBackground),
            active_tab_border: ui(UiKey::TabActiveBorder),
            test_row: ui(UiKey::TreeTestRow),
            fold_mark: editor(EditorKey::FoldMarker),
            fold_arrow: editor(EditorKey::FoldArrow),
            fold_text: editor(EditorKey::FoldText),
            inlay_hint_background: editor(EditorKey::InlayHintBackground),
            inlay_hint_foreground: editor(EditorKey::InlayHintForeground),
            gutter_separator: editor(EditorKey::GutterSeparator),
            method_separator: editor(EditorKey::MethodSeparator),
            sticky_background: editor(EditorKey::StickyBackground),
            sticky_border: editor(EditorKey::StickyBorder),
            code: editor(EditorKey::Foreground),
            keyword: syntax(SyntaxKey::Keyword),
            string: syntax(SyntaxKey::String),
            number: syntax(SyntaxKey::Number),
            comment: syntax(SyntaxKey::Comment),
            doc_comment: syntax(SyntaxKey::DocComment),
            todo: syntax(SyntaxKey::Todo),
            macro_call: syntax(SyntaxKey::Macro),
            function: syntax(SyntaxKey::Function),
            type_name: syntax(SyntaxKey::Type),
            lifetime: syntax(SyntaxKey::Lifetime),
            attribute: syntax(SyntaxKey::Attribute),
            constant: syntax(SyntaxKey::Constant),
            heading: syntax(SyntaxKey::Heading),
            run: ui(UiKey::IconRun),
            banner: ui(UiKey::BannerBackground),
            link: ui(UiKey::Link),
            popup: ui(UiKey::PopupBackground),
            popup_border: ui(UiKey::PopupBorder),
            popup_shadow: ui(UiKey::PopupShadow),
            popup_code: ui(UiKey::PopupCodeBackground),
            text_field: ui(UiKey::TextFieldBackground),
            text_field_border: ui(UiKey::TextFieldBorder),
            text_field_selection: ui(UiKey::TextFieldSelection),
            text_field_caret: ui(UiKey::TextFieldCaret),
            button_border: ui(UiKey::ButtonSecondaryBorder),
            accent_text: ui(UiKey::AccentForeground),
            stripe_active_icon: ui(UiKey::StripeActiveIcon),
            excluded_background: ui(UiKey::TreeExcludedBackground),
            modified_tab: ui(UiKey::TabModified),
            error_text: ui(UiKey::TextError),
            warning_text: ui(UiKey::TextWarning),
            checker_light: ui(UiKey::ImageCheckerLight),
            checker_dark: ui(UiKey::ImageCheckerDark),
            projects: UiKey::PROJECTS.map(ui),
            project_text: ui(UiKey::ProjectForeground),
            editor_background: editor(EditorKey::Background),
            editor_selection: editor(EditorKey::Selection),
            symbol_highlight: editor(EditorKey::SymbolHighlight),
            symbol_highlight_write: editor(EditorKey::SymbolHighlightWrite),
            bracket_matched_background: editor(EditorKey::BracketMatchedBackground),
            bracket_matched: editor(EditorKey::BracketMatchedForeground),
            bracket_unmatched_background: editor(EditorKey::BracketUnmatchedBackground),
            bracket_unmatched: editor(EditorKey::BracketUnmatchedForeground),
            diagnostic_error: editor(EditorKey::DiagnosticError),
            diagnostic_warning: editor(EditorKey::DiagnosticWarning),
            diagnostic_info: editor(EditorKey::DiagnosticInfo),
            diagnostic_hint: editor(EditorKey::DiagnosticHint),
            diagnostic_typo: editor(EditorKey::DiagnosticTypo),
            completion_function: editor(EditorKey::CompletionFunction),
            completion_variable: editor(EditorKey::CompletionVariable),
            completion_field: editor(EditorKey::CompletionField),
            completion_constant: editor(EditorKey::CompletionConstant),
            completion_type: editor(EditorKey::CompletionType),
            completion_interface: editor(EditorKey::CompletionInterface),
            completion_module: editor(EditorKey::CompletionModule),
            completion_keyword: editor(EditorKey::CompletionKeyword),
            completion_snippet: editor(EditorKey::CompletionSnippet),
            completion_other: editor(EditorKey::CompletionOther),
            vcs_modified: ui(UiKey::VcsModified),
            vcs_added: ui(UiKey::VcsAdded),
            vcs_deleted: ui(UiKey::VcsDeleted),
            vcs_unversioned: ui(UiKey::VcsUnversioned),
            vcs_ignored: ui(UiKey::VcsIgnored),
            vcs_conflict: ui(UiKey::VcsConflict),
            vcs_incoming: ui(UiKey::VcsIncoming),
            vcs_outgoing: ui(UiKey::VcsOutgoing),
            vcs_current_branch: ui(UiKey::VcsCurrentBranch),
            gutter_added: editor(EditorKey::GutterAdded),
            gutter_modified: editor(EditorKey::GutterModified),
            gutter_deleted: editor(EditorKey::GutterDeleted),
            diff_inserted: editor(EditorKey::DiffInserted),
            diff_deleted: editor(EditorKey::DiffDeleted),
            diff_modified: editor(EditorKey::DiffModified),
            diff_inserted_word: editor(EditorKey::DiffInsertedWord),
            diff_deleted_word: editor(EditorKey::DiffDeletedWord),
            diff_modified_word: editor(EditorKey::DiffModifiedWord),
            diff_filler: editor(EditorKey::DiffFiller),
            breakpoint: editor(EditorKey::Breakpoint),
            breakpoint_disabled: editor(EditorKey::BreakpointDisabled),
            execution_line: editor(EditorKey::ExecutionLine),
            execution_arrow: editor(EditorKey::ExecutionArrow),
            annotation_recent: editor(EditorKey::AnnotationRecent),
            annotation_old: editor(EditorKey::AnnotationOld),
            coverage_covered: editor(EditorKey::CoverageCovered),
            coverage_partly: editor(EditorKey::CoveragePartly),
            coverage_not_covered: editor(EditorKey::CoverageNotCovered),
            unused: editor(EditorKey::Unused),
            whitespace: editor(EditorKey::Whitespace),
            indent_guide: editor(EditorKey::IndentGuide),
            indent_guide_selected: editor(EditorKey::IndentGuideSelected),
            right_margin: editor(EditorKey::RightMargin),
            graph: UiKey::GRAPH.map(ui),
            dark: theme.dark,
            styles,
        }
    }

    pub fn zen_dark() -> IdePalette {
        IdePalette::from_theme(&ResolvedTheme::zen_dark())
    }

    pub fn of(ctx: &egui::Context) -> IdePalette {
        ctx.data(|data| data.get_temp::<IdePalette>(egui::Id::unique(PALETTE_ID))).unwrap_or_else(IdePalette::zen_dark)
    }

    pub fn style(&self, key: SyntaxKey) -> CodeStyle {
        self.styles[zen_theme::Key::index(key)]
    }

    pub fn syntax(&self, kind: HighlightKind) -> CodeStyle {
        self.style(crate::theme_bridge::syntax_key(kind))
    }

    pub fn project(&self, name: &str) -> Color32 {
        let hash = name.bytes().fold(0u32, |hash, byte| hash.wrapping_mul(31).wrapping_add(u32::from(byte)));
        self.projects[hash as usize % self.projects.len()]
    }

    pub fn problem(&self, severity: &str) -> Color32 {
        match severity {
            "error" => self.diagnostic_error,
            "warning" => self.diagnostic_warning,
            "weakWarning" => self.diagnostic_hint,
            "information" => self.diagnostic_info,
            "typo" => self.diagnostic_typo,
            _ => self.diagnostic_hint,
        }
    }

    pub fn completion_mark(&self, kind: &str) -> (&'static str, Color32) {
        match kind {
            "function" | "method" | "constructor" => ("f", self.completion_function),
            "variable" | "value" => ("v", self.completion_variable),
            "field" | "property" => ("f", self.completion_field),
            "constant" | "enumMember" => ("c", self.completion_constant),
            "struct" | "class" | "enum" => ("s", self.completion_type),
            "interface" | "typeParameter" => ("i", self.completion_interface),
            "module" => ("p", self.completion_module),
            "keyword" | "command" => ("k", self.completion_keyword),
            "table" => ("t", self.completion_type),
            "view" => ("v", self.completion_interface),
            "column" => ("c", self.completion_field),
            "schema" => ("s", self.completion_module),
            "alias" => ("a", self.completion_variable),
            "join" => ("j", self.completion_snippet),
            "key" => ("k", self.completion_constant),
            "snippet" | "template" | "postfix" => ("t", self.completion_snippet),
            _ => ("•", self.completion_other),
        }
    }

    fn popup_frame(&self) -> egui::Frame {
        egui::Frame::new().fill(self.popup).stroke(Stroke::new(1.0, self.popup_border))
    }

    pub fn apply(&self, ctx: &egui::Context) {
        let mut visuals = if self.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        visuals.panel_fill = self.frame;
        visuals.window_fill = self.popup;
        visuals.extreme_bg_color = self.island;
        visuals.selection.bg_fill = self.text_field_selection;
        visuals.selection.stroke = Stroke::new(1.0, self.text);
        visuals.text_cursor.stroke = Stroke::new(2.0, self.text_field_caret);
        visuals.override_text_color = Some(self.text);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.hover);
        visuals.widgets.hovered.weak_bg_fill = self.popup_hover;
        visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        visuals.window_corner_radius = CornerRadius::same(10);
        visuals.menu_corner_radius = CornerRadius::same(8);
        ctx.set_theme(if self.dark { egui::Theme::Dark } else { egui::Theme::Light });
        ctx.set_visuals(visuals);
        ctx.data_mut(|data| data.insert_temp(egui::Id::unique(PALETTE_ID), *self));
        ctx.all_styles_mut(|style| {
            style.spacing.scroll = egui::style::ScrollStyle {
                bar_width: 12.0,
                floating_width: 8.0,
                handle_min_length: 24.0,
                dormant_handle_opacity: 0.0,
                dormant_background_opacity: 0.0,
                active_background_opacity: 0.0,
                interact_background_opacity: 0.12,
                interact_handle_opacity: 0.85,
                ..egui::style::ScrollStyle::floating()
            };
            style.text_styles.insert(egui::TextStyle::Body, theme::regular(13.0));
            style.text_styles.insert(egui::TextStyle::Button, theme::regular(13.0));
            style.text_styles.insert(egui::TextStyle::Monospace, FontId::monospace(13.0));
        });
    }
}

mod metrics {
    pub(super) const TOOLBAR: f32 = 40.0;
    pub(super) const WINDOW_BUTTONS: f32 = 78.0;
    pub(super) const STRIPE: f32 = 40.0;
    pub(super) const STRIPE_BUTTON: f32 = 30.0;
    pub(super) const STATUS_BAR: f32 = 28.0;
    pub(super) const GAP: f32 = 5.0;
    pub(super) const ISLAND_RADIUS: u8 = 10;
    pub(super) const TOOL_HEADER: f32 = 36.0;
    pub(super) const TABS: f32 = 38.0;
    pub(super) const PROJECT_TABS: f32 = 32.0;
    pub(super) const BANNER: f32 = 40.0;
    pub(super) const BREADCRUMBS: f32 = 28.0;
    pub(super) const NAVIGATION_BAR: f32 = 28.0;
    pub(super) const TREE_ROW: f32 = 24.0;
    pub(super) const TREE_INDENT: f32 = 19.25;
    pub(super) const TREE_ARROW: f32 = 7.5;
    pub(super) const TREE_ICON: f32 = 29.0;
    pub(super) const TREE_TEXT: f32 = 40.5;

    const LINE_HEIGHT: f32 = 22.0;

    pub(super) mod gutter {
        pub(in super::super) const NUMBERS_LEFT: f32 = 13.75;
        pub(in super::super) const NUMBERS_TO_FOLD: f32 = 30.0;
        pub(in super::super) const FOLD_TO_SEPARATOR: f32 = 12.25;
        pub(in super::super) const SEPARATOR: f32 = 1.0;
        pub(in super::super) const TEXT_GAP: f32 = 1.5;
        pub(in super::super) const ARROW: f32 = 13.0;
        pub(in super::super) const BREAKPOINT: f32 = 7.0 / 22.0;
    }

    #[derive(Clone, Debug, PartialEq)]
    pub(super) struct EditorMetrics {
        pub(super) font: egui::FontId,
        pub(super) line_height: f32,
        pub(super) char_width: f32,
        pub(super) tab_size: usize,
    }

    impl EditorMetrics {
        pub(super) fn new(ctx: &egui::Context, font: &super::settings::EditorFont, tab_size: u8) -> Self {
            let (size, line_height) = scaled(font.size, font.line_spacing, super::ide_zoom::font_zoom(ctx));
            let font = egui::FontId::new(size, editor_family(ctx));
            let char_width = ctx.fonts_mut(|fonts| fonts.glyph_width(&font, 'M'));
            Self { font, line_height, char_width, tab_size: usize::from(tab_size.max(1)) }
        }

        pub(super) fn gutter_font(&self) -> egui::FontId {
            egui::FontId::new(self.font.size * (1.0 - 0.5 / 13.0), self.font.family.clone())
        }
    }

    pub(super) fn editor_family(ctx: &egui::Context) -> egui::FontFamily {
        let editor = crate::theme::family::editor();
        if ctx.fonts(|fonts| fonts.definitions().families.contains_key(&editor)) {
            editor
        } else {
            egui::FontFamily::Monospace
        }
    }

    pub(super) fn scaled(size: f32, spacing: f32, zoom: f32) -> (f32, f32) {
        let size = size.clamp(6.0, 48.0);
        let spacing = spacing.clamp(0.6, 3.0);
        let line_height = (LINE_HEIGHT * size / 13.0 * spacing / 1.2).round().max(1.0);
        let zoom = if zoom > 0.0 { zoom } else { 1.0 };
        (size / zoom, line_height / zoom)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_default_font_keeps_lines_22_pixels_high() {
            assert_eq!(scaled(13.0, 1.2, 1.0), (13.0, 22.0));
            assert_eq!(scaled(13.0, 1.0, 1.0), (13.0, 18.0));
            assert_eq!(scaled(16.0, 1.5, 1.0), (16.0, 34.0));
            assert_eq!(scaled(100.0, 9.0, 1.0), scaled(48.0, 3.0, 1.0), "the size and spacing are kept in range");
        }

        #[test]
        fn the_ui_zoom_leaves_the_editor_alone() {
            for zoom in [0.75, 1.25, 2.0] {
                let (size, line_height) = scaled(13.0, 1.2, zoom);
                assert!((size * zoom - 13.0).abs() < 1e-4, "{zoom}");
                assert!((line_height * zoom - 22.0).abs() < 1e-4, "{zoom}");
            }
        }

        #[test]
        fn metrics_measure_the_font_without_the_zoom() {
            let ctx = egui::Context::default();
            crate::theme::install_fonts(&ctx);
            ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
            let font = super::super::settings::EditorFont::default();
            let plain = EditorMetrics::new(&ctx, &font, 4);
            assert_eq!(plain.font.family, crate::theme::family::editor());
            assert_eq!((plain.font.size, plain.line_height, plain.tab_size), (13.0, 22.0, 4));
            assert!(
                (plain.char_width - 13.0 * 0.6).abs() < 0.2,
                "the bundled font is 0.6 em wide: {}",
                plain.char_width
            );
            ctx.set_zoom_factor(1.5);
            ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
            let zoomed = EditorMetrics::new(&ctx, &font, 8);
            assert!((zoomed.line_height * 1.5 - 22.0).abs() < 1e-4);
            assert!((zoomed.char_width * 1.5 - plain.char_width).abs() < 0.1);
            assert_eq!(zoomed.tab_size, 8);
        }
    }
}

#[derive(Default)]
pub struct IdeProduct {
    pub name: &'static str,
    pub id: &'static str,
    pub version: &'static str,
    pub highlighters: Vec<Arc<dyn Highlighter>>,
    pub project_models: Vec<Arc<dyn zen_workbench::model::ModelProvider>>,
    pub outliners: Vec<Arc<dyn zen_syntax::Outliner>>,
    pub run_providers: Vec<Arc<dyn zen_run::RunProvider>>,
    pub inspectors: Vec<Arc<dyn zen_project::inspect::Inspector>>,
    pub language_servers: Vec<zen_workbench::language::servers::ServerDeclaration>,
    pub templates: Vec<zen_workbench::templates::Builtin>,
    pub plugins: Option<PluginConfig>,
    pub mcp_port: u16,
    pub modules: Vec<zen_modules::BuiltinModule>,
    pub areas: Vec<&'static str>,
    pub settings_pages: Vec<zen_modules::SettingsPage>,
    pub code_styles: Vec<zen_modules::CodeStylePage>,
    pub folding_options: Vec<zen_modules::folding::FoldingOptions>,
    pub debugger: Option<zen_modules::DebuggerOffer>,
}

type Overlays = (Option<egui::LayerId>, [bool; 25]);

pub struct IdeView {
    product: Arc<IdeProduct>,
    plugins: Arc<Plugins>,
    markdown_views: markdown_view::MarkdownViews,
    plugin_projects_seen: Option<(PathBuf, u64)>,
    palette: IdePalette,
    project_open: bool,
    structure_open: bool,
    expanded: BTreeSet<PathBuf>,
    listings: HashMap<PathBuf, Vec<zen_vfs::Entry>>,
    libraries: Option<libraries_tree::Libraries>,
    scratch_folders: Vec<PathBuf>,
    notifications: notifications::NotificationsUi,
    caret: Option<(usize, usize)>,
    requests: Vec<Request>,
    overview: Overview,
    selection: selection::TreeSelection,
    visible_rows: Vec<PathBuf>,
    reveal_lead: bool,
    pending_tree_action: Option<TreeAction>,
    always_select_opened: bool,
    followed_file: Option<PathBuf>,
    show_excluded: bool,
    tree_focused: bool,
    tree_menu: Option<TreeMenuInfo>,
    tree_bookmarks: (u64, HashMap<PathBuf, Option<char>>),
    menu_request: Option<(PathBuf, bool)>,
    seen_revision: u64,
    tree_exclusions: (u64, zen_index::Exclusions),
    tree_resource_roots: Vec<PathBuf>,
    file_types_seen: u64,
    new_entry: Option<NewEntry>,
    delete_entry: Option<Vec<PathBuf>>,
    exit_prompt: Option<Vec<zen_workbench::exit::RunningWork>>,
    restart_prompt: Option<RestartPrompt>,
    invalidate_dialog: Option<invalidate_dialog::InvalidateDialog>,
    goto_line: Option<(String, bool)>,
    settings: settings::SettingsDialog,
    #[cfg(not(target_arch = "wasm32"))]
    terminal_panes: HashMap<u64, crate::terminal::PaneView>,
    #[cfg(not(target_arch = "wasm32"))]
    terminal_palette: crate::terminal::TerminalPalette,
    terminal_rename: Option<(usize, String, bool)>,
    terminal_link: Option<terminal_link_dialog::LinkDialog>,
    terminal_focus: Option<u64>,
    terminal_drawn: Vec<(u64, egui::Id, Rect)>,
    terminal_keyboard: bool,
    #[cfg(not(target_arch = "wasm32"))]
    terminal_header: terminal_window::TerminalHeader,
    scroll_lock: Option<(ScrollAxis, f64)>,
    last_scroll: f64,
    groups_ui: editor_groups::GroupsUi,
    workspace_host: workspace_host::WorkspaceHost,
    image_textures: Vec<image_view::ImageTexture>,
    editor_view: editor_view::EditorView,
    focus_return: focus_return::FocusReturn,
    held_typing: Vec<egui::Event>,
    typing_released: bool,
    held_answers: Vec<agent::Pressed>,
    overlay_shown: Option<Overlays>,
    find_window: find_window::FindWindow,
    hierarchy_window: hierarchy_window::HierarchyWindow,
    todo_window: todo_window::TodoWindow,
    inspect_window: inspect_window::InspectWindow,
    inspect_dialog: Option<inspect_window::InspectDialog>,
    build_tools: build_tool_window::BuildToolWindow,
    build_sync: build_sync_window::BuildSyncUi,
    dependency_analyzer: dependency_analyzer::DependencyAnalyzerUi,
    ant: ant_tool_window::AntToolWindow,
    npm: npm_tool_window::NpmToolWindow,
    plugin_tools: plugin_tool_window::PluginToolWindows,
    index_views: std::collections::BTreeMap<&'static str, plugin_index_view::IndexView>,
    endpoints: endpoints_window::EndpointsWindow,
    dependencies: dependencies_tool_window::DependenciesToolWindow,
    structure_window: structure_window::StructureWindow,
    breadcrumbs: breadcrumbs::Breadcrumbs,
    navigation_bar: navigation_bar::NavigationBar,
    rename: rename_dialog::RenameDialog,
    refactoring: refactoring_dialog::RefactoringDialog,
    local_history: local_history::LocalHistoryDialog,
    macro_dialogs: macro_dialogs::MacroDialogs,
    launcher_dialog: Option<launcher_dialog::LauncherDialog>,
    artifacts_popup: Option<artifacts_popup::ArtifactsPopup>,
    connection_dialog: Option<connection_dialog::ConnectionDialog>,
    import_dialog: Option<import_dialog::ImportDialog>,
    export_dialog: Option<export_dialog::ExportDialog>,
    backup_dialog: Option<backup_dialog::BackupDialog>,
    csv_import_dialog: Option<csv_import_dialog::CsvImportDialog>,
    transfer_dialog: Option<transfer_dialog::TransferDialog>,
    transaction_dialogs: transaction_dialog::TransactionDialogs,
    ssh_dialog: ssh_dialog::SshDialog,
    associate_dialog: Option<associate_dialog::AssociateDialog>,
    new_project: Option<new_project_dialog::NewProjectDialog>,
    clone: Option<clone_dialog::CloneDialog>,
    create_test: Option<create_test_dialog::CreateTestDialog>,
    test_history: test_history_popup::TestHistoryPopup,
    errors_dialog: Option<internal_errors::ErrorsDialog>,
    encoding_popup: Option<encoding_popup::EncodingPopup>,
    encoding_choice: Option<encoding_popup::EncodingChoice>,
    main_screen: Option<Rect>,
    starters: Option<starters_dialog::StartersDialog>,
    new_from_template: Option<file_template_view::NewFromTemplate>,
    save_as_template: Option<file_template_view::SaveAsTemplate>,
    new_editor_config: Option<editor_config_view::NewEditorConfigDialog>,
    editor_config_files: Option<editor_config_view::EditorConfigFiles>,
    ide_zoom_dialog: bool,
    new_popup: Option<new_popup::NewPopup>,
    new_items: new_popup::NewItemsCache,
    project_header: Option<Rect>,
    manage_settings: manage_settings::ManageSettings,
    external_tool_prompt: Option<external_tools::ToolPrompt>,
    rerun_confirmation: Option<run_launch::RerunQuestion>,
    activity_monitor: Option<activity_monitor::ActivityMonitorDialog>,
    diagnostics: diagnostics_dialogs::DiagnosticsDialogs,
    help: productivity::Help,
    productivity: Option<productivity::ProductivityDialog>,
    tip: Option<tips::TipDialog>,
    status_menu_at: Option<Pos2>,
    run_window: run_window::RunWindow,
    services_window: services_window::ServicesWindow,
    coverage_window: coverage_window::CoverageWindow,
    debug_window: debug_window::DebugWindow,
    run_widget: run_widget::RunWidget,
    run_configurations: run_configurations::RunConfigurationsDialog,
    find_bar: find_bar::FindBar,
    script_bar: sql_script_bar::ScriptBar,
    search_everywhere: search_everywhere::EverywherePopup,
    double_shift: search_everywhere::DoubleShift,
    double_control: search_everywhere::DoubleShift,
    run_anything: run_anything::RunAnythingPopup,
    font_zoom: font_zoom::FontZoom,
    recent_files: recent_files::RecentFiles,
    shortcut_modifiers: Option<egui::Modifiers>,
    recent_locations: recent_locations::RecentLocations,
    file_structure: file_structure::FileStructure,
    select_in: select_in::SelectIn,
    projects: projects_popup::ProjectsPopup,
    paste_history: paste_history::PasteHistory,
    scratch: scratch_popups::ScratchPopups,
    recent_usages: recent_usages::RecentUsages,
    usages_settings: find_usages_settings::UsagesSettings,
    bookmarks: bookmark_dialogs::BookmarkDialogs,
    breakpoint_dialogs: breakpoints_dialog::BreakpointDialogs,
    evaluations: evaluate::Evaluations,
    smart_step: smart_step::SmartStep,
    stream_traces: stream_trace::StreamTraces,
    bookmarks_window: bookmarks_window::BookmarksWindow,
    quick_switch: quick_switch::QuickSwitch,
    find_in_files: find_in_files::FindInFiles,
    #[cfg(not(target_arch = "wasm32"))]
    watcher: Option<(PathBuf, zen_vfs::Watcher)>,
    #[cfg(not(target_arch = "wasm32"))]
    watch_retry: zen_vfs::WatchRetry,
    pub(super) opened_as: Option<PathBuf>,
    free_scroll: Vec<Rect>,
    lsp: language_servers::LanguageServers,
    bottom: Option<&'static str>,
    tool_windows: tool_windows::ToolWindowLayout,
    project_drag: Option<(PathBuf, f32)>,
    project_place: Option<usize>,
    stripes: tool_stripes::Stripes,
    detached_open: Vec<(&'static str, tool_windows::ViewMode, Option<tool_windows::Bounds>)>,
    layout_form: Option<view_modes::LayoutForm>,
    problems_project: bool,
    languages_wake: bool,
    drawn: bool,
    toolchain_watch: toolchain_watch::ToolchainWatch,
    safe_mode: bool,
    update_details: bool,
    vcs: vcs::VcsUi,
    tasks_ui: background_tasks::TasksUi,
    caffeine_ui: caffeine::CaffeineUi,
    indent_ui: indent_widget::IndentUi,
    commit_open: bool,
    commit_window: commit_window::CommitWindow,
    rollback: Option<commit_window::RollbackDialog>,
    changes_dialog: Option<commit_window::ChangesDialog>,
    patch_dialog: Option<commit_window::PatchDialog>,
    diff: Option<diff_view::DiffTab>,
    diff_unified: bool,
    folder_diff: Option<folder_diff::FolderDiffTab>,
    bundle: Option<bundle_view::BundleTab>,
    group_bundles: bool,
    front: Option<front::Front>,
    behind: Option<front::Front>,
    git_window: git_window::GitWindow,
    git_workspace: git_workspace::GitWorkspaceView,
    diff_from_workspace: bool,
    rebase_view: git_workspace::rebase::RebaseView,
    conflict_tab: git_workspace::conflicts::ConflictTab,
    agent_keys: Vec<agent::Queued>,
    agent_keys_last_frame: bool,
    appearance: appearance::Appearance,
}

fn locked_scroll(lock: &mut Option<(ScrollAxis, f64)>, delta: Vec2, now: f64, free: bool) -> Vec2 {
    const GESTURE_END: f64 = 0.25;
    if delta == Vec2::ZERO {
        if lock.is_some_and(|(_, last)| now - last > GESTURE_END) {
            *lock = None;
        }
        return delta;
    }
    if free {
        *lock = None;
        return delta;
    }
    let axis = match *lock {
        Some((axis, last)) if now - last <= GESTURE_END => axis,
        _ if delta.x.abs() > delta.y.abs() => ScrollAxis::Horizontal,
        _ => ScrollAxis::Vertical,
    };
    *lock = Some((axis, now));
    match axis {
        ScrollAxis::Horizontal => vec2(delta.x, 0.0),
        ScrollAxis::Vertical => vec2(0.0, delta.y),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScrollAxis {
    Horizontal,
    Vertical,
}

struct NewEntry {
    package: bool,
    directory: PathBuf,
    rename: Option<PathBuf>,
    folder: bool,
    name: String,
    image: Option<ClipboardImage>,
    focused: bool,
    preview: Option<egui::TextureHandle>,
}

struct ClipboardImage {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

#[derive(Default)]
struct TreeFrame {
    open_file: Option<PathBuf>,
    preview_file: Option<PathBuf>,
    visible: Vec<PathBuf>,
    hovered_folder: Option<PathBuf>,
    action: Option<TreeAction>,
    menu_for: Option<PathBuf>,
}

enum TreeAction {
    New(PathBuf, new_popup::NewChoice),
    NewFile(PathBuf),
    NewDirectory(PathBuf),
    NewPackage(PathBuf),
    Paste(PathBuf),
    Copy(Vec<PathBuf>),
    Cut(Vec<PathBuf>),
    CopyPath(Vec<PathBuf>),
    Delete(Vec<PathBuf>),
    Rename(PathBuf),
    Reformat(PathBuf),
    OpenRightSplit(PathBuf),
    Reveal(PathBuf),
    OpenInTerminal(PathBuf),
    ShowHistory(PathBuf),
    RepairFile(PathBuf),
    PutLabel,
    VcsHistory(PathBuf),
    Annotate(PathBuf),
    ShowDiff(PathBuf),
    Rollback(Vec<PathBuf>),
    ChangeLanguage(PathBuf),
    Move(PathBuf),
    RunScratch(PathBuf),
    RunTarget(PathBuf, u32),
    GitAdd(Vec<PathBuf>),
    CommitFiles(Vec<PathBuf>),
    Reload(Vec<PathBuf>),
    CompareWith(PathBuf),
    CompareFolders(PathBuf, PathBuf),
    CompareFiles(Vec<PathBuf>),
    ExternalTool(String, PathBuf),
    CompareWithEditor(PathBuf),
    Ignore(Vec<PathBuf>),
    ToggleBookmarks(Vec<PathBuf>),
    BookmarkMnemonic(PathBuf),
    DeleteBookmarks(Vec<PathBuf>),
    Associate(PathBuf),
    OverrideType(Vec<PathBuf>, String),
    RevertOverride(Vec<PathBuf>),
    MarkExcluded(PathBuf),
    CancelExclusion(PathBuf),
    MarkResourceRoot(PathBuf),
    UnmarkResourceRoot(PathBuf),
}

fn override_types() -> Vec<(String, String)> {
    let mut types: Vec<(String, String)> = zen_workbench::file_types::list()
        .types
        .into_iter()
        .filter(|ty| ty.id != zen_vfs::file_types::DETECTED && ty.id != zen_vfs::file_types::NATIVE)
        .map(|ty| (ty.id, ty.name))
        .collect();
    types.sort_by_key(|(_, name)| name.to_lowercase());
    types
}

#[derive(Clone, Debug, PartialEq)]
struct RestartPrompt {
    running: Vec<zen_workbench::exit::RunningWork>,
    invalidate: Option<zen_workbench::maintenance::InvalidateParams>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum FolderMark {
    #[default]
    Unknown,
    Excludable,
    Excluded,
    ExcludedOtherwise,
}

#[derive(Clone, Debug, Default)]
struct TreeMenuInfo {
    path: PathBuf,
    can_paste: bool,
    formatter: bool,
    editor: Option<PathBuf>,
    folder: FolderMark,
    resource_root: bool,
    types: Vec<(String, String)>,
    new_items: Vec<new_popup::NewItem>,
    tools: Vec<zen_workbench::external_tools::ExternalTool>,
    scratch_run: Option<String>,
    run_targets: Vec<(String, u32)>,
}

impl IdeView {
    fn new(product: Arc<IdeProduct>, plugins: Arc<Plugins>) -> Self {
        Self {
            product,
            plugins,
            plugin_projects_seen: None,
            markdown_views: markdown_view::MarkdownViews::default(),
            palette: IdePalette::zen_dark(),
            project_open: true,
            structure_open: false,
            expanded: BTreeSet::new(),
            listings: HashMap::new(),
            libraries: None,
            scratch_folders: Vec::new(),
            notifications: notifications::NotificationsUi::default(),
            caret: None,
            requests: Vec::new(),
            overview: Overview::default(),
            selection: selection::TreeSelection::default(),
            visible_rows: Vec::new(),
            reveal_lead: false,
            pending_tree_action: None,
            always_select_opened: false,
            followed_file: None,
            show_excluded: true,
            tree_focused: false,
            tree_menu: None,
            tree_bookmarks: (0, HashMap::new()),
            menu_request: None,
            seen_revision: 0,
            tree_exclusions: (0, zen_index::Exclusions::default()),
            tree_resource_roots: Vec::new(),
            file_types_seen: zen_vfs::file_types::generation(),
            new_entry: None,
            delete_entry: None,
            exit_prompt: None,
            restart_prompt: None,
            invalidate_dialog: None,
            goto_line: None,
            settings: settings::SettingsDialog::default(),
            #[cfg(not(target_arch = "wasm32"))]
            terminal_panes: HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            terminal_palette: crate::terminal::TerminalPalette::zen_dark(),
            terminal_rename: None,
            terminal_link: None,
            terminal_focus: None,
            terminal_drawn: Vec::new(),
            terminal_keyboard: false,
            #[cfg(not(target_arch = "wasm32"))]
            terminal_header: terminal_window::TerminalHeader::default(),
            scroll_lock: None,
            last_scroll: f64::NEG_INFINITY,
            image_textures: Vec::new(),
            editor_view: editor_view::EditorView::default(),
            focus_return: focus_return::FocusReturn::default(),
            held_typing: Vec::new(),
            typing_released: false,
            held_answers: Vec::new(),
            overlay_shown: None,
            find_window: find_window::FindWindow::default(),
            hierarchy_window: hierarchy_window::HierarchyWindow::default(),
            todo_window: todo_window::TodoWindow::default(),
            inspect_window: inspect_window::InspectWindow::default(),
            project_drag: None,
            project_place: None,
            inspect_dialog: None,
            build_tools: build_tool_window::BuildToolWindow::default(),
            build_sync: build_sync_window::BuildSyncUi::default(),
            dependency_analyzer: dependency_analyzer::DependencyAnalyzerUi::default(),
            ant: ant_tool_window::AntToolWindow::default(),
            npm: npm_tool_window::NpmToolWindow::default(),
            plugin_tools: plugin_tool_window::PluginToolWindows::default(),
            index_views: std::collections::BTreeMap::new(),
            endpoints: endpoints_window::EndpointsWindow::default(),
            dependencies: dependencies_tool_window::DependenciesToolWindow::default(),
            structure_window: structure_window::StructureWindow::default(),
            breadcrumbs: breadcrumbs::Breadcrumbs::default(),
            navigation_bar: navigation_bar::NavigationBar::default(),
            rename: rename_dialog::RenameDialog::default(),
            refactoring: refactoring_dialog::RefactoringDialog::default(),
            local_history: local_history::LocalHistoryDialog::default(),
            macro_dialogs: macro_dialogs::MacroDialogs::default(),
            launcher_dialog: None,
            artifacts_popup: None,
            connection_dialog: None,
            import_dialog: None,
            export_dialog: None,
            backup_dialog: None,
            csv_import_dialog: None,
            transfer_dialog: None,
            transaction_dialogs: transaction_dialog::TransactionDialogs::default(),
            ssh_dialog: ssh_dialog::SshDialog::default(),
            associate_dialog: None,
            new_project: None,
            clone: None,
            create_test: None,
            test_history: test_history_popup::TestHistoryPopup::default(),
            errors_dialog: None,
            encoding_popup: None,
            encoding_choice: None,
            main_screen: None,
            starters: None,
            new_from_template: None,
            save_as_template: None,
            new_editor_config: None,
            editor_config_files: None,
            ide_zoom_dialog: false,
            new_popup: None,
            new_items: new_popup::NewItemsCache::default(),
            project_header: None,
            manage_settings: manage_settings::ManageSettings::default(),
            external_tool_prompt: None,
            rerun_confirmation: None,
            activity_monitor: None,
            diagnostics: Default::default(),
            help: productivity::Help::default(),
            productivity: None,
            tip: None,
            status_menu_at: None,
            run_window: run_window::RunWindow::default(),
            services_window: services_window::ServicesWindow::default(),
            coverage_window: coverage_window::CoverageWindow::default(),
            debug_window: debug_window::DebugWindow::default(),
            run_widget: run_widget::RunWidget::default(),
            run_configurations: run_configurations::RunConfigurationsDialog::default(),
            find_bar: find_bar::FindBar::default(),
            script_bar: sql_script_bar::ScriptBar::default(),
            search_everywhere: search_everywhere::EverywherePopup::default(),
            double_shift: search_everywhere::DoubleShift::default(),
            double_control: search_everywhere::DoubleShift::of(search_everywhere::Modifier::Control),
            run_anything: run_anything::RunAnythingPopup::default(),
            font_zoom: font_zoom::FontZoom::default(),
            recent_files: recent_files::RecentFiles::default(),
            shortcut_modifiers: None,
            recent_locations: recent_locations::RecentLocations::default(),
            file_structure: file_structure::FileStructure::default(),
            select_in: select_in::SelectIn::default(),
            projects: projects_popup::ProjectsPopup::default(),
            paste_history: paste_history::PasteHistory::default(),
            scratch: scratch_popups::ScratchPopups::default(),
            recent_usages: recent_usages::RecentUsages::default(),
            usages_settings: find_usages_settings::UsagesSettings::default(),
            bookmarks: bookmark_dialogs::BookmarkDialogs::default(),
            breakpoint_dialogs: breakpoints_dialog::BreakpointDialogs::default(),
            evaluations: evaluate::Evaluations::default(),
            smart_step: smart_step::SmartStep::default(),
            stream_traces: stream_trace::StreamTraces::default(),
            bookmarks_window: bookmarks_window::BookmarksWindow::default(),
            quick_switch: quick_switch::QuickSwitch::default(),
            find_in_files: find_in_files::FindInFiles::default(),
            #[cfg(not(target_arch = "wasm32"))]
            watcher: None,
            #[cfg(not(target_arch = "wasm32"))]
            watch_retry: zen_vfs::WatchRetry::default(),
            opened_as: None,
            groups_ui: editor_groups::GroupsUi::default(),
            workspace_host: workspace_host::WorkspaceHost::with_features(),
            free_scroll: Vec::new(),
            update_details: false,
            vcs: vcs::VcsUi::default(),
            tasks_ui: background_tasks::TasksUi::default(),
            caffeine_ui: caffeine::CaffeineUi::default(),
            indent_ui: indent_widget::IndentUi::default(),
            commit_open: false,
            commit_window: commit_window::CommitWindow::default(),
            rollback: None,
            changes_dialog: None,
            patch_dialog: None,
            diff: None,
            diff_unified: false,
            folder_diff: None,
            bundle: None,
            group_bundles: false,
            front: None,
            behind: None,
            git_window: git_window::GitWindow::default(),
            git_workspace: git_workspace::GitWorkspaceView::default(),
            diff_from_workspace: false,
            rebase_view: git_workspace::rebase::RebaseView::default(),
            conflict_tab: git_workspace::conflicts::ConflictTab::default(),
            agent_keys: Vec::new(),
            agent_keys_last_frame: false,
            appearance: appearance::Appearance::default(),
            bottom: None,
            tool_windows: tool_windows::ToolWindowLayout::default(),
            stripes: tool_stripes::Stripes::default(),
            detached_open: Vec::new(),
            layout_form: None,
            problems_project: false,
            languages_wake: false,
            drawn: false,
            lsp: language_servers::LanguageServers::default(),
            toolchain_watch: toolchain_watch::ToolchainWatch::default(),
            safe_mode: false,
        }
    }

    fn show(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        #[cfg(not(target_arch = "wasm32"))]
        let profile = crate::capture::profile_begin(ui.ctx());
        let held = std::mem::take(&mut self.held_typing);
        self.typing_released = !held.is_empty();
        filter_popups::release_typing(ui, held);
        let pressed = self.agent_keys(ui, session);
        let keys_now =
            !self.agent_keys.is_empty() || pressed.iter().chain(&self.held_answers).any(agent::Pressed::delivered);
        let by_keys = keys_now || std::mem::take(&mut self.agent_keys_last_frame);
        self.agent_keys_last_frame = keys_now;
        if by_keys {
            zen_core::macros::as_agent(session, |session| self.show_frame(ui, registry, session));
            self.refuse_agent_settings();
        } else {
            self.show_frame(ui, registry, session);
        }
        self.held_answers.extend(pressed);
        if self.held_typing.is_empty() {
            let answers = std::mem::take(&mut self.held_answers);
            self.answer_keys(session, answers);
        }
        #[cfg(not(target_arch = "wasm32"))]
        crate::capture::profile_end(profile);
    }

    fn show_frame(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        self.appearance.frame(ui.ctx());
        self.watch_escape(ui.ctx(), session);
        self.show_window(ui, registry, session);
        self.follow_escape(ui.ctx());
        self.remember_copies(ui.ctx(), registry, session);
        self.return_focus(ui.ctx());
    }

    fn remember_copies(&self, ctx: &egui::Context, registry: &Registry, session: &mut Session) {
        let copied: Vec<String> = ctx.output(|output| {
            output
                .commands
                .iter()
                .filter_map(|command| match command {
                    egui::OutputCommand::CopyText(text)
                        if !text.is_empty() && text.len() <= zen_workbench::clipboard_history::MAX_ENTRY_BYTES =>
                    {
                        Some(text.clone())
                    }
                    _ => None,
                })
                .collect()
        });
        for text in copied {
            let _ = registry.dispatch(session, "editor.clipboardHistory.add", serde_json::json!({ "text": text }));
        }
    }

    fn return_focus(&mut self, ctx: &egui::Context) {
        self.overlay_shown = self.overlays(ctx);
        let overlay = self.overlay_open(ctx);
        let editor = self.editor_view.has_focus_in(ctx);
        let focused = ctx.memory(egui::Memory::focused);
        match self.focus_return.frame(overlay, editor, focused) {
            Some(focus_return::Owner::Editor) => self.editor_view.focus(),
            Some(focus_return::Owner::Widget(id)) => ctx.memory_mut(|memory| memory.request_focus(id)),
            None => return,
        }
        ctx.request_repaint();
    }

    pub(super) fn step(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        now: f64,
        focused: bool,
    ) {
        if session.state::<Workbench>().is_some() {
            for message in session.state_mut::<Workbench>().take_messages() {
                self.post_at(ctx, message);
            }
            zen_workbench::read_only::offer_writable(session);
        }
        self.share_editor_font_size(session);
        self.follow_markdown_layout(session);
        if session.state::<Workbench>().and_then(Workbench::project).is_none() {
            self.notifications.flush(session);
            return;
        }
        if let Err(error) = self.find_in_files.finish_replace(ctx, registry, session) {
            self.report_at(ctx, error);
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.follow_terminals(session);
        self.follow_terminal_links(ctx, registry, session);
        self.as_git(|view| view.vcs_frame(ctx, registry, session, now));
        self.language_servers(ctx, registry, session);
        self.poll_language_servers(ctx, registry, session, now, focused);
        self.follow_usages(ctx, registry, session);
        self.follow_hierarchy(ctx, registry, session);
        self.follow_everywhere(session);
        self.follow_todo(ctx, registry, session, now);
        self.follow_results(ctx, registry, session);
        self.follow_plugin_projects(registry, session);
        self.follow_build_tools(registry, session, now);
        self.follow_build_sync(registry, session, now);
        self.follow_ant(registry, session, now);
        self.follow_npm(ctx, session, now);
        self.follow_plugin_tool_windows(ctx, registry, session, now);
        self.follow_plugin_diffs(registry, session);
        zen_workbench::plugin_document::open_ready(session);
        self.follow_endpoints(registry, session, now);
        self.follow_dependencies(registry, session, now);
        self.follow_runs(registry, session);
        self.follow_trust(registry, session);
        zen_workbench::java::bytecode::poll(session);
        zen_workbench::java::console::poll(session);
        zen_workbench::project_index::poll(session, std::time::Instant::now());
        self.follow_index_reveal(ctx, registry, session);
        if session.state::<zen_workbench::project_index::Indexes>().is_some_and(|indexes| indexes.indexing()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.follow_file_watchers(ctx, session);
        self.follow_services(registry, session);
        self.follow_coverage(registry, session);
        self.follow_debug(ctx, registry, session, now);
        zen_workbench::bookmarks::sync(session);
        self.refresh_tree_bookmarks(registry, session);
        self.notifications.flush(session);
    }

    pub(super) fn take_drawn(&mut self) -> bool {
        std::mem::take(&mut self.drawn)
    }

    fn open_everywhere_pick(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        picked: search_everywhere::Pick,
    ) {
        match picked {
            search_everywhere::Pick::Place(path, offset) => {
                let params = serde_json::json!({ "path": path, "offset": offset });
                self.dispatch(ui, registry, session, "navigate.place.goTo", params);
            }
            search_everywhere::Pick::File(path, location) => {
                let _ = registry.dispatch(session, "file.document.open", serde_json::json!({ "path": path }));
                if let Some((line, column)) = location {
                    let params = serde_json::json!({ "line": line, "column": column });
                    self.dispatch(ui, registry, session, "editor.caret.goTo", params);
                }
            }
            search_everywhere::Pick::Directory(path) => {
                self.run_action(ui, registry, session, "view.project.select", serde_json::json!({ "paths": [path] }));
            }
            search_everywhere::Pick::Action(binding) => {
                self.run_action(ui, registry, session, &binding.command, binding.params);
            }
            search_everywhere::Pick::RightSplit(inner) => {
                let path = match inner.as_ref() {
                    search_everywhere::Pick::File(path, _) | search_everywhere::Pick::Place(path, _) => path.clone(),
                    _ => return self.open_everywhere_pick(ui, registry, session, *inner),
                };
                self.dispatch(ui, registry, session, "editor.split.openRight", serde_json::json!({ "path": path }));
                self.open_everywhere_pick(ui, registry, session, *inner);
            }
        }
    }

    fn show_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        self.main_screen = ui.input(|input| input.viewport().inner_rect);
        self.lock_scroll_axis(ui);
        self.free_scroll.clear();
        self.scroll_bars(ui);
        self.absorb_tool_flags();
        self.new_popup_keys(ui, registry, session);
        self.shortcuts(ui, registry, session);
        self.mouse_navigation(ui, registry, session);
        if session.state::<Workbench>().is_some() {
            for message in session.state_mut::<Workbench>().take_messages() {
                self.post(ui, message);
            }
        }
        for message in self.plugins.take_messages() {
            self.notify(NewNotification::message(Group::Plugins, Kind::Info, message));
        }
        let now = ui.input(|input| input.time);
        let focused = ui.input(|input| input.viewport().focused.unwrap_or(true));
        self.step(ui.ctx(), registry, session, now, focused);
        self.drawn = true;
        let project = session.state::<Workbench>().and_then(|workbench| workbench.project().map(Path::to_owned));
        let Some(project) = project else { return };
        let overlays = self.overlays(ui.ctx());
        if overlays.is_some() && overlays != self.overlay_shown {
            self.editor_view.cancel_focus();
            if self.overlay_shown.is_none()
                && let Some(focused) = ui.ctx().memory(egui::Memory::focused)
            {
                let editor = self.editor_view.has_focus_in(ui.ctx());
                #[cfg(not(target_arch = "wasm32"))]
                let terminal = self.terminal_drawn.iter().any(|(_, widget, _)| *widget == focused);
                #[cfg(target_arch = "wasm32")]
                let terminal = false;
                if editor || terminal {
                    ui.ctx().memory_mut(|memory| memory.surrender_focus(focused));
                }
            }
            if !self.typing_released {
                self.held_typing = filter_popups::hold_typing(ui);
            }
        }
        let palette = self.palette;
        let mut everywhere = std::mem::take(&mut self.search_everywhere);
        let picked = everywhere.show(ui, registry, session, &palette, &|name| self.highlighter_for(name));
        self.search_everywhere = everywhere;
        if let Some(picked) = picked {
            self.open_everywhere_pick(ui, registry, session, picked);
        }
        let folder = self.tree_folder(session);
        let mut find_in_files = std::mem::take(&mut self.find_in_files);
        find_in_files.set_too_many(self.find_window.asking.is_some());
        let request = find_in_files.show(ui, registry, session, &palette, folder, &|name| self.highlighter_for(name));
        self.find_in_files = find_in_files;
        let opened = match request {
            Some(find_in_files::Request::OpenInFindWindow) => {
                self.open_in_find_window(registry, session, false, None, None)
            }
            Some(find_in_files::Request::Failed(error)) => Err(agent::ActionError::Dispatch(error)),
            None => Ok(serde_json::Value::Null),
        };
        if let Err(agent::ActionError::Dispatch(error)) = opened {
            self.report(ui, error);
        }
        self.too_many_results(ui, registry, session);
        self.usages_settings_dialog(ui, registry, session);
        self.recent_usages_popup(ui, registry, session);
        self.paste_history.show(ui, registry, session, &palette);
        self.run_anything_popup(ui, registry, session);
        for error in self.scratch.show(ui, registry, session, &palette) {
            self.report(ui, error);
        }
        if let Some(done) = self.bookmarks.show(ui, &palette) {
            for (command, params) in done.commands {
                self.dispatch(ui, registry, session, command, params);
            }
            self.editor_view.focus();
        }
        let was_open = self.breakpoint_dialogs.is_open();
        if let Some(breakpoints_dialog::Outcome::Navigate { path, line }) =
            self.breakpoint_dialogs.show(ui, registry, session, &palette)
        {
            let offset = debug_window::offset_of(session, &path, line, 0);
            self.dispatch(
                ui,
                registry,
                session,
                "navigate.place.goTo",
                serde_json::json!({ "path": path, "offset": offset }),
            );
        }
        if was_open && !self.breakpoint_dialogs.is_open() {
            self.editor_view.forget_breakpoints();
            self.editor_view.focus();
        }
        self.value_popups(ui, registry, session);
        self.smart_step_popup(ui, registry, session);
        self.stream_traces.poll(registry, session);
        self.stream_traces.show(ui, &palette);
        if let Some(run) = self.quick_switch.show(ui, &palette) {
            self.quick_switch_run(ui, registry, session, run);
        }
        match self.recent_files.show(ui, registry, session, &palette) {
            Some(recent_files::Action::Tool(action)) => {
                let shown = tool_stripes::KINDS
                    .iter()
                    .find(|kind| kind.action == action)
                    .is_some_and(|kind| self.tool_visible(kind.id));
                if !shown {
                    self.run_action(ui, registry, session, action, serde_json::json!({}));
                }
            }
            Some(recent_files::Action::RecentLocations) => self.recent_locations.open_with(false),
            Some(recent_files::Action::Failed(error)) => self.report(ui, error),
            None => {}
        }
        let mut locations = std::mem::take(&mut self.recent_locations);
        let place = locations.show(ui, registry, session, &palette, &|path| self.highlighter_at(path));
        self.recent_locations = locations;
        if let Some(recent_locations::Pick { path, offset }) = place {
            let params = serde_json::json!({ "path": path, "offset": offset });
            self.dispatch(ui, registry, session, "navigate.place.goTo", params);
            self.editor_view.focus();
        }
        if let Some(file_structure::Pick { path, offset }) = self.file_structure.show(ui, registry, session, &palette) {
            let params = serde_json::json!({ "path": path, "offset": offset });
            self.dispatch(ui, registry, session, "navigate.place.goTo", params);
            self.editor_view.focus();
        }
        if let Some((target, path)) = self.select_in.show(ui, &palette) {
            let params = serde_json::json!({ "target": target.id(), "path": path });
            self.run_action(ui, registry, session, "view.selectIn.target", params);
        }

        self.navigation_bar_input(ui, registry, session);

        let p = self.palette;
        let frame = egui::Frame::new().fill(p.frame);
        let chrome = !self.appearance.hides_chrome();
        if chrome {
            egui::Panel::top("ide-toolbar").exact_size(metrics::TOOLBAR).show_separator_line(false).frame(frame).show(
                ui,
                |ui| {
                    self.toolbar(ui, registry, session, &project);
                },
            );
        } else if !self.appearance.full_screen() {
            egui::Panel::top("ide-title")
                .exact_size(metrics::TOOLBAR)
                .show_separator_line(false)
                .frame(frame)
                .show(ui, appearance::title_bar);
        }
        if !chrome {
            self.hidden_toolbar(ui, registry, session);
        }
        if chrome && self.overview.tabs.len() > 1 && self.overview.tabs.contains(&project) {
            egui::Panel::top("ide-project-tabs")
                .exact_size(metrics::PROJECT_TABS)
                .show_separator_line(false)
                .frame(frame)
                .show(ui, |ui| {
                    self.project_tabs(ui);
                });
        }
        if chrome && self.navigation_bar_top() {
            egui::Panel::top("ide-navigation-bar")
                .exact_size(metrics::NAVIGATION_BAR)
                .show_separator_line(false)
                .frame(frame)
                .show(ui, |ui| {
                    let rect = ui.max_rect();
                    let bar = Rect::from_min_max(
                        pos2(rect.left() + 12.0, rect.top()),
                        pos2(rect.right() - 12.0, rect.bottom()),
                    );
                    self.navigation_bar_in(ui, bar, registry, session, false);
                });
        }
        if chrome {
            self.follow_caffeine_look(session);
            egui::Panel::bottom("ide-status")
                .exact_size(metrics::STATUS_BAR)
                .show_separator_line(false)
                .frame(frame)
                .show(ui, |ui| {
                    self.status_bar(ui, registry, session);
                });
            egui::Panel::left("ide-stripe-left")
                .exact_size(metrics::STRIPE)
                .resizable(false)
                .show_separator_line(false)
                .frame(frame)
                .show(ui, |ui| {
                    self.left_stripe(ui);
                });
            egui::Panel::right("ide-stripe-right")
                .exact_size(metrics::STRIPE)
                .resizable(false)
                .show_separator_line(false)
                .frame(frame)
                .show(ui, |ui| {
                    if let Some((action, params)) = self.right_stripe(ui, session) {
                        self.run_action(ui, registry, session, action, params);
                    }
                });
            self.stripe_drag(ui);
        }
        let island = egui::Frame::new()
            .fill(p.island)
            .corner_radius(CornerRadius::same(metrics::ISLAND_RADIUS))
            .outer_margin(egui::Margin::same(metrics::GAP as i8));
        self.tool_window_panels(ui, registry, session, &project);
        self.follow_terminal_keyboard(ui.ctx());
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.frame)).show(ui, |ui| {
            island.show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                self.editor_area(ui, registry, session);
            });
        });
        self.navigation_bar_overlay(ui, registry, session);
        self.settings_dialog(ui, registry, session);
        self.language_server_log(ui, registry, session);
        self.run_configurations_dialog(ui, registry, session);
        self.as_git(|view| view.vcs_dialogs(ui, registry, session));
        self.as_git(|view| view.rollback_dialog(ui, registry, session));
        self.as_git(|view| view.changes_dialog(ui, registry, session));
        self.as_git(|view| view.patch_dialog(ui, registry, session));
        self.local_history_dialog(ui, registry, session);
        self.refactoring_dialog(ui, registry, session);
        self.macro_dialogs(ui, registry, session);
        self.layout_dialog(ui);
        self.launcher_dialog(ui, registry, session);
        self.artifacts_popup(ui, registry, session);
        self.connection_dialog(ui, registry, session);
        self.import_dialog(ui, registry, session);
        self.export_dialog(ui, registry, session);
        self.backup_dialog(ui, registry, session);
        self.csv_import_dialog(ui, registry, session);
        self.transfer_dialog(ui, registry, session);
        self.transaction_dialogs(ui, registry, session);
        self.ssh_dialog(ui, registry, session);
        self.associate_dialog(ui, registry, session);
        self.new_project_dialog(ui, registry, session);
        self.clone_dialog(ui, registry, session);
        self.create_test_dialog(ui, registry, session);
        self.inspect_dialog(ui, registry, session);
        self.test_history_popup(ui, registry, session);
        self.internal_errors_dialog(ui, registry, session);
        self.encoding_popups(ui, registry, session);
        self.starters_dialog(ui, registry, session);
        self.new_popup(ui, registry, session);
        self.new_from_template_dialog(ui, registry, session);
        self.save_as_template_dialog(ui, registry, session);
        self.new_editor_config_dialog(ui, registry, session);
        self.editor_config_files_dialog(ui, registry, session);
        self.ide_zoom_dialog(ui);
        self.manage_settings_dialogs(ui, registry, session);
        self.external_tool_prompt(ui, registry, session);
        self.rerun_confirmation(ui, registry, session);
        self.person_question(ui, registry, session);
        self.activity_monitor(ui, registry, session);
        self.diagnostics_dialogs(ui, registry, session);
        self.tip_dialog(ui);
        self.productivity_dialog(ui);
        self.exit_dialog(ui);
        self.certificate_dialog(ui);
        self.restart_dialog(ui);
        self.invalidate_dialog(ui);
        self.update_notification(ui);
        self.follow_notifications(ui, registry, session);
        self.tasks_panel(ui, registry, session);
        self.notification_balloons(ui, registry, session);
    }

    fn update_notification(&mut self, ui: &mut Ui) {
        let Some(offer) = self.overview.update.clone() else { return };
        let title = t!("ide-update-title");
        let logged =
            NewNotification::message(Group::Updates, Kind::Info, format!("{} {}", self.product.name, offer.version))
                .titled(title)
                .action(Action::new("update", t!("ide-update-install"), "view.update.install", serde_json::json!({})))
                .action(Action::new(
                    "later",
                    t!("ide-update-later"),
                    "view.update.later",
                    serde_json::json!({ "version": offer.version }),
                ))
                .silent();
        self.notifications.queue_once(format!("update:{}", offer.version), logged);
        let p = self.palette;
        let screen = ui.ctx().content_rect();
        let width = 380.0;
        let id = ui.scope_id().with("update-notification");
        egui::Area::new(id)
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(screen.right() - width - 56.0, screen.bottom() - metrics::STATUS_BAR - 12.0))
            .pivot(Align2::LEFT_BOTTOM)
            .show(ui.ctx(), |ui| {
                p.popup_frame().corner_radius(10).inner_margin(14).show(ui, |ui| {
                    ui.set_width(width - 28.0);
                    ui.horizontal(|ui| {
                        let (mark, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                        crate::icons::product_mark(ui, mark, self.product.id, self.product.name, false);
                        ui.label(egui::RichText::new(t!("ide-update-title")).font(theme::semibold(13.5)).color(p.text));
                    });
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!("{} {}", self.product.name, offer.version))
                            .font(theme::regular(13.0))
                            .color(p.text_secondary),
                    );
                    if self.update_details {
                        ui.add_space(6.0);
                        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                            for note in &offer.notes {
                                ui.label(egui::RichText::new(&note.version).font(theme::semibold(12.5)).color(p.text));
                                for line in plain_notes(&note.notes) {
                                    ui.label(
                                        egui::RichText::new(line).font(theme::regular(12.5)).color(p.text_secondary),
                                    );
                                }
                                ui.add_space(4.0);
                            }
                        });
                    }
                    if let Some(error) = &offer.error {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(error).font(theme::regular(12.5)).color(p.excluded));
                    }
                    ui.add_space(8.0);
                    if let Some(progress) = offer.progress {
                        let text = if progress > 0.0 {
                            t!("ide-update-progress", percent = format!("{:.0}", progress * 100.0))
                        } else {
                            t!("ide-update-installing")
                        };
                        ui.add(egui::ProgressBar::new(progress).desired_height(4.0).fill(p.accent));
                        ui.label(egui::RichText::new(text).font(theme::regular(12.5)).color(p.text_secondary));
                        return;
                    }
                    ui.horizontal(|ui| {
                        let link = |ui: &mut Ui, text: String, color: Color32| {
                            ui.add(egui::Label::new(egui::RichText::new(text).color(color)).sense(Sense::click()))
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                        };
                        if link(ui, t!("ide-update-install"), p.link).clicked() {
                            self.requests.push(Request::Update);
                        }
                        ui.add_space(10.0);
                        if link(ui, t!("ide-update-notes"), p.link).clicked() {
                            self.update_details = !self.update_details;
                        }
                        ui.add_space(10.0);
                        if link(ui, t!("ide-update-later"), p.text_secondary).clicked() {
                            self.requests.push(Request::DismissUpdate(offer.version.clone()));
                        }
                    });
                });
            });
    }

    fn command_program(&self) -> Option<PathBuf> {
        let found = launcher_dialog::launcher_on_path(self.product.id);
        launcher_dialog::program_for_commands(std::env::current_exe().ok(), self.product.id, found.as_deref())
    }

    fn settings_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        if self.settings.is_open() {
            self.refresh_style_pages(session);
        }
        let sections = self.settings_sections();
        let form = self
            .settings
            .shown_module_page()
            .and_then(|id| self.product.settings_pages.iter().find(|page| page.id == id))
            .and_then(|page| registry.dispatch(session, page.form, serde_json::json!({})).ok())
            .and_then(|value| serde_json::from_value::<zen_modules::form::Form>(value).ok());
        let declared: Vec<zen_project::inspect::DeclaredSetting> = session
            .state::<zen_workbench::inspections::Inspections>()
            .map(|inspections| inspections.declared())
            .unwrap_or_default();
        let context = settings::SettingsContext {
            settings: &self.overview.settings,
            mcp_url: self.overview.automation.as_deref(),
            product_id: self.product.id,
            executable: self.settings.is_open().then(|| self.command_program()).flatten(),
            form,
            sections: &sections,
            language_servers: self.settings.shows_language_servers().then(|| self.lsp.servers()),
            declared: &declared,
            folding: &self.product.folding_options,
        };
        let mut dispatch = |id: &str, params| {
            registry.dispatch(session, id, params).map_err(|error| match error {
                zen_core::DispatchError::Failed { error, .. } => error.to_string(),
                other => other.to_string(),
            })
        };
        if let Some(settings) = self.settings.show(ui, &self.palette, &context, Some(&mut dispatch)) {
            self.requests.push(Request::Settings(Box::new(settings)));
        }
        if std::mem::take(&mut self.settings.restart_request) {
            self.requests.push(Request::Restart);
        }
        if let Some(path) = self.settings.open_request.take() {
            self.dispatch(ui, registry, session, "file.document.open", serde_json::json!({ "path": path }));
        }
        if let Some((command, params)) = self.settings.form_request.take() {
            let answer = registry.dispatch(session, &command, params).map_err(|error| match error {
                zen_core::DispatchError::Failed { error, .. } => error.to_string(),
                other => other.to_string(),
            });
            self.settings.form_answer(answer);
        }
        if let Some((command, params)) = self.settings.server_request.take() {
            self.lsp.invalidate();
            self.dispatch(ui, registry, session, command, params);
        }
    }

    fn settings_sections(&self) -> Vec<settings::SectionDef> {
        let styles: Vec<zen_modules::CodeStylePage> =
            self.product.code_styles.iter().chain(&self.settings.style_pages).copied().collect();
        settings::page::sections(&self.product.settings_pages, &styles)
    }

    fn refresh_style_pages(&mut self, session: &Session) {
        self.settings.style_pages = settings::code_style_page::plugin_pages(session, &self.product.code_styles);
    }

    fn lock_scroll_axis(&mut self, ui: &Ui) {
        let now = ui.input(|input| input.time);
        let delta = ui.input(|input| input.smooth_scroll_delta);
        if delta != Vec2::ZERO {
            self.last_scroll = now;
        }
        let pointer = ui.input(|input| input.pointer.hover_pos());
        let free = pointer.is_some_and(|pointer| self.free_scroll.iter().any(|area| area.contains(pointer)));
        let kept = locked_scroll(&mut self.scroll_lock, delta, now, free);
        if kept != delta {
            ui.ctx().input_mut(|input| input.smooth_scroll_delta = kept);
        }
    }

    fn scroll_bars(&mut self, ui: &mut Ui) {
        const STAY: f64 = 0.8;
        const FADE: f64 = 0.3;
        let since = ui.input(|input| input.time) - self.last_scroll;
        let visible = if since <= STAY { 1.0 } else { (1.0 - (since - STAY) / FADE).clamp(0.0, 1.0) };
        if visible > 0.0 {
            ui.ctx().request_repaint();
        }
        ui.style_mut().spacing.scroll.active_handle_opacity = 0.6 * visible as f32;
    }

    fn shortcuts(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        if self.settings.is_recording() || self.search_everywhere.is_recording() {
            return;
        }
        if matches!(
            self.transaction_dialogs.open,
            Some(
                transaction_dialog::Dialog::Execute(_)
                    | transaction_dialog::Dialog::Parameters(_)
                    | transaction_dialog::Dialog::Several(_),
            )
        ) {
            return;
        }
        #[cfg(target_os = "macos")]
        self.main_menu(ui, registry, session);
        let doubles = !session.settings().get_or(crate::keymap::DISABLE_DOUBLE_MODIFIERS, false);
        if self.double_shift(ui) && doubles {
            self.search_everywhere.double_shift();
        }
        if self.double_control(ui) && doubles {
            let stroke = zen_core::keymap::Keystroke::double('⌃');
            if let Some(binding) =
                stroke.and_then(|stroke| crate::keymap::current().lookup(&stroke, zen_core::keymap::Context::Global))
            {
                self.run_action(ui, registry, session, &binding.action, binding.params);
            }
        }
        let keymap = crate::keymap::current();
        let mut matched = Vec::new();
        let diff_shown = self.diff_in_front(session);
        let results_focused = self.find_window.results_focused(ui.ctx());
        let expanded = zen_workbench::workspaces::expanded(session);
        let database = expanded.as_deref() == Some(zen_workbench::database::WORKSPACE);
        let focused_widget = ui.ctx().memory(egui::Memory::focused);
        let context = if self.terminal_has_keyboard(ui.ctx()) {
            zen_core::keymap::Context::Terminal
        } else if database && self.workspace_has_keyboard(session) {
            zen_core::keymap::Context::Database
        } else if self.markdown_editor_has_keyboard(ui.ctx(), session) {
            zen_core::keymap::Context::Markdown
        } else {
            zen_core::keymap::Context::Global
        };
        let shift_escape = |event: &egui::Event| matches!(event, egui::Event::Key { key: Key::Escape, pressed: true, modifiers, .. } if modifiers.shift);
        let keep_shift_escape =
            ui.input(|input| input.events.iter().any(shift_escape)) && self.keeps_shift_escape(ui.ctx());
        let diff_typing = diff_shown
            && self.diff.as_ref().is_some_and(diff_view::DiffTab::typed)
            && ui
                .ctx()
                .memory(egui::Memory::focused)
                .is_some_and(|focused| egui::text_edit::TextEditState::load(ui.ctx(), focused).is_some());
        let bookmarks_focused = self.bookmarks_window.has_focus(ui.ctx());
        let overlay = self.overlay_open(ui.ctx());
        let jump = self.jump_source(ui.ctx(), session).is_some() && !diff_typing;
        let mut typed_by_shortcut = false;
        ui.input_mut(|input| {
            input.events.retain(|event| {
                if typed_by_shortcut && matches!(event, egui::Event::Text(_)) {
                    typed_by_shortcut = false;
                    return false;
                }
                if let egui::Event::Key { key, physical_key, pressed: true, modifiers, .. } = event
                    && !(bookmarks_focused && bookmarks_window::is_step(*key, *modifiers))
                    && !(diff_shown && *key == Key::F7 && !modifiers.command && !modifiers.alt && !modifiers.ctrl)
                    && !(keep_shift_escape && shift_escape(event))
                    && !self.editor_view.takes_key(*key, *modifiers)
                    && !self.workspace_takes_key(focused_widget, expanded.as_deref(), *key, *modifiers)
                    && !self.search_everywhere.takes_key(*key, *modifiers)
                    && !(results_focused && find_results::own_key(*key, *modifiers))
                    && let Some(binding) = crate::keymap::lookup(&keymap, *key, *physical_key, *modifiers, context)
                        .and_then(|binding| search_everywhere::resolved(binding, &keymap, registry))
                        .filter(|binding| !(overlay && binding.action == "view.navigationBar.jump"))
                        .filter(|binding| !(diff_typing && binding.command.starts_with("editor.")))
                        .filter(|binding| binding.action != commit_window::JUMP_TO_SOURCE || jump)
                {
                    typed_by_shortcut =
                        modifiers.alt && !modifiers.command && !modifiers.ctrl && key.name().chars().count() == 1;
                    matched.push((binding, *modifiers));
                    return false;
                }
                true
            });
        });
        for (binding, modifiers) in matched {
            self.shortcut_modifiers = Some(modifiers);
            if binding.action == "view.toolWindow.hide" && self.workspace_has_keyboard(session) {
                self.run_action(ui, registry, session, "workspace.host.collapse", serde_json::json!({}));
            } else {
                self.run_action(ui, registry, session, &binding.command, binding.params);
            }
            self.shortcut_modifiers = None;
        }
    }

    fn markdown_editor_has_keyboard(&self, ctx: &egui::Context, session: &Session) -> bool {
        use zen_workbench::preview::{PreviewKind, PreviewMode, Previews};
        let Some(document) = session.state::<Workbench>().and_then(Workbench::active_document) else { return false };
        let path = document.path();
        PreviewKind::of(path) == Some(PreviewKind::Markdown)
            && self.editor_view.has_focus_in(ctx)
            && session
                .state::<Previews>()
                .is_none_or(|previews| previews.mode_of(path, PreviewKind::Markdown) != PreviewMode::PreviewOnly)
    }

    fn keeps_shift_escape(&self, ctx: &egui::Context) -> bool {
        if self.overlay_open(ctx) || egui::Popup::is_any_open(ctx) {
            return true;
        }
        let Some(focused) = ctx.memory(egui::Memory::focused) else { return false };
        if self.editor_view.has_focus_in(ctx) {
            return false;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.terminal_drawn.iter().any(|(_, widget, _)| *widget == focused) {
            return true;
        }
        egui::text_edit::TextEditState::load(ctx, focused).is_some()
    }

    fn overlay_open(&self, ctx: &egui::Context) -> bool {
        ctx.memory(|memory| memory.top_modal_layer()).is_some()
            || self.search_everywhere.open_tab().is_some()
            || self.recent_files.is_open()
            || self.recent_locations.is_open()
            || self.file_structure.is_open()
            || self.select_in.is_open()
            || self.paste_history.is_open()
            || self.run_anything.is_open()
            || self.scratch.is_open()
            || self.recent_usages.is_open()
            || self.bookmarks.is_open()
            || self.quick_switch.is_open()
            || self.find_in_files.state().is_some()
            || self.find_in_files.holds_keyboard()
            || self.find_window.asking.is_some()
            || self.projects.is_open()
            || self.navigation_bar.is_active()
            || self.layout_dialog_open()
    }

    fn overlays(&self, ctx: &egui::Context) -> Option<Overlays> {
        let modal = ctx.memory(|memory| memory.top_modal_layer());
        let open = [
            self.search_everywhere.open_tab().is_some(),
            self.recent_files.is_open(),
            self.recent_locations.is_open(),
            self.file_structure.is_open(),
            self.paste_history.is_open(),
            self.find_in_files.state().is_some(),
            self.projects.is_open(),
            self.settings.is_open(),
            self.exit_prompt.is_some(),
            self.diagnostics.debug_log.is_some(),
            self.diagnostics.special.is_some(),
            self.diagnostics.collect,
            self.diagnostics.licences.is_some(),
            self.restart_prompt.is_some(),
            self.invalidate_dialog.is_some(),
            self.rename.is_open(),
            self.refactoring.is_open(),
            self.goto_line.is_some(),
            self.run_configurations.is_open(),
            self.local_history.is_history_open(),
            self.local_history.is_label_open(),
            self.new_entry.is_some(),
            self.delete_entry.is_some(),
            self.rollback.is_some(),
            self.changes_dialog.is_some(),
        ];
        (modal.is_some() || open.contains(&true)).then_some((modal, open))
    }

    #[cfg(target_os = "macos")]
    fn main_menu(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let ctx = ui.ctx().clone();
        if !ctx.input(|input| input.viewport().focused.unwrap_or(false)) {
            return;
        }
        customization::note_commands(registry);
        let keymap = crate::keymap::current();
        let bindings = keymap.bindings();
        let layouts = self.layout_items();
        let new_items: Vec<(String, String)> =
            self.cached_menu_items(session).into_iter().map(|item| (item.key(), item.title)).collect();
        let available = |action: &str| {
            action.starts_with("new:")
                || action.starts_with(main_menu::RECENT)
                || main_menu::layout_item(action).is_some_and(|(command, name)| {
                    command != "view.layout.delete" || layouts.iter().any(|(_, item, on)| !on && item == name)
                })
                || bindings.iter().find(|binding| binding.action == action).is_some_and(|binding| {
                    let command = match binding.command.as_str() {
                        smart_step::SMART_STEP_INTO => "debug.execution.stepInTargets",
                        stream_trace::STREAM_TRACE => "debug.stream.trace",
                        command => command,
                    };
                    (command.starts_with("view.") || registry.is_available(session, command))
                        && (binding.command != "view.project.closeOthers" || self.overview.open.len() > 1)
                        && self.separator_choosable(session, &binding.command, &binding.params)
                })
                || registry.info(action).is_some() && registry.is_available(session, action)
        };
        let scroll = session.state::<Workbench>().is_some_and(|workbench| workbench.find().scroll_to_results);
        let checked =
            |action: &str| (action == "editor.find.scrollToResults" && scroll) || self.action_checked(action, session);
        let types = session.state::<zen_modules::search::TypeNoun>() == Some(&zen_modules::search::TypeNoun::Type);
        let title = |action: &str| match action {
            "view.gotoClass.open" if types => Some(t!("menu-navigate-type")),
            _ => self
                .macro_title(action, session)
                .or_else(|| self.appearance_title(action))
                .or_else(|| read_only::menu_title(session, action)),
        };
        let recent = main_menu::recent_items(&self.overview.recent);
        let tool_windows = self.plugin_tools.menu_items();
        let lists =
            main_menu::Lists { layouts: &layouts, new_items: &new_items, recent: &recent, tool_windows: &tool_windows };
        main_menu::update(&ctx, self.product.name, &available, &title, &checked, lists);
        for action in main_menu::take_chosen(ctx.viewport_id()) {
            if let Some(path) = action.strip_prefix(main_menu::RECENT) {
                self.requests.push(Request::Open(PathBuf::from(path)));
                continue;
            }
            if action.starts_with("new:") {
                self.choose_new_by_key(ui, registry, session, &action);
                continue;
            }
            if let Some((command, name)) = main_menu::layout_item(&action) {
                let params = serde_json::json!({ "name": name, "ask": command == "view.layout.delete" });
                self.run_action(ui, registry, session, command, params);
                continue;
            }
            let binding = bindings.iter().find(|binding| binding.action == action);
            let Some(binding) = binding.filter(|_| !customization::from_agent(&action)) else {
                self.run_customized(ui, registry, session, &action);
                continue;
            };
            let key = binding.keystrokes.first().and_then(|stroke| {
                Key::ALL.iter().find(|key| format!("{key:?}") == stroke.key).map(|key| (*key, stroke))
            });
            match key {
                Some((key, stroke)) => {
                    let modifiers = egui::Modifiers {
                        alt: stroke.alt,
                        ctrl: stroke.control,
                        shift: stroke.shift,
                        mac_cmd: stroke.command,
                        command: stroke.command,
                    };
                    ui.input_mut(|input| {
                        input.events.push(egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers,
                        })
                    });
                }
                None => self.run_action(ui, registry, session, &binding.command, binding.params.clone()),
            }
        }
    }

    fn mouse_navigation(&mut self, ui: &Ui, registry: &Registry, session: &mut Session) {
        let (back, forward) = ui.input(|input| {
            (
                input.pointer.button_pressed(egui::PointerButton::Extra1),
                input.pointer.button_pressed(egui::PointerButton::Extra2),
            )
        });
        if back {
            self.dispatch(ui, registry, session, "navigate.history.back", serde_json::Value::Null);
        }
        if forward {
            self.dispatch(ui, registry, session, "navigate.history.forward", serde_json::Value::Null);
        }
    }

    fn double_control(&mut self, ui: &Ui) -> bool {
        let modifier = self.double_control.modifier();
        let (now, modifiers, typed) =
            ui.input(|input| (input.time, input.modifiers, search_everywhere::typed_besides(&input.events, modifier)));
        self.double_control.update(now, modifiers, typed)
    }

    fn run_anything_popup(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        if !self.run_anything.is_open() {
            return;
        }
        let palette = self.palette;
        let projects: Vec<PathBuf> = self.overview.recent.iter().map(|project| project.path.clone()).collect();
        match self.run_anything.show(ui, registry, session, &palette, &projects) {
            Some(run_anything::Outcome::Run(params)) => match registry.dispatch(session, "run.anything.run", params) {
                Ok(started) if started["way"] == "terminal" => self.show_tool("terminal"),
                Ok(started) if started["sessionId"].is_u64() => self.show_debug_window(),
                Ok(_) => self.show_tool("run"),
                Err(error) => self.report(ui, error),
            },
            Some(run_anything::Outcome::Start(command, params)) => {
                self.run_action(ui, registry, session, command, params);
                self.run_widget.invalidate();
            }
            Some(run_anything::Outcome::Open(path)) if self.overview.open.contains(&path) => {
                self.requests.push(Request::Focus(path));
            }
            Some(run_anything::Outcome::Open(path)) => self.requests.push(Request::Open(path)),
            Some(run_anything::Outcome::EditConfigurations) => self.run_configurations.open(),
            None => {}
        }
    }

    fn double_shift(&mut self, ui: &Ui) -> bool {
        let (now, modifiers, typed) =
            ui.input(|input| (input.time, input.modifiers, search_everywhere::typed(&input.events)));
        self.double_shift.update(now, modifiers, typed)
    }

    fn run_action(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        action: &str,
        params: serde_json::Value,
    ) {
        let group = Group::of_command(action);
        let run = (action == "db.script.run").then(|| params.clone());
        let code_run = (action == "db.injection.run").then(|| params.clone());
        match self.with_group(group, |view| view.perform_action(ui.ctx(), registry, session, action, params, false)) {
            Ok(_) | Err(agent::ActionError::Refused(_)) => {}
            Err(agent::ActionError::Dispatch(error)) if sql_script_bar::not_bound(&error) && run.is_some() => {
                self.script_bar.ask_binding(run.unwrap_or_default());
            }
            Err(agent::ActionError::Dispatch(error)) if sql_script_bar::not_bound(&error) && code_run.is_some() => {
                let path = match &error {
                    zen_core::DispatchError::Failed { error, .. } => {
                        error.data.as_ref().and_then(|data| data.get("path")).and_then(serde_json::Value::as_str)
                    }
                    _ => None,
                };
                if let Some(path) = path.map(PathBuf::from) {
                    let mut run = code_run.unwrap_or_default();
                    run["path"] = serde_json::json!(path);
                    self.editor_view.ask_sql_connection(registry, session, &path, Some(run));
                }
            }
            Err(agent::ActionError::Dispatch(error)) => self.report(ui, error),
        }
    }

    fn run_customized(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, action: &str) {
        let from_agent = customization::from_agent(action);
        self.run_customized_as(ui, registry, session, action, from_agent);
    }

    fn run_customized_as(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        action: &str,
        from_agent: bool,
    ) {
        if from_agent {
            let refused = zen_core::macros::as_agent(session, |session| {
                if registry.info(action).is_some() {
                    registry
                        .dispatch(session, action, serde_json::json!({}))
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                } else {
                    self.run_for_agent(ui.ctx(), registry, session, (action, action, serde_json::json!({}))).map(|_| ())
                }
            });
            if let Err(error) = refused {
                self.warn(error);
            }
        } else if registry.info(action).is_some() {
            self.dispatch(ui, registry, session, action, serde_json::Value::Null);
        } else {
            self.run_action(ui, registry, session, action, serde_json::json!({}));
        }
    }

    fn told(&mut self, result: Result<serde_json::Value, String>, agent: bool) -> Result<serde_json::Value, String> {
        if let (Err(error), false) = (&result, agent) {
            self.warn(error.clone());
        }
        result
    }

    fn perform_action(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        action: &str,
        params: serde_json::Value,
        agent: bool,
    ) -> Result<serde_json::Value, agent::ActionError> {
        use agent::ActionError::{Dispatch, Refused};
        let text_file = self.text_file_in_front(session);
        let done = Ok(serde_json::Value::Null);
        if !agent {
            self.help.record(action);
        }
        match action {
            markdown_view::SELECT_ALL | markdown_view::COPY | markdown_view::COPY_MARKDOWN => {
                self.markdown_preview_action(ctx, session, action).map_err(Refused)
            }
            transaction_dialog::PENDING => {
                session.state_mut::<transaction_dialog::TransactionDialogRequest>().0 =
                    Some(transaction_dialog::Dialog::Pending);
                Ok(serde_json::json!({ "dialog": "pendingTransactions" }))
            }
            export_dialog::OPEN => {
                if registry.info("db.export.start").is_none() {
                    return Err(Refused(t!("ide-dbc-no-module")));
                }
                let dialog = export_dialog::ExportDialog::open(registry, session, &params).map_err(Refused)?;
                self.export_dialog = Some(dialog);
                Ok(serde_json::json!({ "dialog": "exportData" }))
            }
            backup_dialog::OPEN_BACKUP | backup_dialog::OPEN_RESTORE => {
                if registry.info("db.backup.start").is_none() {
                    return Err(Refused(t!("ide-dbc-no-module")));
                }
                let mode = if action == backup_dialog::OPEN_BACKUP {
                    backup_dialog::Mode::Backup
                } else {
                    backup_dialog::Mode::Restore
                };
                let dialog = backup_dialog::BackupDialog::open(registry, session, mode, &params).map_err(Refused)?;
                self.backup_dialog = Some(dialog);
                Ok(serde_json::json!({ "dialog": "backupDatabase" }))
            }
            csv_import_dialog::OPEN => {
                if registry.info("db.csvImport.start").is_none() {
                    return Err(Refused(t!("ide-dbc-no-module")));
                }
                let dialog = csv_import_dialog::CsvImportDialog::open(registry, session, &params).map_err(Refused)?;
                self.csv_import_dialog = Some(dialog);
                Ok(serde_json::json!({ "dialog": "importData" }))
            }
            transfer_dialog::OPEN => {
                if registry.info("db.transfer.start").is_none() {
                    return Err(Refused(t!("ide-dbc-no-module")));
                }
                let dialog = transfer_dialog::TransferDialog::open(registry, session, &params).map_err(Refused)?;
                self.transfer_dialog = Some(dialog);
                Ok(serde_json::json!({ "dialog": "transferData" }))
            }
            _ if action
                .strip_prefix("view.toolWindow.")
                .is_some_and(|id| plugin_tool_window::kind(id).is_some_and(|kind| kind.action == action)) =>
            {
                if let Some(id) = action.strip_prefix("view.toolWindow.") {
                    self.toggle_tool(id);
                }
                done
            }
            _ if action.starts_with("view.database.") => {
                if self.workspace_view_action(session, action) {
                    done
                } else {
                    Err(Refused("the Database workspace is not in front".to_owned()))
                }
            }
            "view.newProject.open" | "view.newProject.empty" => {
                let generator = match action {
                    "view.newProject.empty" => Some(zen_workbench::new_project::EMPTY),
                    _ => params.get("generator").and_then(serde_json::Value::as_str),
                };
                let mut dispatch = |id: &str, params| dialog_dispatch(registry, session, agent, id, params);
                let mut dialog =
                    new_project_dialog::NewProjectDialog::open(&mut dispatch, generator).map_err(Refused)?;
                dialog.by_agent = agent;
                self.new_project = Some(dialog);
                Ok(serde_json::json!({ "dialog": "newProject" }))
            }
            "view.newProject.close" => {
                self.new_project = None;
                done
            }
            "view.project.fromVcs" => {
                let mut dispatch = |id: &str, params| dialog_dispatch(registry, session, agent, id, params);
                let mut dialog = clone_dialog::CloneDialog::open(&mut dispatch).map_err(Refused)?;
                dialog.by_agent = agent;
                self.clone = Some(dialog);
                Ok(serde_json::json!({ "dialog": "clone" }))
            }
            "view.project.fromVcs.close" => {
                self.clone = None;
                done
            }
            "view.project.fromSources" => {
                let path = params.get("path").and_then(serde_json::Value::as_str).map(PathBuf::from);
                if path.is_none() && agent {
                    return Err(Refused(
                        "the chooser is a system dialog; give `path`, or call project.fromSources.detect".to_owned(),
                    ));
                }
                let Some(path) = path.or_else(|| new_project_dialog::pick_sources(&t!("ide-module-sources-title")))
                else {
                    return done;
                };
                let params = serde_json::json!({ "path": path });
                let found = dialog_dispatch(registry, session, agent, "project.fromSources.detect", params);
                let found = self.told_to_person(ctx, agent, found)?;
                if !agent {
                    self.post_at(ctx, new_project_dialog::sources_notice(&found));
                }
                if let Some(root) = found["root"].as_str() {
                    self.requests.push(Request::Open(PathBuf::from(root)));
                }
                Ok(found)
            }
            "view.module.new" => {
                let generator = params.get("generator").and_then(serde_json::Value::as_str);
                let mut dispatch = |id: &str, params| dialog_dispatch(registry, session, agent, id, params);
                let mut dialog =
                    new_project_dialog::NewProjectDialog::open_module(&mut dispatch, generator).map_err(Refused)?;
                dialog.by_agent = agent;
                self.new_project = Some(dialog);
                Ok(serde_json::json!({ "dialog": "newProject" }))
            }
            "view.module.fromSources" => {
                let path = params.get("path").and_then(serde_json::Value::as_str).map(PathBuf::from);
                if path.is_none() && agent {
                    return Err(Refused(
                        "the chooser is a system dialog; give `path`, or call project.module.fromSources".to_owned(),
                    ));
                }
                let Some(path) = path.or_else(|| new_project_dialog::pick_sources(&t!("ide-module-sources-title")))
                else {
                    return done;
                };
                let params = serde_json::json!({ "path": path });
                let answer = dialog_dispatch(registry, session, agent, "project.module.fromSources", params);
                let answer = self.told_to_person(ctx, agent, answer)?;
                self.after_module(ctx, registry, session, agent, &answer);
                Ok(answer)
            }
            starters_dialog::OPEN | starters_dialog::SET | starters_dialog::APPLY | starters_dialog::CLOSE => {
                self.starters_action(registry, session, agent, action, &params).map_err(Refused)
            }
            "view.project.open" if agent => Err(Refused(
                "Open… asks for a folder in a system dialog; call the open_project tool with the folder instead"
                    .to_owned(),
            )),
            "view.project.open" => {
                self.open_folder_dialog();
                done
            }
            "view.project.close" => {
                let current = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
                self.requests.extend(current.map(Request::Close));
                done
            }
            "view.project.closeAll" => {
                self.requests.extend(self.overview.open.iter().cloned().map(Request::Close));
                done
            }
            "view.project.closeOthers" => {
                let current = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
                let others = self.overview.open.iter().filter(|path| Some(*path) != current.as_ref());
                self.requests.extend(others.cloned().map(Request::Close));
                done
            }
            "view.project.trust" if agent => {
                Err(Refused("Only the person trusts a project; agents cannot, and cannot ask for it here".to_owned()))
            }
            "view.project.trust" => {
                self.ask_trust(session);
                done
            }
            "view.toolWindow.project" => {
                self.toggle_tool("project");
                done
            }
            "view.toolWindow.structure" => {
                self.toggle_tool("structure");
                done
            }
            "view.toolWindow.bookmarks" => {
                self.toggle_tool("bookmarks");
                done
            }
            "view.bookmarks.next" | "view.bookmarks.previous" => {
                self.bookmarks_window.refresh(registry, session);
                let Some(bookmarks_window::BookmarksAction::Open { path, line, .. }) =
                    self.bookmarks_window.step(action == "view.bookmarks.next")
                else {
                    return Err(Refused("there are no bookmarks or breakpoints".to_owned()));
                };
                self.show_tool("bookmarks");
                registry
                    .dispatch(
                        session,
                        "navigate.bookmark.open",
                        serde_json::json!({ "path": path, "line": line.map(|line| line + 1) }),
                    )
                    .map_err(Dispatch)
            }
            "view.toolWindow.commit" => {
                self.toggle_tool("commit");
                done
            }
            "view.vcs.commit" => {
                self.open_commit();
                self.show_tool("commit");
                done
            }
            "view.toolWindow.hide"
            | "view.toolWindow.hideAll"
            | "view.toolWindow.move"
            | "view.toolWindow.resize"
            | "view.toolWindow.viewMode"
            | "view.layout.restore"
            | "view.layout.restoreDefault"
            | "view.layout.apply"
            | "view.layout.saveAsNew"
            | "view.layout.saveChanges"
            | "view.layout.rename"
            | "view.layout.delete" => self.layout_action(ctx, action, &params),
            "view.vcs.rollback" => {
                let paths = self.rollback_context(session);
                self.refresh_changes(registry, session);
                self.open_rollback(paths);
                done
            }
            "view.vcs.newChangelist" | "view.vcs.moveToChangelist" | "view.vcs.shelve" => {
                let given = self.given_paths(&params);
                self.refresh_changes(registry, session);
                if self.vcs.info.root.is_none() {
                    return Err(Refused("the project is not in a Git repository".to_owned()));
                }
                match action {
                    "view.vcs.newChangelist" => self.open_new_changelist(given),
                    "view.vcs.shelve" => {
                        if self.commit_window.checked_paths().is_empty() && given.is_empty() {
                            return Err(Refused("there are no changes to shelve".to_owned()));
                        }
                        self.open_shelve(registry, session, given);
                    }
                    _ => {
                        let paths = if given.is_empty() { self.commit_window.tracked_context() } else { given };
                        if paths.is_empty() {
                            return Err(Refused(
                                "Move to Another Changelist needs changed files: select one in the Commit tool \
                                 window or give `paths`"
                                    .to_owned(),
                            ));
                        }
                        self.open_move_to_changelist(paths);
                    }
                }
                done
            }
            "view.vcs.unshelve" => {
                self.refresh_shelf(registry, session, true);
                let given = params.get("id").and_then(serde_json::Value::as_u64);
                let paths = self.given_paths(&params);
                let Some((id, paths)) = given.map(|id| (id, paths)).or_else(|| self.shelf_context()) else {
                    return Err(Refused("there are no shelved changes to unshelve".to_owned()));
                };
                self.open_unshelve(registry, session, id, paths);
                if self.changes_dialog.is_none() {
                    return Err(Refused(format!("there are no shelved changes with the id {id}")));
                }
                done
            }
            "view.vcs.showShelf" => {
                self.show_commit_tab(commit_window::CommitTab::Shelf);
                done
            }
            "view.vcs.createPatch"
            | "view.vcs.copyPatch"
            | "view.vcs.applyPatch"
            | "view.vcs.applyPatchFromClipboard" => {
                self.refresh_changes(registry, session);
                if self.vcs.info.root.is_none() {
                    return Err(Refused("the project is not in a Git repository".to_owned()));
                }
                self.patch_action(ctx, registry, session, action, &params, agent)
            }
            "view.localHistory.show" => {
                let path = params
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .map(PathBuf::from)
                    .or_else(|| self.history_context(session));
                let Some(path) = path else {
                    return Err(Refused("Show History needs a project, a file or a `path`".to_owned()));
                };
                self.local_history.open(path);
                done
            }
            "view.localHistory.putLabel" => {
                self.local_history.open_label();
                done
            }
            "view.fileTemplates.new" => {
                let template = params.get("template").and_then(serde_json::Value::as_str);
                let folder = params.get("directory").and_then(serde_json::Value::as_str).map(PathBuf::from);
                self.open_new_from_template(registry, session, template, folder, false).map_err(Refused)?;
                done
            }
            "view.newElement.open" if !agent && !self.tree_focused => {
                Err(Refused("New… opens in the Project tool window; select it first".to_owned()))
            }
            "view.newElement.open" => {
                let folder = params.get("directory").and_then(serde_json::Value::as_str).map(PathBuf::from);
                self.open_new_popup(session, folder, None).map_err(Refused)?;
                done
            }
            "view.fileTemplates.saveAs" => {
                self.open_save_as_template(session).map_err(Refused)?;
                done
            }
            connection_dialog::OPEN => {
                if agent && let Some(id) = params.get("detection").and_then(serde_json::Value::as_str) {
                    let patterns = session
                        .state::<zen_workbench::hidden_files::HiddenFromAgents>()
                        .map(|hidden| hidden.patterns().to_vec())
                        .unwrap_or_default();
                    if zen_workbench::database::detect::hidden(session, id, &patterns) {
                        return Err(Refused(
                            "this detected database comes from a file hidden from agents; the person adds it"
                                .to_owned(),
                        ));
                    }
                }
                self.open_connection_dialog(registry, session, &params).map_err(Refused)?;
                Ok(serde_json::json!({ "dialog": "connectionDialog" }))
            }
            import_dialog::OPEN => {
                if registry.info("db.import.dbeaver").is_none() {
                    return Err(Refused(t!("ide-dbc-no-module")));
                }
                let mut dialog = import_dialog::ImportDialog::open(session, &params);
                dialog.list(registry, session);
                self.import_dialog = Some(dialog);
                Ok(serde_json::json!({ "dialog": "importConnections" }))
            }
            artifacts_popup::OPEN => {
                self.open_artifacts_popup(registry, session).map_err(Refused)?;
                Ok(serde_json::json!({ "popup": "buildArtifacts" }))
            }
            "view.commandLineLauncher.open" => {
                self.open_launcher_dialog();
                done
            }
            "view.fileEncoding.open" if !text_file => {
                Err(Refused("File Encoding needs an open text file: open one first".to_owned()))
            }
            "view.fileEncoding.open" => {
                let rect = ctx.content_rect();
                self.open_encoding_popup(registry, session, pos2(rect.right() - 280.0, rect.bottom() - 28.0));
                done
            }
            "view.associateFileType.open" => {
                let given = params.get("path").and_then(serde_json::Value::as_str).map(PathBuf::from);
                let Some(path) = given.or_else(|| self.file_context(session)) else {
                    return Err(Refused(
                        "Associate with File Type needs a file: open or select one, or give `path`".to_owned(),
                    ));
                };
                self.open_associate_dialog(path);
                done
            }
            "view.editorConfig.new" => {
                let folder = params.get("folder").and_then(serde_json::Value::as_str).map(PathBuf::from);
                let folder = folder
                    .map(|folder| match session.state::<Workbench>() {
                        Some(workbench) => workbench.resolve(&folder),
                        None => folder,
                    })
                    .or_else(|| self.new_file_folder(session));
                let Some(folder) = folder else {
                    return Err(Refused("New EditorConfig File needs an open project".to_owned()));
                };
                self.open_new_editor_config(folder);
                done
            }
            "view.editorConfig.showFiles" => {
                self.show_editor_config_files(registry, session).map_err(Dispatch)?;
                done
            }
            "view.editorConfig.disable" => {
                self.disable_editor_config(session);
                done
            }
            ide_zoom::OPEN | ide_zoom::IN | ide_zoom::OUT | ide_zoom::RESET => {
                Ok(self.ide_zoom_action(action).unwrap_or_default())
            }
            zen_workbench::internal_errors::OPEN_ACTION => {
                self.open_errors_dialog();
                done
            }
            "view.welcome.open" => {
                let section = welcome::WelcomeSection::from_params(&params).map_err(Refused)?;
                self.requests.push(Request::Welcome(section));
                Ok(serde_json::json!({ "section": section.map(welcome::WelcomeSection::name) }))
            }
            "view.guide.open" => {
                let url = welcome::guide_url(&params).map_err(Refused)?;
                ctx.open_url(egui::OpenUrl::new_tab(url.clone()));
                Ok(serde_json::json!({ "url": url }))
            }
            "view.ideSettings.export" => {
                self.open_export_settings(registry, session, &params).map_err(Refused)?;
                done
            }
            "view.ideSettings.import" => {
                self.open_import_settings(registry, session, &params).map_err(Refused)?;
                done
            }
            "view.ideSettings.restoreDefaults" => {
                self.open_restore_settings(registry, session, &params, agent).map_err(Refused)
            }
            zen_workbench::external_tools::RUN_ACTION => self.run_external_tool(registry, session, &params),
            commit_window::JUMP_TO_SOURCE => self.jump_to_source(ctx, registry, session, &params),
            "view.vcs.showDiff" => {
                let path =
                    session.state::<Workbench>().and_then(Workbench::active_document).map(|d| d.path().to_owned());
                let Some(path) = path else {
                    return Err(Refused("Show Diff needs a file open in the editor".to_owned()));
                };
                self.open_diff(ctx, registry, session, &path);
                done
            }
            "view.toolWindow.terminal" => {
                self.toggle_tool("terminal");
                done
            }
            "view.toolWindow.problems" => {
                self.toggle_tool("problems");
                done
            }
            "view.toolWindow.find" => {
                self.toggle_tool("find");
                done
            }
            "view.toolWindow.hierarchy" => {
                self.toggle_tool("hierarchy");
                done
            }
            "view.toolWindow.coverage" => {
                self.toggle_tool("coverage");
                done
            }
            "view.toolWindow.todo" => {
                self.toggle_tool("todo");
                done
            }
            "view.toolWindow.maven" | "view.toolWindow.gradle" => {
                let tool = if action == "view.toolWindow.maven" {
                    build_tool_window::Tool::Maven
                } else {
                    build_tool_window::Tool::Gradle
                };
                self.toggle_build_tool(tool).map_err(Refused)?;
                if self.tool_visible(tool.id()) {
                    self.build_tools.read_now(tool, registry, session);
                }
                done
            }
            build_sync_window::LOAD_CHANGES => self.load_build_changes(registry, session, &params).map_err(Refused),
            "view.toolWindow.build" => {
                if !self.build_tools.available("build") {
                    return Err(Refused(t!("ide-sync-not-available")));
                }
                self.toggle_tool("build");
                done
            }
            dependency_analyzer::OPEN
            | dependency_analyzer::SET
            | dependency_analyzer::SELECT
            | dependency_analyzer::TOGGLE
            | dependency_analyzer::REFRESH => {
                self.dependency_analyzer_action(registry, session, action, &params).map_err(Refused)
            }
            "view.toolWindow.ant" => {
                self.toggle_ant(registry, session).map_err(Refused)?;
                done
            }
            "view.toolWindow.npm" => {
                self.toggle_npm(registry, session).map_err(Refused)?;
                done
            }
            "view.buildTool.select" | "view.buildTool.toggle" | "view.buildTool.run"
                if params.get("tool").and_then(serde_json::Value::as_str) == Some("npm") =>
            {
                self.npm_tool_action(registry, session, action, &params).map_err(Refused)
            }
            "view.toolWindow.endpoints" => {
                self.toggle_endpoints(registry, session).map_err(Refused)?;
                done
            }
            "view.toolWindow.dependencies" => {
                self.toggle_dependencies(registry, session).map_err(Refused)?;
                done
            }
            "view.dependencies.select" | "view.dependencies.search" => {
                self.dependencies_action(registry, session, action, &params).map_err(Refused)
            }
            "view.buildTool.select" | "view.buildTool.toggle" | "view.buildTool.run"
                if params.get("tool").and_then(serde_json::Value::as_str) == Some("ant") =>
            {
                self.ant_tool_action(registry, session, action, &params).map_err(Refused)
            }
            "view.buildTool.select" | "view.buildTool.toggle" | "view.buildTool.run" => {
                self.build_tool_action(registry, session, action, &params).map_err(Refused)
            }
            build_tool_window::MENU
            | build_tool_window::SEARCH
            | build_tool_window::EXECUTE_GOAL
            | build_tool_window::EXECUTE_TASK
            | build_tool_window::EFFECTIVE_POM
            | build_tool_window::BEFORE_RUN
            | build_tool_window::TASK_BEFORE_RUN
            | build_tool_window::ASSIGN_SHORTCUT => {
                self.tool_window_action(registry, session, action, &params).map_err(Refused)
            }
            "view.occurrence.next" | "view.occurrence.previous" => {
                if !self.tool_visible("todo") {
                    return Err(Refused(
                        "Previous and Next Occurrence move through the TODO tool window's items; show it first \
                         (view.toolWindow.todo)"
                            .to_owned(),
                    ));
                }
                match self.todo_window.occurrence(action == "view.occurrence.next") {
                    Some(go) => match go.command() {
                        Some((command, params)) => registry.dispatch(session, command, params).map_err(Dispatch),
                        None => done,
                    },
                    None => done,
                }
            }
            "view.inspectCode.open" => {
                self.open_inspect_dialog(registry, session, false);
                done
            }
            "view.inspectByName.open" => {
                self.open_inspect_dialog(registry, session, true);
                done
            }
            "view.codeCleanup.open" => {
                self.open_cleanup_dialog(registry, session);
                done
            }
            "view.silentCleanup.run" => {
                self.silent_cleanup(registry, session).map_err(Refused)?;
                done
            }
            "view.inspectResults.tab" => {
                let run = params.get("run").and_then(serde_json::Value::as_u64);
                let known = run.is_none_or(|run| self.inspect_tabs(session).iter().any(|(tab, _)| *tab == run));
                if !known {
                    return Err(Refused("`run` is not a run with results".to_owned()));
                }
                self.inspect_window.tab = run;
                self.show_tool("problems");
                done
            }
            "view.inspectResults.groupByDirectory.toggle" => {
                let options = &mut self.inspect_window.options;
                options.group_by_directory = !options.group_by_directory;
                Ok(serde_json::json!({ "groupByDirectory": options.group_by_directory }))
            }
            "view.inspectResults.showExcluded.toggle" => {
                let window = &mut self.inspect_window;
                window.show_excluded = !window.show_excluded;
                Ok(serde_json::json!({ "showExcluded": window.show_excluded }))
            }
            "view.todo.tab" => {
                let tab = match params.get("tab").filter(|tab| !tab.is_null()) {
                    None => self.todo_window.options.tab,
                    Some(tab) => serde_json::from_value::<zen_workbench::todo::TodoTab>(tab.clone()).map_err(|_| {
                        Refused("`tab` is one of project, currentFile, scopeBased and currentChangelist".to_owned())
                    })?,
                };
                let scope = params.get("scope").and_then(serde_json::Value::as_str).map(str::to_owned);
                self.todo_window.select_tab(tab, scope).map_err(Refused)?;
                self.show_tool("todo");
                done
            }
            "view.todo.filter" => {
                let wanted = params.get("name").and_then(serde_json::Value::as_str).map(str::to_owned);
                let known =
                    |name: &String| self.overview.settings.todo.filters.iter().any(|filter| filter.name == *name);
                if let Some(name) = wanted.as_ref().filter(|name| !known(name)) {
                    return Err(Refused(format!("no TODO filter `{name}`")));
                }
                self.todo_window.options.filter = wanted;
                self.show_tool("todo");
                done
            }
            "view.todo.expandAll" => {
                self.todo_window.expand_all();
                done
            }
            "view.todo.collapseAll" => {
                self.todo_window.collapse_all();
                done
            }
            "view.todo.previewSource.toggle" => {
                let options = &mut self.todo_window.options;
                options.preview_source = !options.preview_source;
                Ok(serde_json::json!({ "previewSource": options.preview_source }))
            }
            "view.todo.navigateWithSingleClick.toggle" => {
                let options = &mut self.todo_window.options;
                options.navigate_with_single_click = !options.navigate_with_single_click;
                Ok(serde_json::json!({ "navigateWithSingleClick": options.navigate_with_single_click }))
            }
            "view.toolWindow.run" => {
                self.toggle_tool("run");
                done
            }
            "view.toolWindow.debug" => {
                self.toggle_tool("debug");
                done
            }
            "view.toolWindow.services" => {
                self.toggle_tool("services");
                done
            }
            "view.runConfigurations.open" => {
                self.run_configurations.open();
                done
            }
            "view.testHistory.open" => {
                let at = ctx.content_rect().center_top() + vec2(-210.0, 90.0);
                self.open_test_history(registry, session, at).map_err(Dispatch)
            }
            "view.createTest.open" => {
                let options = self.open_create_test(registry, session, &params).map_err(Dispatch)?;
                Ok(serde_json::json!({ "dialog": "createTest", "options": options }))
            }
            "view.runAnything.open" => {
                if let Some(context) =
                    params.get("context").and_then(|context| serde_json::from_value(context.clone()).ok())
                {
                    self.run_anything.set_context(context);
                }
                let query = params.get("query").and_then(serde_json::Value::as_str).unwrap_or_default();
                self.run_anything.open(run_anything::Mode::Anything, query);
                done
            }
            "view.runConfigurations.choose" => {
                let debug = params.get("debug").and_then(serde_json::Value::as_bool).unwrap_or_default();
                let query = params.get("query").and_then(serde_json::Value::as_str).unwrap_or_default();
                self.run_anything.open(run_anything::Mode::Choose { debug }, query);
                done
            }
            "view.toolWindow.git" => {
                self.toggle_tool("git");
                done
            }
            "view.fullScreen.toggle" => {
                self.toggle_full_screen();
                done
            }
            "view.navigationBar.jump" => self.navigation_bar_action(ctx, registry, session, &params),
            "view.navigationBar.top"
            | "view.navigationBar.statusBar"
            | "view.navigationBar.hide"
            | "view.navigationBar.members.toggle" => Ok(self.navigation_bar_setting(action)),
            "view.codeVision.authors.toggle" => {
                let shown = self.toggle_code_authors();
                Ok(serde_json::json!({ "codeVision": { "authors": shown } }))
            }
            "view.editorFontSize.increase" => Ok(self.change_all_editors_font(Some(1.0))),
            "view.editorFontSize.decrease" => Ok(self.change_all_editors_font(Some(-1.0))),
            "view.editorFontSize.reset" => Ok(self.change_all_editors_font(None)),
            "view.powerSaveMode.toggle" => {
                let on = match params.get("on").and_then(serde_json::Value::as_bool) {
                    Some(on) if on == self.power_save_on() => on,
                    _ => self.toggle_power_save(),
                };
                Ok(serde_json::json!({ "powerSaveMode": on }))
            }
            "view.statusBarWidgets.open" => {
                if let Some(id) = params.get("widget").and_then(serde_json::Value::as_str) {
                    let Some(widget) = StatusWidget::from_id(id) else {
                        return Err(Refused(format!("there is no status bar widget `{id}`")));
                    };
                    let shown =
                        params.get("shown").and_then(serde_json::Value::as_bool).unwrap_or(!self.widget_shown(widget));
                    self.set_widget_shown(widget, shown);
                } else {
                    let at = ctx.content_rect().right_bottom() - vec2(220.0, metrics::STATUS_BAR);
                    self.open_status_menu(at);
                }
                Ok(serde_json::json!({ "widgets": status_widgets::widgets_state(&self.overview.settings) }))
            }
            "view.activityMonitor.open" => {
                self.open_activity_monitor();
                done
            }
            "view.debugLogSettings.open" => {
                self.open_debug_log_settings(registry, session);
                done
            }
            "view.collectLogs.open" => {
                self.diagnostics.collect = true;
                done
            }
            "view.specialFiles.open" => {
                self.open_special_files(registry, session);
                done
            }
            "view.openSourceLicences.open" => Ok(self.open_licences()),
            "view.whatsNew.open" => {
                let opened = self.open_whats_new(registry, session);
                self.told(opened, agent).map_err(Refused)
            }
            "view.keyboardShortcuts.open" => {
                let opened = self.open_shortcuts_reference(ctx);
                self.told(opened, agent).map_err(Refused)
            }
            "view.keyboardShortcuts.export" => {
                let written = self.export_shortcuts_reference(session, &params, agent);
                self.told(written, agent).map_err(Refused)
            }
            "view.tipOfTheDay.open" => Ok(self.open_tip(&params)),
            "view.productivity.open" => Ok(self.open_productivity()),
            "view.productivity.reset" => {
                self.reset_productivity();
                done
            }
            "view.presentationMode.toggle" | "view.distractionFreeMode.toggle" | "view.zenMode.toggle" => {
                if let Some(mode) = appearance::Mode::of_action(action) {
                    self.toggle_mode(mode);
                }
                done
            }
            "view.ide.exit" => {
                let confirmed = params.get("confirmed").and_then(serde_json::Value::as_bool).unwrap_or(false);
                self.requests.push(Request::Exit { confirmed });
                done
            }
            "view.settings.newProjects" => {
                self.settings.open_for_new_projects();
                Ok(serde_json::json!(true))
            }
            "view.settings.open" => {
                self.refresh_style_pages(session);
                let sections = self.settings_sections();
                let page = params.get("page").and_then(serde_json::Value::as_str);
                match page.and_then(|page| settings::PageId::from_name(page, &sections)) {
                    Some(page) => self.settings.open_at(page),
                    None => self.settings.open(),
                }
                if let Some(part) = params.get("section").and_then(serde_json::Value::as_str)
                    && !self.settings.show_project_structure_part(part)
                {
                    return Err(Refused(format!(
                        "no Project Structure section `{part}`; the sections are project, modules, libraries, \
                         artifacts, sdks and problems"
                    )));
                }
                if let Some(tab) = params.get("tab").and_then(serde_json::Value::as_str)
                    && !self.settings.show_plugins_tab(tab)
                {
                    return Err(Refused(format!(
                        "no Plugins tab `{tab}`; the tabs are marketplace, installed and builtIn"
                    )));
                }
                if let Some(query) = params.get("query").and_then(serde_json::Value::as_str) {
                    let now = ctx.input(|input| input.time);
                    let mut dispatch = |id: &str, params| {
                        registry.dispatch(session, id, params).map_err(|error| match error {
                            zen_core::DispatchError::Failed { error, .. } => error.to_string(),
                            other => other.to_string(),
                        })
                    };
                    let more = settings::module_texts(&self.product.folding_options);
                    let found = zen_i18n::with_language(&zen_i18n::interface_language(), || {
                        self.settings.search_for(query, &sections, &more, now, Some(&mut dispatch))
                    });
                    return Ok(serde_json::json!({ "search": found }));
                }
                done
            }
            "vcs.stash.create" | "vcs.stash.pop" if self.git_in_front(session) => {
                let result = registry.dispatch(session, action, params).map_err(Dispatch);
                self.git_workspace.history_changed();
                result
            }
            "view.vcs.update" if self.git_in_front(session) && !text_field_focused(ctx) => {
                let result = registry.dispatch(session, "vcs.workspace.pull", serde_json::json!({})).map_err(Dispatch);
                self.git_workspace.history_changed();
                result
            }
            "view.vcs.update" => {
                self.open_update();
                done
            }
            "view.vcs.push" => {
                self.open_push(registry, session);
                done
            }
            "view.vcs.newBranch" => {
                let start = params.get("start").and_then(serde_json::Value::as_str).map(str::to_owned);
                self.open_new_branch(start);
                done
            }
            "view.vcs.logShowDiff" => {
                self.git_show_diff(ctx, registry, session);
                done
            }
            "view.vcs.logSelectPaths" => {
                self.show_tool("git");
                self.git_select_paths(registry, session).map_err(Refused)?;
                done
            }
            "view.vcs.showHistory" => {
                let path = params
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .map(PathBuf::from)
                    .or_else(|| self.history_context(session));
                let Some(path) = path else {
                    return Err(Refused("Show History needs a project, a file or a `path`".to_owned()));
                };
                self.git_show_history(registry, session, &path).map_err(Refused)?;
                self.show_tool("git");
                done
            }
            _ if action.starts_with("view.vcs.log") => {
                self.show_tool("git");
                self.git_window.view_action(action);
                done
            }
            "view.vcs.checkoutRevision" => {
                self.open_checkout_revision();
                done
            }
            "view.vcs.branches" => {
                self.vcs.open_branches = true;
                done
            }
            "view.projects.popup" => {
                let current = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
                Ok(self.projects_popup_action(&params, current.as_deref()))
            }
            "view.find.open" if self.settings.is_open() => {
                self.settings.focus_search();
                done
            }
            "editor.edit.undo" | "editor.edit.redo" if self.git_in_front(session) && !text_field_focused(ctx) => {
                let command = if action == "editor.edit.undo" { "vcs.history.undo" } else { "vcs.history.redo" };
                let result = registry.dispatch(session, command, serde_json::json!({})).map_err(Dispatch);
                self.git_workspace.history_changed();
                result
            }
            "view.find.open" if self.git_in_front(session) => {
                let result = registry.dispatch(session, "vcs.graph.search", serde_json::json!({})).map_err(Dispatch);
                self.git_workspace.search_focus = true;
                result
            }
            "view.find.open" | "view.replace.open" => {
                let replace = action == "view.replace.open";
                let result = registry
                    .dispatch(session, "editor.find.open", serde_json::json!({ "replace": replace }))
                    .map_err(Dispatch);
                self.find_bar.focus();
                result
            }
            "view.findInFiles.pin" => {
                let on = params.get("on").and_then(serde_json::Value::as_bool).unwrap_or(!self.find_in_files.pinned());
                self.find_in_files.set_pinned(on);
                Ok(serde_json::json!({ "pinned": on }))
            }
            "view.findInFiles.openInFindWindow" => {
                let all = params.get("all").and_then(serde_json::Value::as_bool).unwrap_or(false);
                let new_tab = params.get("newTab").and_then(serde_json::Value::as_bool);
                let wait_ms = params.get("waitMs").and_then(serde_json::Value::as_u64);
                self.open_in_find_window(registry, session, all, new_tab, wait_ms)
            }
            "view.findResults.settings" => {
                let tab = params.get("tab").and_then(serde_json::Value::as_u64);
                self.reopen_find_in_files(registry, session, tab).map(|()| serde_json::json!({ "open": true }))
            }
            "view.findResults.newTab" => {
                let on = params.get("on").and_then(serde_json::Value::as_bool).unwrap_or(!self.find_in_files.new_tab());
                self.find_in_files.set_new_tab(on);
                Ok(serde_json::json!({ "newTab": on }))
            }
            "editor.find.searchInSelection" => {
                let shown = session.state::<Workbench>().is_some_and(|workbench| workbench.find().open);
                let result = registry.dispatch(session, action, params).map_err(Dispatch);
                if !shown && result.is_ok() {
                    self.find_bar.focus();
                }
                result
            }
            "view.findInFiles.open" | "view.replaceInFiles.open" => {
                let selected = session.state::<Workbench>().and_then(Workbench::active_document).and_then(|document| {
                    let range = document.selections().primary().range();
                    document.snapshot().slice(range).ok().map(|text| text.into_owned())
                });
                let folder = self.tree_folder(session);
                self.find_in_files.open(action == "view.replaceInFiles.open", selected, folder);
                done
            }
            "view.recentFiles.open" => {
                match params.get("editedOnly").and_then(serde_json::Value::as_bool) {
                    Some(edited_only) => self.recent_files.open_with(edited_only),
                    None => self.recent_files.press(),
                }
                let params = serde_json::json!({ "editedOnly": self.recent_files.edited_only() });
                registry.dispatch(session, "navigate.recent.files", params).map_err(Dispatch)
            }
            "view.switcher.open" => {
                let held = self.shortcut_modifiers.take();
                let backward = params
                    .get("backward")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or_else(|| held.is_some_and(|modifiers| modifiers.shift));
                let hold = held.filter(|modifiers| modifiers.ctrl || modifiers.alt || modifiers.command);
                self.recent_files.switch(backward, hold);
                registry.dispatch(session, "navigate.recent.files", serde_json::json!({})).map_err(Dispatch)
            }
            "view.recentLocations.open" => {
                self.recent_files = recent_files::RecentFiles::default();
                match params.get("editedOnly").and_then(serde_json::Value::as_bool) {
                    Some(edited_only) => self.recent_locations.open_with(edited_only),
                    None => self.recent_locations.press(),
                }
                let params = serde_json::json!({ "editedOnly": self.recent_locations.edited_only() });
                registry.dispatch(session, "navigate.recent.locations", params).map_err(Dispatch)
            }
            "view.fileStructure.open" if !text_file => Err(Refused(format!(
                "File Structure needs a text file in front in the editor; {} is there now (open one with \
                 file.document.open)",
                self.front_kind(session)
            ))),
            "view.fileStructure.open" => {
                let path =
                    session.state::<Workbench>().and_then(Workbench::active_document).map(|d| d.path().to_owned());
                let Some(path) = path else { return Err(Refused("no file is open".to_owned())) };
                let params = self.structure_window.params(&path);
                self.file_structure.open(&path, params.clone());
                registry.dispatch(session, "editor.structure.get", params).map_err(Dispatch)
            }
            "view.scratch.new" => {
                self.scratch.open_languages(registry, session, scratch_popups::LanguageTarget::New).map_err(Refused)
            }
            "view.scratch.show" => self.scratch.open_files(registry, session).map_err(Refused),
            "view.scratch.newBuffer" => {
                let clear = params.get("clear").and_then(serde_json::Value::as_bool);
                let answer = registry
                    .dispatch(session, "file.scratch.newBuffer", serde_json::json!({ "clear": clear }))
                    .map_err(Dispatch)?;
                if answer["needsAnswer"] == true
                    && let Some(path) = answer["path"].as_str()
                {
                    self.scratch.ask_clear(PathBuf::from(path));
                }
                Ok(answer)
            }
            "view.project.move" if !self.moves_scratch(session, &params, agent) => {
                let context = self.refactoring_place(ctx, session, &params, agent, true)?;
                self.open_refactoring_for(session, "move", context)
            }
            "view.changeSignature.open" => {
                let context = self.refactoring_place(ctx, session, &params, agent, false)?;
                self.open_refactoring_for(session, "changeSignature", context)
            }
            "view.pullMembersUp.open"
            | "view.pushMembersDown.open"
            | "view.encapsulateFields.open"
            | "view.extract.parameterObject"
            | "view.extract.interface"
            | "view.extract.superclass" => {
                let context = self.refactoring_place(ctx, session, &params, agent, false)?;
                let action = match action {
                    "view.pullMembersUp.open" => "pullMembersUp",
                    "view.pushMembersDown.open" => "pushMembersDown",
                    "view.encapsulateFields.open" => "encapsulateFields",
                    "view.extract.parameterObject" => "introduceParameterObject",
                    "view.extract.interface" => "extractInterface",
                    _ => "extractSuperclass",
                };
                self.open_refactoring_for(session, action, context)
            }
            "view.project.move" | "view.project.changeLanguage" => {
                let given = params.get("path").and_then(serde_json::Value::as_str).map(PathBuf::from);
                let lead = (agent || self.tree_focused).then(|| self.selection.lead().cloned()).flatten();
                let scratches = session.state::<zen_workbench::scratch::Scratches>();
                let scratch = given
                    .or(lead)
                    .filter(|path| path.is_file() && scratches.is_some_and(|scratches| scratches.contains(path)));
                let Some(path) = scratch else {
                    return Err(Refused(
                        "give the `path` of a scratch file, or select one under Scratches and Consoles in the Project \
                         tool window"
                            .to_owned(),
                    ));
                };
                if action == "view.project.move" {
                    let project = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
                    let Some(project) = project else { return Err(Refused("no project is open".to_owned())) };
                    self.scratch.open_move(path, &project);
                    done
                } else {
                    let target = scratch_popups::LanguageTarget::Change(path);
                    self.scratch.open_languages(registry, session, target).map_err(Refused)
                }
            }
            "view.customFolding.open" if !text_file => Err(Refused(format!(
                "Custom Folding needs a text file in front in the editor; {} is there now (open one with \
                 file.document.open, or call editor.fold.regions)",
                self.front_kind(session)
            ))),
            "view.customFolding.open" => {
                let regions =
                    registry.dispatch(session, "editor.fold.regions", serde_json::json!({})).map_err(Dispatch)?;
                self.editor_view.open_custom_folding(&regions);
                Ok(regions)
            }
            "view.pasteFromHistory.open" if !text_file => Err(Refused(format!(
                "Paste from History needs a text file in front in the editor; {} is there now (open one with \
                 file.document.open)",
                self.front_kind(session)
            ))),
            "view.recentFindUsages.open" => {
                let recent =
                    registry.dispatch(session, "navigate.usages.recent", serde_json::json!({})).map_err(Dispatch)?;
                self.recent_usages.open(&recent);
                Ok(recent)
            }
            "view.quickSwitchScheme.open" if self.quick_switch.is_open() => Ok(self.quick_switch.to_json()),
            "view.quickSwitchScheme.open" => {
                let groups = self.quick_switch_groups(registry, session);
                self.quick_switch.open(groups);
                Ok(self.quick_switch.to_json())
            }
            "view.pasteFromHistory.open" => {
                self.paste_history.open();
                registry.dispatch(session, "editor.clipboardHistory.list", serde_json::json!({})).map_err(Dispatch)
            }
            "view.gotoClass.open" => {
                self.search_everywhere.toggle(search_everywhere::Tab::CLASSES);
                done
            }
            "view.gotoSymbol.open" => {
                self.search_everywhere.toggle(search_everywhere::Tab::SYMBOLS);
                done
            }
            "view.gotoFile.open" => {
                self.search_everywhere.toggle(search_everywhere::Tab::FILES);
                done
            }
            "view.gotoText.open" => {
                self.search_everywhere.toggle(search_everywhere::Tab::TEXT);
                done
            }
            "view.gotoDatabaseObject.open" if registry.info("db.object.goTo").is_none() => {
                Err(Refused("database support is off in this IDE".to_owned()))
            }
            "view.gotoDatabaseObject.open" => {
                self.search_everywhere.toggle(search_everywhere::Tab::DATABASE);
                done
            }
            "view.findAction.open" => {
                self.search_everywhere.toggle(search_everywhere::Tab::ACTIONS);
                done
            }
            "view.searchEverywhere.open" => {
                self.search_everywhere.toggle(search_everywhere::Tab::ALL);
                done
            }
            "view.refactoring.open" => {
                let provider = params.get("provider").and_then(serde_json::Value::as_str).unwrap_or_default();
                let providers =
                    session.state_mut::<zen_workbench::language::refactoring_dialog::RefactoringProviders>();
                if providers.get(provider).is_none() {
                    return Err(Refused(format!(
                        "there is no refactoring {provider:?}; refactor.dialog.list names them"
                    )));
                }
                let context = params.get("context").cloned().unwrap_or(serde_json::Value::Null);
                self.open_refactoring(provider.to_owned(), context);
                done
            }
            "view.safeDelete.open" if params.get("paths").is_some() => {
                let paths = params.get("paths").cloned().unwrap_or_default();
                self.open_refactoring("safeDelete".to_owned(), serde_json::json!({ "paths": paths }));
                done
            }
            "view.safeDelete.open" if !agent && self.tree_focused && self.project_open => {
                let paths = self.deletable_selection();
                if paths.is_empty() {
                    return Err(Refused("select files or folders of the project to delete".to_owned()));
                }
                self.open_refactoring("safeDelete".to_owned(), serde_json::json!({ "paths": paths }));
                done
            }
            "view.rename.open" if !agent && self.tree_focused && self.project_open => {
                let root = self.visible_rows.first().cloned();
                let lead = self.selection.lead().cloned().filter(|lead| {
                    Some(lead) != root.as_ref() && lead.exists() && !self.scratch_folders.contains(lead)
                });
                match lead {
                    Some(lead) => {
                        self.open_rename_entry(lead);
                        done
                    }
                    None => Err(Refused("select a file or folder of the project to rename".to_owned())),
                }
            }
            "file.lineSeparator.set" | "file.bom.add" | "file.bom.remove"
                if !agent && self.tree_focused && params.get("paths").is_none() =>
            {
                let paths = selection::outermost(&self.selection.items(&self.visible_rows));
                let mut params = params;
                if let Some(object) = params.as_object_mut() {
                    object.insert("paths".to_owned(), serde_json::json!(paths));
                }
                registry.dispatch(session, action, params).map_err(Dispatch)
            }
            "view.project.select" => {
                let paths: Vec<PathBuf> = params
                    .get("paths")
                    .and_then(|paths| serde_json::from_value(paths.clone()).ok())
                    .unwrap_or_default();
                let project = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
                let Some(project) = project else { return Err(Refused("no project is open".to_owned())) };
                if paths.is_empty() {
                    return Err(Refused("give `paths`: existing files or folders of the project".to_owned()));
                }
                self.select_paths(&project, paths).map_err(Refused)?;
                self.show_tool("project");
                self.tree_focused = true;
                done
            }
            "view.project.selectOpened" => {
                let project = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
                let Some(project) = project else { return Err(Refused("no project is open".to_owned())) };
                let file =
                    session.state::<Workbench>().and_then(Workbench::active_document).map(|d| d.path().to_owned());
                self.select_opened_file(&project, file.as_deref()).map_err(Refused)?;
                self.show_tool("project");
                Ok(serde_json::json!({ "selected": file }))
            }
            "view.project.alwaysSelectOpened" => {
                let on = match params.get("on") {
                    None | Some(serde_json::Value::Null) => !self.always_select_opened,
                    Some(serde_json::Value::Bool(on)) => *on,
                    Some(_) => return Err(Refused("`on` is true or false".to_owned())),
                };
                self.set_always_select_opened(on);
                Ok(serde_json::json!({ "alwaysSelectOpened": on }))
            }
            "view.project.contextMenu" => {
                let path = params.get("path").and_then(serde_json::Value::as_str).map(PathBuf::from);
                let Some(path) = path.or_else(|| self.selection.lead().cloned()).filter(|path| path.exists()) else {
                    return Err(Refused("give the `path` of a file or folder shown in the tree".to_owned()));
                };
                if !self.selection.contains(&path) {
                    self.selection.select(path.clone());
                }
                self.show_tool("project");
                self.menu_request = Some((path, true));
                done
            }
            "view.project.excludedFiles" => {
                let show = params.get("show").and_then(serde_json::Value::as_bool).unwrap_or(!self.show_excluded);
                self.show_excluded = show;
                self.show_tool("project");
                Ok(serde_json::json!({ "shown": show }))
            }
            "view.project.rename" => {
                let path = params.get("path").and_then(serde_json::Value::as_str).map(PathBuf::from);
                let Some(path) = path.filter(|path| path.exists()) else {
                    return Err(Refused("Rename needs the `path` of an existing file or folder".to_owned()));
                };
                self.open_rename_entry(path);
                done
            }
            "view.selectIn.open" => {
                let path = self.select_in_file(session, &params, agent)?;
                let targets = self.select_in_targets(ctx, registry, session, &path);
                self.select_in.open(&path, targets.clone());
                let targets: Vec<&str> = targets.into_iter().map(select_in::Target::id).collect();
                Ok(serde_json::json!({ "path": path, "targets": targets }))
            }
            "view.selectIn.target" => {
                let path = self.select_in_file(session, &params, agent)?;
                let id = params.get("target").and_then(serde_json::Value::as_str).unwrap_or_default();
                let Some(target) = select_in::Target::from_id(id) else {
                    return Err(Refused(
                        "give `target`: projectView, structure, commit, finder or terminal".to_owned(),
                    ));
                };
                self.select_in.close();
                self.select_in_target(ctx, registry, session, (target, &path), &params, agent)
            }
            "view.project.openInTerminal" => {
                let path = params.get("path").and_then(serde_json::Value::as_str).map(PathBuf::from);
                let project = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
                let Some(path) = path.or(project).filter(|path| path.exists()) else {
                    return Err(Refused("Open In Terminal needs the `path` of a file or folder".to_owned()));
                };
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let folder = folder_of(&path);
                    let opened = registry
                        .dispatch(session, "terminal.session.new", serde_json::json!({ "cwd": folder }))
                        .map_err(Dispatch)?;
                    Ok(serde_json::json!({ "tab": opened["tab"], "id": opened["id"], "folder": folder }))
                }
                #[cfg(target_arch = "wasm32")]
                Err(Refused("there is no terminal here".to_owned()))
            }
            "view.vcs.commitFiles" => {
                let paths = self.given_paths(&params);
                if paths.is_empty() {
                    return Err(Refused("Commit File… needs `paths`".to_owned()));
                }
                self.open_commit_files(registry, session, &paths);
                done
            }
            "view.project.compareWith" => self.compare_with(ctx, registry, session, &params, agent),
            "view.resourceBundle.open"
            | "view.resourceBundle.select"
            | "view.resourceBundle.filter"
            | "view.resourceBundle.addKey"
            | "view.resourceBundle.renameKey" => self.bundle_action(registry, session, action, &params),
            "view.folderDiff.compare"
            | "view.folderDiff.refresh"
            | "view.folderDiff.filter"
            | "view.folderDiff.select"
            | "view.folderDiff.setAction"
            | "view.folderDiff.synchronize"
            | "view.folderDiff.open" => self.folder_action(ctx, registry, session, action, &params, agent),
            "view.compare.clipboard"
            | "view.diff.blank"
            | "view.diff.swapSides"
            | "view.diff.whitespace"
            | "view.diff.viewer"
            | "view.diff.setText"
            | "view.diff.next"
            | "view.diff.previous" => self.diff_action(registry, session, action, &params),
            "view.gotoLine.open" | "view.rename.open" if !text_file => Err(Refused(format!(
                "{} needs a text file in front in the editor; {} is there now (open one with file.document.open)",
                if action == "view.gotoLine.open" { "Go to Line" } else { "Rename" },
                self.front_kind(session)
            ))),
            "view.gotoLine.open" => {
                self.open_goto();
                done
            }
            "editor.code.reformat" | "editor.imports.optimize" | "editor.code.fixAll"
                if params.get("path").is_none()
                    && session.state::<Workbench>().and_then(Workbench::active_document).is_none() =>
            {
                Err(Refused(format!(
                    "{action} needs a file open in the editor or a `path`; {} is there now",
                    self.front_kind(session)
                )))
            }
            "view.bookmark.goToMnemonic" if self.bookmarks.name() == Some("goToMnemonic") => {
                self.bookmarks.close();
                done
            }
            "view.bookmark.goToMnemonic" => {
                self.bookmarks.open_go_to(registry, session);
                done
            }
            "view.bookmark.show" if self.bookmarks.name() == Some("lineBookmarks") => {
                self.bookmarks.close();
                done
            }
            evaluate::EVALUATE | evaluate::QUICK_EVALUATE => {
                let stopped = registry
                    .dispatch(session, "debug.session.get", serde_json::json!({}))
                    .is_ok_and(|state| state["state"] == "stopped");
                if !stopped {
                    return Err(Refused(t!("ide-evaluate-not-stopped")));
                }
                let given = params.get("expression").and_then(serde_json::Value::as_str).map(str::to_owned);
                let expression = given.or_else(|| evaluate::at_caret(session));
                let caret = self.editor_view.caret_place().rect();
                let anchor = self.editor_view.take_value_anchor();
                if action == evaluate::EVALUATE {
                    let at = caret.map_or(pos2(240.0, 160.0), |rect| rect.left_bottom() + vec2(0.0, 8.0));
                    self.evaluations.open_dialog(registry, session, &expression.unwrap_or_default(), at);
                    return Ok(serde_json::json!({ "dialog": "evaluateExpression" }));
                }
                let Some(expression) = expression.filter(|expression| !expression.trim().is_empty()) else {
                    return Err(Refused(t!("ide-evaluate-nothing")));
                };
                let anchor =
                    anchor.or(caret).unwrap_or_else(|| Rect::from_min_size(pos2(240.0, 160.0), vec2(1.0, 16.0)));
                self.evaluations.quick(&expression, anchor, ctx.input(|input| input.time));
                Ok(serde_json::json!({ "popup": "quickEvaluate" }))
            }
            smart_step::SMART_STEP_INTO => {
                let anchor = self
                    .editor_view
                    .caret_place()
                    .rect()
                    .unwrap_or_else(|| Rect::from_min_size(pos2(240.0, 160.0), vec2(1.0, 16.0)));
                self.smart_step.open(registry, session, anchor).map_err(Refused)?;
                if let Some((command, params)) = self.smart_step.poll(registry, session) {
                    registry.dispatch(session, command, params).map_err(Dispatch)?;
                    return Ok(serde_json::json!({ "stepped": true }));
                }
                Ok(serde_json::json!({ "popup": "smartStepInto" }))
            }
            stream_trace::STREAM_TRACE => {
                self.stream_traces.open(registry, session, &params).map_err(Refused)?;
                Ok(serde_json::json!({ "dialog": "streamTrace" }))
            }
            breakpoints_dialog::VIEW => {
                let place = self.breakpoint_place(session, &params).ok();
                self.breakpoint_dialogs.open_view(registry, session, place);
                Ok(serde_json::json!({ "dialog": "breakpoints" }))
            }
            breakpoints_dialog::PROPERTIES | breakpoints_dialog::ADD_CONDITIONAL | breakpoints_dialog::ADD_LOGGING => {
                let (path, line) = self.breakpoint_place(session, &params).map_err(Refused)?;
                let mut set = serde_json::json!({ "path": path, "line": line });
                if action == breakpoints_dialog::ADD_LOGGING {
                    set["suspend"] = serde_json::json!("none");
                    set["logHit"] = serde_json::json!(true);
                }
                let exists = session
                    .state::<zen_workbench::debug::Breakpoints>()
                    .is_some_and(|breakpoints| breakpoints.get(&path, line).is_some());
                if action != breakpoints_dialog::PROPERTIES && (!exists || action == breakpoints_dialog::ADD_LOGGING) {
                    registry.dispatch(session, "debug.breakpoint.set", set).map_err(Dispatch)?;
                }
                let focus = match action {
                    breakpoints_dialog::ADD_CONDITIONAL => Some("condition"),
                    breakpoints_dialog::ADD_LOGGING => Some("logExpression"),
                    _ => params.get("focus").and_then(serde_json::Value::as_str),
                };
                let at = self
                    .editor_view
                    .take_breakpoint_anchor()
                    .or_else(|| self.editor_view.caret_place().rect().map(|rect| rect.left_bottom()))
                    .unwrap_or(pos2(240.0, 160.0));
                let place = (path.clone(), line);
                if !self.breakpoint_dialogs.open_popup(registry, session, place, at, focus) {
                    return Err(Refused(format!("{}:{} has no breakpoint", path.display(), line + 1)));
                }
                self.editor_view.forget_breakpoints();
                Ok(serde_json::json!({ "dialog": "breakpointProperties" }))
            }
            "view.bookmark.show" => {
                self.bookmarks.open_show(registry, session);
                Ok(serde_json::json!({ "rows": self.bookmarks.shown_rows() }))
            }
            "navigate.bookmark.toggle" if !agent && self.tree_focused && self.project_open => {
                let paths = self.tree_bookmark_targets();
                if paths.is_empty() {
                    return Err(Refused("select files or folders in the Project tool window".to_owned()));
                }
                registry
                    .dispatch(session, "navigate.bookmark.toggleFiles", serde_json::json!({ "paths": paths }))
                    .map_err(Dispatch)
            }
            "view.bookmark.mnemonic" if !agent && self.tree_focused && self.project_open => {
                let Some(lead) = self.tree_bookmark_targets().pop() else {
                    return Err(Refused("select a file or folder in the Project tool window".to_owned()));
                };
                self.bookmarks.open_mnemonic(registry, session, lead, None);
                done
            }
            "view.bookmark.openTabs" => {
                let open = session.state::<Workbench>().is_some_and(|workbench| !workbench.documents().is_empty());
                if !open {
                    return Err(Refused("Bookmark Open Tabs needs a file open in the editor".to_owned()));
                }
                self.bookmarks.open_tabs_name(registry, session);
                done
            }
            "view.bookmark.mnemonic" | "view.bookmark.rename" | "view.bookmark.moveToList" => {
                let (path, line) = self.bookmark_place(session, &params).map_err(Refused)?;
                let opened = match action {
                    "view.bookmark.mnemonic" => {
                        self.bookmarks.open_mnemonic(registry, session, path.clone(), line);
                        true
                    }
                    "view.bookmark.rename" => self.bookmarks.open_rename(registry, session, path.clone(), line),
                    _ => self.bookmarks.open_move_to_list(registry, session, path.clone(), line),
                };
                if !opened {
                    let place = match line {
                        Some(line) => format!("{}:{}", path.display(), line + 1),
                        None => path.display().to_string(),
                    };
                    return Err(Refused(format!("{place} has no bookmark")));
                }
                done
            }
            "view.safeDelete.open" if !text_file => Err(Refused(format!(
                "Safe Delete needs a text file in front in the editor or files selected in the Project tool \
                 window; {} is there now (pass paths, or call refactor.safeDelete.find and refactor.safeDelete.apply)",
                self.front_kind(session)
            ))),
            "view.safeDelete.open" if agent || self.editor_view.has_focus_in(ctx) => {
                let at = session.state::<Workbench>().and_then(Workbench::active_document).map(|document| {
                    serde_json::json!({ "path": document.path(), "offset": document.selections().primary().head })
                });
                let Some(at) = at else { return Err(Refused("no file is open".to_owned())) };
                self.open_refactoring("safeDelete".to_owned(), at);
                done
            }
            "view.safeDelete.open" => {
                Err(Refused("Safe Delete needs the editor or the Project tool window to have the keyboard".to_owned()))
            }
            "view.rename.open" if agent || self.editor_view.has_focus_in(ctx) => {
                self.open_rename(session);
                done
            }
            "view.rename.open" => Err(Refused("Rename needs the editor to have the keyboard".to_owned())),
            "view.toolWindow.notifications" => {
                self.toggle_notifications();
                done
            }
            zen_workbench::notifications::RUN_ACTION => self.notification_action(ctx, registry, session, params, agent),
            "view.update.install" => {
                self.requests.push(Request::Update);
                done
            }
            "view.ide.restart" => {
                let confirmed = params.get("confirmed").and_then(serde_json::Value::as_bool).unwrap_or(false);
                let ask = match (agent, confirmed) {
                    (true, _) => RestartAsk::Person,
                    (false, true) => RestartAsk::Never,
                    (false, false) => RestartAsk::Always,
                };
                self.requests.push(Request::RestartAsking(ask, None));
                done
            }
            "view.invalidateCaches.open" => {
                self.open_invalidate_dialog();
                done
            }
            "view.repairIde.start" => {
                self.listings.clear();
                let report = registry.dispatch(session, "ide.repair.run", serde_json::json!({}));
                report.map_err(Dispatch)
            }
            "view.toolWindow.caffeine" => {
                self.toggle_tool("caffeine");
                done
            }
            "view.indent.menu" => {
                self.indent_ui.open_menu = true;
                done
            }
            "view.update.later" => {
                let version = params.get("version").and_then(serde_json::Value::as_str).map(str::to_owned);
                let version = version.or_else(|| self.overview.update.as_ref().map(|offer| offer.version.clone()));
                let Some(version) = version else {
                    return Err(Refused("no new version is on offer".to_owned()));
                };
                self.requests.push(Request::DismissUpdate(version));
                done
            }
            "view.plugin.install" => {
                let id = params.get("id").and_then(serde_json::Value::as_str).unwrap_or_default();
                self.plugins.install_plugin_by_id(id).map_err(Refused)?;
                done
            }
            "view.notifications.balloonsOff" => {
                let id = params.get("group").and_then(serde_json::Value::as_str).unwrap_or_default();
                let Some(group) = Group::ALL.into_iter().find(|group| group.id() == id) else {
                    return Err(Refused(format!("there is no notification group `{id}`")));
                };
                let mut settings = self.overview.settings.clone();
                settings.balloon_groups_off.insert(group.id().to_owned());
                self.overview.settings = settings.clone();
                self.requests.push(Request::Settings(Box::new(settings)));
                Ok(serde_json::json!({ "balloonGroupsOff": self.overview.settings.balloon_groups_off }))
            }
            "view.plugin.ignore" => {
                let extension = params.get("extension").and_then(serde_json::Value::as_str).unwrap_or_default();
                self.plugins.ignore(extension);
                done
            }
            "view.plugin.ignoreForProjects" => {
                let id = params.get("id").and_then(serde_json::Value::as_str).unwrap_or_default();
                if id.is_empty() {
                    return Err(Refused("give the plug-in's id".to_owned()));
                }
                self.plugins.ignore_for_projects(id);
                done
            }
            "view.debugger.install" => {
                let Some(offer) = self.product.debugger else {
                    return Err(Refused("no built-in plug-in that is on installs a debugger".to_owned()));
                };
                let result = registry.dispatch(session, offer.command, params).map_err(Dispatch)?;
                self.notify(NewNotification::message(Group::Debugger, Kind::Info, t!(offer.installing)));
                Ok(result)
            }
            "view.intentions.open" | "view.problem.fix" if !text_file => Err(Refused(format!(
                "Show Context Actions needs a text file in front in the editor; {} is there now (open one with \
                 file.document.open, or call editor.intentions.list or problems.fixes.list)",
                self.front_kind(session)
            ))),
            "view.intentions.open" | "view.problem.fix" if agent || self.editor_view.has_focus_in(ctx) => {
                if action == "view.problem.fix" {
                    self.editor_view.context_actions(ctx, registry, session, true);
                } else {
                    self.editor_view.show_context_actions(ctx, registry, session);
                }
                done
            }
            "view.intentions.open" | "view.problem.fix" => {
                Err(Refused("Show Context Actions needs the editor to have the keyboard".to_owned()))
            }
            "view.generate.open" | "view.overrideMethods.open" | "view.implementMethods.open" if !text_file => {
                Err(Refused(format!(
                    "Generate needs a text file in front in the editor; {} is there now (open one with \
                     file.document.open, or call editor.generate.list and editor.generate.apply)",
                    self.front_kind(session)
                )))
            }
            "view.generate.open" | "view.overrideMethods.open" | "view.implementMethods.open"
                if agent || self.editor_view.has_focus_in(ctx) =>
            {
                self.editor_view.open_generate(match action {
                    "view.overrideMethods.open" => Some("override"),
                    "view.implementMethods.open" => Some("implement"),
                    _ => None,
                });
                done
            }
            "view.generate.open" | "view.overrideMethods.open" | "view.implementMethods.open" => {
                Err(Refused("Generate needs the editor to have the keyboard".to_owned()))
            }
            "view.surround.open" | "view.surroundWithTemplate.open" if !text_file => Err(Refused(format!(
                "Surround With needs a text file in front in the editor; {} is there now (open one with \
                 file.document.open, or call editor.surround.list and editor.surround.apply)",
                self.front_kind(session)
            ))),
            "view.surround.open" | "view.surroundWithTemplate.open" if agent || self.editor_view.has_focus_in(ctx) => {
                self.editor_view.open_surround(registry, session, action == "view.surroundWithTemplate.open");
                done
            }
            "view.surround.open" | "view.surroundWithTemplate.open" => {
                Err(Refused("Surround With needs the editor to have the keyboard".to_owned()))
            }
            id if extract_popup::command_of(id).is_some() && !text_file => Err(Refused(format!(
                "Extract and Inline need a text file in front in the editor; {} is there now (open one with \
                 file.document.open, or call {} with a path)",
                self.front_kind(session),
                extract_popup::command_of(id).unwrap_or_default()
            ))),
            id if extract_popup::command_of(id).is_some() && (agent || self.editor_view.has_focus_in(ctx)) => {
                if let Some(command) = extract_popup::command_of(id) {
                    self.editor_view.open_extract(command);
                }
                done
            }
            id if extract_popup::command_of(id).is_some() => {
                Err(Refused("Extract and Inline need the editor to have the keyboard".to_owned()))
            }
            "view.unwrap.open" if !text_file => Err(Refused(format!(
                "Unwrap/Remove needs a text file in front in the editor; {} is there now (open one with \
                 file.document.open, or call editor.unwrap.list and editor.unwrap.apply)",
                self.front_kind(session)
            ))),
            "view.unwrap.open" if agent || self.editor_view.has_focus_in(ctx) => {
                let count = self.editor_view.open_unwrap(registry, session).map_err(Refused)?;
                Ok(serde_json::json!({ "entries": count, "applied": count == 1, "popup": count > 1 }))
            }
            "view.unwrap.open" => Err(Refused("Unwrap/Remove needs the editor to have the keyboard".to_owned())),
            zen_workbench::spelling::SAVE_TO => self
                .editor_view
                .select_dictionary(ctx, session, &params)
                .map(|()| serde_json::Value::Null)
                .map_err(Refused),
            "lsp.server.install" | "lsp.server.dismiss" => {
                let id = params["id"].as_str().unwrap_or_default().to_owned();
                let started = self.lsp.follow_notification(action, &id);
                let answer = registry.dispatch(session, action, params).map_err(Dispatch)?;
                if let Some(text) = started {
                    self.notify(NewNotification::message(Group::LanguageServers, Kind::Info, text));
                }
                Ok(answer)
            }
            "coverage.run.start" => {
                let runs =
                    |session: &Session| session.state::<zen_workbench::run::Runs>().map_or(0, |runs| runs.runs().len());
                let before = runs(session);
                let result = registry.dispatch(session, action, params).map_err(Dispatch);
                if runs(session) > before {
                    self.show_tool("run");
                }
                result
            }
            "run.process.start" | "debug.session.start" => {
                if !agent && let Some(id) = run_configurations::page_before_start(registry, session, &params) {
                    self.run_configurations.open_to_launch(&id, action);
                    return Ok(serde_json::json!({ "dialog": "runConfigurations", "configurationId": id }));
                }
                self.launch_run(registry, session, action, params, agent)
            }
            "view.macro.toggleRecording" | "view.macro.edit" | "view.macro.playSaved" => {
                self.macro_action(ctx, registry, session, action, &params)
            }
            command => match zen_workbench::workspaces::refusal(session, command, &params) {
                Some(reason) => Err(Refused(reason)),
                None => registry.dispatch(session, command, params).map_err(Dispatch),
            },
        }
    }

    fn launch_run(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        action: &str,
        params: serde_json::Value,
        agent: bool,
    ) -> Result<serde_json::Value, agent::ActionError> {
        if action == "debug.session.start" {
            let result = registry.dispatch(session, action, params).map_err(agent::ActionError::Dispatch);
            if result.is_ok() {
                self.show_debug_window();
            }
            return result;
        }
        self.start_run(registry, session, params, agent)
    }

    pub(super) fn select_paths(&mut self, project: &Path, paths: Vec<PathBuf>) -> Result<(), String> {
        let in_library = |view: &Self, path: &Path| {
            view.libraries.as_ref().is_some_and(|libraries| libraries.library_of(project, path).is_some())
        };
        if let Some(outside) = paths
            .iter()
            .find(|path| !path.exists() || !(zen_workbench::inside_project(project, path) || in_library(self, path)))
        {
            return Err(format!("{} is not a file or folder of the project or its libraries", outside.display()));
        }
        for path in &paths {
            if !zen_workbench::inside_project(project, path) {
                self.reveal_library_path(project, path);
                continue;
            }
            self.expanded.extend(
                path.ancestors().skip(1).take_while(|folder| folder.starts_with(project)).map(Path::to_path_buf),
            );
        }
        self.selection.select_all_of(paths);
        self.reveal_lead = true;
        Ok(())
    }

    fn select_in_tree(&mut self, project: &Path, path: &Path) {
        if self.reveal_library_path(project, path) {
            self.selection.select_all_of(vec![path.to_owned()]);
            self.reveal_lead = true;
            return;
        }
        if !path.starts_with(project) {
            return;
        }
        self.expanded
            .extend(path.ancestors().skip(1).take_while(|folder| folder.starts_with(project)).map(Path::to_path_buf));
        self.selection.select_all_of(vec![path.to_owned()]);
        self.reveal_lead = true;
    }

    fn header_action_shown(&self, action: &str) -> bool {
        action != "locate" || !self.always_select_opened
    }

    fn select_opened_file(&mut self, project: &Path, file: Option<&Path>) -> Result<(), String> {
        let file = file.ok_or("Select Opened File needs a file open in the editor")?;
        if !zen_workbench::inside_project(project, file)
            && !self.libraries.as_ref().is_some_and(|libraries| libraries.library_of(project, file).is_some())
        {
            return Err(format!("{} is not a file of the project or its libraries", file.display()));
        }
        self.select_in_tree(project, file);
        Ok(())
    }

    fn set_always_select_opened(&mut self, on: bool) {
        if on && !self.always_select_opened {
            self.followed_file = None;
        }
        self.always_select_opened = on;
    }

    fn expand_all(&mut self, project: &Path) {
        const LIMIT: usize = 3000;
        let mut queue = vec![project.to_owned()];
        let mut count = 0;
        while let Some(folder) = queue.pop() {
            if count >= LIMIT {
                break;
            }
            count += 1;
            self.expanded.insert(folder.clone());
            let Ok(entries) = std::fs::read_dir(&folder) else { continue };
            for entry in entries.filter_map(Result::ok) {
                let name = entry.file_name().to_string_lossy().into_owned();
                let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
                let excluded = self.tree_exclusions.1.excludes_entry(&entry.path())
                    || (is_dir && zen_vfs::excluded_folders::is_excluded(&entry.path()));
                if is_dir && !name.starts_with('.') && !excluded && !zen_vfs::is_ignored(&name) {
                    queue.push(entry.path());
                }
            }
        }
        self.listings.clear();
    }

    fn new_project_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let Some(dialog) = &mut self.new_project else { return };
        if self.settings.is_open() {
            dialog.settings_changed();
            return;
        }
        let agent = dialog.by_agent;
        let mut dispatch = |id: &str, params| dialog_dispatch(registry, session, agent, id, params);
        match dialog.show(ui, &p, &mut dispatch) {
            Some(new_project_dialog::Outcome::Created(path)) => {
                self.new_project = None;
                self.requests.push(Request::open_created(agent, path));
            }
            Some(new_project_dialog::Outcome::Module { path, registered, reload }) => {
                self.new_project = None;
                let answer = serde_json::json!({ "path": path, "registered": registered, "reload": reload });
                self.after_module(ui.ctx(), registry, session, agent, &answer);
            }
            Some(new_project_dialog::Outcome::Cancelled) => self.new_project = None,
            Some(new_project_dialog::Outcome::MorePlugins) => {
                self.settings.open_at(settings::PageId::Plugins);
                self.settings.show_plugins_tab("marketplace");
            }
            Some(new_project_dialog::Outcome::GeneratorSettings) => {
                self.settings.open_at(settings::PageId::SystemSettings);
            }
            None => {}
        }
    }

    fn told_to_person(
        &mut self,
        ctx: &egui::Context,
        agent: bool,
        answer: Result<serde_json::Value, String>,
    ) -> Result<serde_json::Value, agent::ActionError> {
        answer.map_err(|error| {
            if !agent {
                self.post_at(ctx, error.clone());
            }
            agent::ActionError::Refused(error)
        })
    }

    fn after_module(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        agent: bool,
        answer: &serde_json::Value,
    ) {
        let path = answer["path"].as_str().unwrap_or_default();
        let name = Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(file) = answer["registered"].as_str() else {
            self.post_at(ctx, t!("ide-new-module-no-build", name = name));
            return;
        };
        let message = if answer["changed"] == false {
            t!("ide-module-sources-listed", name = name.as_str(), file = file)
        } else {
            t!("ide-new-module-added", name = name.as_str(), file = file)
        };
        let reloaded = answer["reload"].as_str().map_or(Ok(serde_json::Value::Null), |reload| {
            dialog_dispatch(registry, session, agent, reload, serde_json::json!({}))
        });
        let message = match reloaded {
            Ok(_) => message,
            Err(error) => format!("{message} {error}"),
        };
        self.post_at(ctx, message);
    }

    fn clone_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let Some(dialog) = &mut self.clone else { return };
        let agent = dialog.by_agent;
        let mut dispatch = |id: &str, params| dialog_dispatch(registry, session, agent, id, params);
        match dialog.show(ui, &p, &mut dispatch) {
            Some(new_project_dialog::Outcome::Created(path)) => {
                self.clone = None;
                self.requests.push(Request::open_created(agent, path));
            }
            Some(_) => self.clone = None,
            None => {}
        }
    }

    fn ask_trust(&mut self, session: &Session) {
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
        if let Some(root) = root.filter(|root| !zen_workbench::trust::trusted(session, root)) {
            self.requests.push(Request::AskTrust(root));
        }
    }

    fn follow_trust(&mut self, registry: &Registry, session: &mut Session) {
        let Some(root) = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned) else { return };
        let safe = !zen_workbench::trust::trusted(session, &root);
        if self.safe_mode && !safe {
            let _ = registry.dispatch(session, "project.model.refresh", serde_json::json!({}));
        }
        self.safe_mode = safe;
    }

    fn safe_mode_banner(&mut self, ui: &mut Ui, session: &Session) {
        let p = self.palette;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::BANNER), Sense::hover());
        let band =
            Rect::from_min_max(pos2(rect.left() + 6.0, rect.top()), pos2(rect.right() - 6.0, rect.bottom() - 4.0));
        ui.painter().rect_filled(band, 6, p.banner);
        let y = band.center().y;
        let text = t!("ide-safe-mode-banner");
        let drawn = ui.painter().with_clip_rect(band.shrink(4.0)).text(
            pos2(band.left() + 14.0, y),
            Align2::LEFT_CENTER,
            &text,
            theme::regular(13.0),
            p.text,
        );
        accessibility::announce(ui, ui.scope_id().with("safe-mode-banner"), drawn, || text.clone());
        let mut x = drawn.right() + 14.0;
        if self.link(ui, &mut x, y, &t!("ide-safe-mode-trust"), "safe-mode-trust") {
            self.ask_trust(session);
        }
    }

    fn starters_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let Some(dialog) = &mut self.starters else { return };
        let agent = dialog.by_agent;
        let mut dispatch = |id: &str, params| dialog_dispatch(registry, session, agent, id, params);
        if dialog.show(ui, &p, &mut dispatch).is_some() {
            self.starters = None;
        }
    }

    pub(super) fn starters_action(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        agent: bool,
        action: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let mut dispatch = |id: &str, params| dialog_dispatch(registry, session, agent, id, params);
        if action == starters_dialog::OPEN {
            let text = |key: &str| params.get(key).and_then(serde_json::Value::as_str).map(str::to_owned);
            let mut dialog = starters_dialog::StartersDialog::new(text("module"), text("file").map(PathBuf::from));
            dialog.by_agent = agent;
            dialog.load(&mut dispatch);
            let state = dialog.state();
            self.starters = Some(dialog);
            return Ok(state);
        }
        let dialog = self.starters.as_mut().ok_or("Edit Starters is not open")?;
        match action {
            starters_dialog::SET => {
                let ticked =
                    params.get("ticked").and_then(serde_json::Value::as_array).ok_or("`ticked` is required")?;
                dialog.set(ticked.iter().filter_map(serde_json::Value::as_str).map(str::to_owned).collect());
            }
            starters_dialog::APPLY => {
                if dialog.apply(&mut dispatch) {
                    self.starters = None;
                    return Ok(serde_json::json!({ "applied": true }));
                }
                return Err(dialog.state()["error"].as_str().unwrap_or_default().to_owned());
            }
            _ => {
                self.starters = None;
                return Ok(serde_json::json!({ "closed": true }));
            }
        }
        dialog.load(&mut dispatch);
        Ok(dialog.state())
    }

    fn open_folder_dialog(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut picker = rfd::FileDialog::new().set_title(t!("ide-open-title"));
            let directory = self.overview.settings.default_project_directory.trim();
            if Path::new(directory).is_dir() {
                picker = picker.set_directory(directory);
            }
            if let Some(folder) = picker.pick_folder() {
                self.requests.push(Request::Open(folder));
            }
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session, project: &Path) {
        let p = self.palette;
        let rect = ui.max_rect();
        let title_bar = appearance::title_strip(ui, rect, ui.scope_id().with("toolbar-drag"));
        accessibility::name_with(&title_bar, egui::Role::TitleBar, || app::window_title(project, self.product.name));
        project_glow(
            ui.painter(),
            rect,
            &p,
            p.project(&project.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()),
        );
        let y = rect.center().y;
        let full_screen = ui.ctx().input(|input| input.viewport().fullscreen.unwrap_or(false));
        let mut x = rect.left() + if full_screen { 12.0 } else { metrics::WINDOW_BUTTONS };
        let mark = Rect::from_min_size(pos2(x, y - 10.0), Vec2::splat(20.0));
        crate::icons::product_mark(ui, mark, self.product.id, self.product.name, false);
        x = mark.right() + 8.0;
        title_bar.context_menu(|ui| {
            if ui.button(t!("ide-toolbar-add-action")).clicked() {
                self.settings.open_customize(Some(customization::MAIN_TOOLBAR), true);
                ui.close();
            }
            if ui.button(t!("ide-toolbar-customize")).clicked() {
                self.settings.open_customize(Some(customization::MAIN_TOOLBAR), false);
                ui.close();
            }
        });
        let custom = self.overview.settings.customization.clone();
        for (index, action) in custom.items(customization::MAIN_TOOLBAR).iter().enumerate() {
            if action == customization::SEPARATOR {
                let line = pos2(x + 2.0, y);
                ui.painter().vline(
                    line.x,
                    (y - 9.0)..=(y + 9.0),
                    Stroke::new(1.0, p.text_secondary.gamma_multiply(0.4)),
                );
                x += 8.0;
                continue;
            }
            if !customization::is_known(action) {
                continue;
            }
            let (glyph, name) = match action.as_str() {
                customization::BACK => (Some(glyph_back as Glyph), t!("ide-action-back")),
                customization::FORWARD => (Some(glyph_forward as Glyph), t!("ide-action-forward")),
                other => (None, keymap_page::action_title(other)),
            };
            let glyph = custom.glyph(action).or(glyph);
            let command = registry.info(action).is_some();
            let button = Rect::from_center_size(pos2(x + 12.0, y), Vec2::splat(26.0));
            let available = !command || registry.is_available(session, action);
            let sense = if available { Sense::click() } else { Sense::hover() };
            let response = ui.interact(button, ui.scope_id().with(("nav", index)), sense);
            if available {
                hover(ui, &response, &p);
            }
            hint(ui, &response, &name, crate::keymap::shortcut(action).as_deref(), Side::Below);
            accessibility::button(&response, &name);
            if !available {
                accessibility::disabled(&response);
            }
            if response.clicked() {
                if command {
                    self.dispatch(ui, registry, session, action, serde_json::Value::Null);
                } else {
                    self.run_action(ui, registry, session, action, serde_json::json!({}));
                }
            }
            let color = if available { p.text_secondary } else { p.text_secondary.gamma_multiply(0.4) };
            match glyph {
                Some(icon) => icon(ui.painter(), button.shrink(7.0), color),
                None => {
                    let letter = name.chars().next().map(String::from).unwrap_or_default();
                    ui.painter().text(button.center(), Align2::CENTER_CENTER, letter, theme::semibold(13.0), color);
                }
            }
            x += 28.0;
        }
        x += 8.0;

        let name = project
            .file_name()
            .map_or_else(|| project.display().to_string(), |name| name.to_string_lossy().into_owned());
        let name_galley = ui.painter().layout_no_wrap(name.clone(), theme::semibold(13.0), p.text);
        let widget = Rect::from_min_size(pos2(x, y - 14.0), vec2(name_galley.size().x + 58.0, 28.0));
        let response = ui.interact(widget, ui.scope_id().with("project-widget"), Sense::click());
        accessibility::button(&response, &name);
        hover(ui, &response, &p);
        let badge = Rect::from_min_size(pos2(widget.left() + 6.0, y - 10.0), Vec2::splat(20.0));
        project_badge(ui, &p, badge, &name);
        ui.painter().galley(pos2(badge.right() + 8.0, y - name_galley.size().y / 2.0), name_galley, p.text);
        crate::icons::chevron_down(
            ui.painter(),
            Rect::from_center_size(pos2(widget.right() - 12.0, y), Vec2::splat(11.0)),
            p.text_secondary,
        );
        if response.clicked() {
            self.projects.toggle();
        }
        self.projects_popup(ui, widget, project);
        x = widget.right() + 6.0;

        self.as_git(|view| view.vcs_widget(ui, registry, session, x, y));

        let mut right = rect.right() - 10.0;
        let icons: [(Glyph, Color32, &str, &str); 3] = [
            (glyph_settings, p.text_secondary, "settings-title", "view.settings.open"),
            (glyph_search, p.text_secondary, "ide-action-search", "⇧⇧"),
            (glyph_more, p.text_secondary, "ide-action-more", ""),
        ];
        for (index, (icon, color, name, shortcut)) in icons.into_iter().enumerate() {
            let button = Rect::from_center_size(pos2(right - 14.0, y), Vec2::splat(28.0));
            let response = ui.interact(button, ui.scope_id().with(("toolbar-right", index)), Sense::click());
            hover(ui, &response, &p);
            if index == 0 {
                hint(ui, &response, &t!(name), crate::keymap::shortcut(shortcut).as_deref(), Side::Below);
                if response.clicked() {
                    self.settings.open();
                }
            } else if index == 1 {
                hint(ui, &response, &t!(name), Some(shortcut), Side::Below);
                if response.clicked() {
                    self.search_everywhere.toggle(search_everywhere::Tab::ALL);
                }
            } else if in_development(ui, &response, &t!(name), Side::Below) {
                self.post(ui, t!("ide-in-development-notice"));
            }
            icon(ui.painter(), button.shrink(7.0), color);
            right -= if index == 1 { 42.0 } else { 30.0 };
        }
        self.run_widget(ui, registry, session, right, y);
    }

    fn follow_terminal_keyboard(&mut self, ctx: &egui::Context) {
        let typing = self.terminal_has_keyboard(ctx);
        if typing && !self.terminal_keyboard {
            self.requests.push(Request::Autosave);
        }
        self.terminal_keyboard = typing;
    }

    fn terminal_has_keyboard(&self, ctx: &egui::Context) -> bool {
        let focused = ctx.memory(egui::Memory::focused);
        self.tool_visible("terminal")
            && focused.is_some_and(|focused| self.terminal_drawn.iter().any(|(_, widget, _)| *widget == focused))
    }

    fn set_theme(&mut self, theme: &ResolvedTheme) {
        self.palette = IdePalette::from_theme(theme);
        self.image_textures.clear();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.terminal_palette = crate::terminal::TerminalPalette::from_theme(theme);
        }
    }

    fn project_tabs(&mut self, ui: &mut Ui) {
        let p = self.palette;
        let rect = ui.max_rect().shrink2(vec2(metrics::GAP + 2.0, 3.0));
        let tabs = self.overview.tabs.clone();
        let width = rect.width() / tabs.len() as f32;
        let count = tabs.len();
        let starts: Vec<f32> = (0..count).map(|index| index as f32 * width).collect();
        let widths = vec![width; count];
        let pointer = ui.input(|input| input.pointer.interact_pos());
        let in_row = pointer.filter(|at| rect.expand(8.0).contains(*at));
        let keys: Vec<u64> = tabs.iter().map(tab_drag::key).collect();
        let scope = ui.scope_id().with("project-tab-glide");
        let held = self
            .project_drag
            .as_ref()
            .and_then(|(path, grab)| tabs.iter().position(|tab| tab == path).map(|at| (at, *grab)));
        let followed = held.zip(in_row);
        let targets = match followed {
            Some(((at, grab), pointer)) => {
                let x = pointer.x - rect.left();
                let place = tab_drag::drop_place(&starts, &widths, at, x, &[], self.project_place);
                self.project_place = Some(place);
                if ui.input(|input| input.pointer.primary_released()) {
                    let settled = tab_drag::drag_targets(&starts, &widths, 0.0, at, place, None);
                    tab_drag::begin_settle(ui, scope, &keys, &settled);
                }
                tab_drag::drag_targets(&starts, &widths, 0.0, at, place, Some(x - grab))
            }
            None => tab_drag::settled(ui, scope, &keys).unwrap_or_else(|| starts.clone()),
        };
        if !ui.input(|input| input.pointer.primary_down()) {
            self.project_drag = None;
        }
        let follows = followed.map(|((at, _), _)| at);
        let order: Vec<usize> = (0..count).filter(|index| Some(*index) != follows).chain(follows).collect();
        for index in order {
            let path = &tabs[index];
            let id = scope.with(keys[index]);
            let left = tab_drag::glide(ui, id, targets[index], follows == Some(index));
            let lift = tab_drag::lift(ui, id.with("lift"), follows == Some(index));
            let tab = Rect::from_min_size(pos2(rect.left() + left, rect.top()), vec2(width, rect.height()))
                .shrink2(vec2(2.0, 0.0));
            tab_drag::paint_lift(ui.painter(), tab, p.popup_shadow, lift);
            if lift > 0.0 {
                ui.painter().rect_filled(tab, 6, p.island.gamma_multiply(lift));
            }
            let response = ui.interact(tab, ui.scope_id().with(("project-tab", path)), Sense::click_and_drag());
            if response.drag_started() {
                let grab = response.interact_pointer_pos().map_or(0.0, |at| at.x - (rect.left() + left));
                self.project_drag = Some((path.clone(), grab));
                self.project_place = None;
            }
            if response.drag_stopped() {
                if response.interact_pointer_pos().is_some_and(|at| rect.expand(8.0).contains(at)) {
                    let to = self.project_place.unwrap_or(index);
                    if to != index {
                        self.requests.push(Request::MoveTab(path.clone(), to));
                    }
                }
                self.project_drag = None;
            }
            let active = self.overview.active_tab.as_ref() == Some(path);
            accessibility::name_with(&response, egui::Role::Tab, || project_name(path));
            accessibility::edit(&response, |node| node.set_selected(active));
            if active {
                ui.painter().rect_filled(tab, 6, p.island);
            } else if response.hovered() {
                ui.painter().rect_filled(tab, 6, p.hover);
            }
            let name =
                path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
            let color = if active { p.text } else { p.text_secondary };
            let galley = ui.painter().layout_no_wrap(name.clone(), theme::regular(12.5), color);
            let badge = Rect::from_center_size(
                pos2(tab.center().x - galley.size().x / 2.0 - 12.0, tab.center().y),
                Vec2::splat(16.0),
            );
            project_badge(ui, &p, badge, &name);
            ui.painter().galley(pos2(badge.right() + 6.0, tab.center().y - galley.size().y / 2.0), galley, color);
            let mut closed = false;
            let pointer_in = ui.input(|input| input.pointer.hover_pos()).is_some_and(|pointer| tab.contains(pointer));
            if pointer_in || active {
                let cross = Rect::from_center_size(pos2(tab.left() + 14.0, tab.center().y), Vec2::splat(16.0));
                let cross_response =
                    ui.interact(cross, ui.scope_id().with(("project-tab-close", path)), Sense::click());
                accessibility::button_with(&cross_response, || t!("a11y-close-tab", name = project_name(path)));
                let c = cross.shrink(4.5);
                let color = if cross_response.hovered() { p.text } else { p.text_secondary };
                ui.painter().line_segment([c.left_top(), c.right_bottom()], Stroke::new(1.2, color));
                ui.painter().line_segment([c.right_top(), c.left_bottom()], Stroke::new(1.2, color));
                if cross_response.clicked() {
                    closed = true;
                    self.requests.push(Request::Close(path.clone()));
                }
            }
            if response.clicked() && !closed && !active {
                self.requests.push(Request::Focus(path.clone()));
            }
        }
    }

    fn project_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session, project: &Path) {
        self.group_bundles = zen_workbench::resource_bundles::grouped_in_tree(session);
        let p = self.palette;
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        self.project_header = Some(header);
        let mut right = header.right() - 8.0;
        let actions: [(Glyph, &str, &str); 6] = [
            (glyph_minus, "hide", "ide-project-hide"),
            (glyph_more, "more", "ide-project-options"),
            (glyph_collapse, "collapse", "ide-project-collapse-all"),
            (glyph_expand, "expand", "ide-project-expand-all"),
            (glyph_locate, "locate", "ide-project-select-opened"),
            (glyph_plus_menu, "new", "ide-project-new"),
        ];
        let active_file =
            session.state::<Workbench>().and_then(Workbench::active_document).map(|d| d.path().to_owned());
        let actions: Vec<_> = actions.into_iter().filter(|(_, action, _)| self.header_action_shown(action)).collect();
        for (icon, action, name) in actions {
            if action == "new" {
                ui.painter().vline(
                    right - 3.0,
                    header.center().y - 8.0..=header.center().y + 8.0,
                    Stroke::new(1.0, p.hover),
                );
                right -= 8.0;
            }
            let button = Rect::from_center_size(pos2(right - 12.0, header.center().y), Vec2::splat(24.0));
            let response = ui.interact(button, ui.scope_id().with(("project-action", action)), Sense::click());
            accessibility::button_with(&response, || t!(name));
            hover(ui, &response, &p);
            icon(ui.painter(), button.shrink(6.0), p.text_secondary);
            if action != "more" && action != "new" {
                hint(ui, &response, &t!(name), None, Side::Below);
            }
            if response.clicked() {
                match action {
                    "hide" => self.hide_tool("project"),
                    "collapse" => {
                        self.expanded.clear();
                        self.expanded.insert(project.to_owned());
                    }
                    "expand" => self.expand_all(project),
                    "locate" => {
                        let _ = self.select_opened_file(project, active_file.as_deref());
                    }
                    _ => {}
                }
            }
            match action {
                "new" if response.clicked() => {
                    let folder = self
                        .selection
                        .items(&self.visible_rows)
                        .last()
                        .filter(|path| !self.is_library_path(project, path))
                        .map(|path| folder_of(path))
                        .unwrap_or_else(|| project.to_owned());
                    if let Err(error) = self.open_new_popup(session, Some(folder), Some(response.rect.left_bottom())) {
                        self.post(ui, error);
                    }
                }
                "more" => {
                    egui::Popup::menu(&response).show(|ui| {
                        ui.menu_button(t!("ide-project-behavior"), |ui| {
                            let mut always = self.always_select_opened;
                            if ui.checkbox(&mut always, t!("ide-project-always-select-opened")).clicked() {
                                self.set_always_select_opened(always);
                            }
                        });
                        ui.checkbox(&mut self.show_excluded, t!("ide-project-excluded-files"));
                        ui.separator();
                        self.tool_window_menu(ui, "project", false);
                    });
                }
                _ => {}
            }
            right -= 26.0;
        }
        let room = Rect::from_min_max(header.min, pos2((right + 2.0).max(header.left()), header.bottom()));
        let title = ui.painter().with_clip_rect(room.intersect(ui.clip_rect())).text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            t!("ide-project-title"),
            theme::semibold(13.0),
            p.text,
        );
        accessibility::heading(ui, ui.scope_id().with("project-title"), title, || t!("ide-project-title"));
        let chevron = Rect::from_center_size(pos2(title.right() + 9.0, header.center().y), Vec2::splat(11.0));
        if chevron.right() <= room.right() {
            crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
        }
        if self.always_select_opened && active_file != self.followed_file {
            if let Some(path) = &active_file {
                self.select_in_tree(project, path);
            }
            self.followed_file = active_file.clone();
        }

        if !self.expanded.contains(project) {
            self.expanded.insert(project.to_owned());
        }
        let revision = session.state::<Workbench>().map_or(0, Workbench::revision);
        let revision_changed = revision != self.seen_revision;
        let file_types = zen_vfs::file_types::generation();
        if file_types != self.file_types_seen {
            self.file_types_seen = file_types;
            self.listings.clear();
        }
        if revision_changed {
            self.seen_revision = revision;
            self.listings.clear();
            self.libraries = None;
        }
        let settings = session.state::<zen_workbench::project_settings::ProjectSettingsState>();
        let settings_revision = settings.map_or(0, zen_workbench::project_settings::ProjectSettingsState::revision);
        if let Some(settings) =
            settings.map(|state| state.settings()).filter(|_| settings_revision != self.tree_exclusions.0)
        {
            let exclusions = zen_index::Exclusions::new(&settings.excluded, &settings.exclude_patterns);
            self.tree_exclusions = (settings_revision, exclusions);
            self.tree_resource_roots = settings.resource_roots().map(Path::to_owned).collect();
        }
        self.refresh_libraries(ui.ctx(), registry, session, project);
        let active = session
            .state::<Workbench>()
            .and_then(|workbench| workbench.active_document().map(|document| document.path().to_owned()));
        let active = active.filter(|_| self.selection.is_empty());
        if revision_changed {
            self.selection.retain(|path| path.exists());
        }
        self.scratch_folders = session
            .state::<zen_workbench::scratch::Scratches>()
            .map(zen_workbench::scratch::Scratches::folders)
            .unwrap_or_default();
        let mut frame = TreeFrame::default();
        let tree = egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let root = zen_vfs::Entry {
                is_symlink: false,
                path: project.to_owned(),
                name: project.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
                is_dir: true,
            };
            self.tree_row(ui, &root, 0, active.as_deref(), &mut frame, libraries_tree::RowMode::default());
            self.libraries_rows(ui, project, active.as_deref(), &mut frame);
            if self.overview.settings.project_view_scratches {
                self.scratch_rows(ui, project, active.as_deref(), &mut frame);
            }
        });
        self.visible_rows = std::mem::take(&mut frame.visible);
        self.reveal_lead = false;
        let tree_rect = tree.inner_rect;

        if ui.input(|input| input.pointer.any_pressed()) {
            let pointer = ui.input(|input| input.pointer.interact_pos());
            self.tree_focused = pointer.is_some_and(|pointer| tree_rect.contains(pointer));
        }
        let libraries = self.libraries.clone().unwrap_or_default();
        let target = |lead: Option<&PathBuf>| {
            lead.filter(|path| !libraries.is_read_only(project, path))
                .map_or_else(|| project.to_owned(), |path| folder_of(path))
        };

        if self.tree_focused && self.new_entry.is_none() && self.delete_entry.is_none() && !self.new_popup_open() {
            self.tree_keys(ui, project, &mut frame);
        }

        if self.tree_focused && self.new_entry.is_none() {
            let events = ui.input(|input| input.events.clone());
            let mut paste_text = None;
            let mut paste_key = false;
            let mut pasted_image = None;
            let root = self.visible_rows.first().cloned();
            let selected = |self_: &Self, movable: bool| {
                let mut paths = selection::outermost(&self_.selection.items(&self_.visible_rows));
                paths.retain(|path| !libraries_tree::is_group(path));
                if movable {
                    paths.retain(|path| Some(path) != root.as_ref() && !self_.is_library_path(project, path));
                }
                paths
            };
            for event in events {
                match event {
                    egui::Event::Copy => frame.action = Some(TreeAction::Copy(selected(self, false))),
                    egui::Event::Cut => frame.action = Some(TreeAction::Cut(selected(self, true))),
                    egui::Event::Key { key: Key::C, pressed: true, modifiers, .. }
                        if modifiers.command && !modifiers.shift && !modifiers.alt =>
                    {
                        frame.action = Some(TreeAction::Copy(selected(self, false)))
                    }
                    egui::Event::Key { key: Key::X, pressed: true, modifiers, .. }
                        if modifiers.command && !modifiers.shift && !modifiers.alt =>
                    {
                        frame.action = Some(TreeAction::Cut(selected(self, true)))
                    }
                    egui::Event::Key { key: Key::C, pressed: true, modifiers, .. }
                        if modifiers.command && modifiers.shift && !modifiers.alt =>
                    {
                        frame.action = Some(TreeAction::CopyPath(selected(self, false)))
                    }
                    egui::Event::Paste(text) => paste_text = Some(text),
                    egui::Event::PasteImage(image) => {
                        pasted_image = Some(ClipboardImage {
                            width: image.size[0],
                            height: image.size[1],
                            rgba: image.pixels.iter().flat_map(|pixel| pixel.to_srgba_unmultiplied()).collect(),
                        });
                    }
                    egui::Event::Key { key: Key::V, pressed: true, modifiers, .. }
                        if modifiers.command && !modifiers.shift && !modifiers.alt =>
                    {
                        paste_key = true
                    }
                    _ => {}
                }
            }
            if paste_text.is_some() || paste_key || pasted_image.is_some() {
                let directory = target(self.selection.lead());
                let has_files = session.state::<FileClipboard>().is_some_and(FileClipboard::can_paste);
                let text_paths: Vec<PathBuf> = paste_text
                    .unwrap_or_default()
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute() && path.exists())
                    .collect();
                if has_files {
                    self.paste(ui, registry, session, None, directory);
                } else if !text_paths.is_empty() {
                    self.paste(ui, registry, session, Some(text_paths), directory);
                } else if let Some(image) = pasted_image.or_else(clipboard_image) {
                    let name = free_image_name(&directory, "image.png");
                    self.new_entry = Some(NewEntry {
                        package: false,
                        directory,
                        rename: None,
                        folder: false,
                        name,
                        image: Some(image),
                        focused: false,
                        preview: None,
                    });
                } else {
                    self.post(ui, t!("ide-paste-nothing"));
                }
            }
        }

        let over_tree = ui.input(|input| input.pointer.hover_pos()).is_some_and(|pointer| tree_rect.contains(pointer));
        let dropped: Vec<PathBuf> = ui.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_owned())
                .filter(|path| !path.as_os_str().is_empty())
                .filter(|path| over_tree || !commit_window::is_patch_file(path))
                .collect()
        });
        let diff_takes_drops = self.diff_in_front(session) && self.diff.as_ref().is_some_and(diff_view::DiffTab::typed);
        let over_tree =
            ui.input(|input| input.pointer.hover_pos()).is_some_and(|pointer| ui.max_rect().contains(pointer));
        if !dropped.is_empty() && (!diff_takes_drops || over_tree) {
            let directory = frame.hovered_folder.clone().unwrap_or_else(|| target(self.selection.lead()));
            self.paste(ui, registry, session, Some(dropped), directory);
        }

        match frame.action.take().or_else(|| self.pending_tree_action.take()) {
            Some(TreeAction::NewFile(directory)) => {
                self.new_entry = Some(NewEntry {
                    package: false,
                    directory,
                    folder: false,
                    name: String::new(),
                    image: None,
                    focused: false,
                    preview: None,
                    rename: None,
                })
            }
            Some(TreeAction::NewDirectory(directory)) => {
                self.new_entry = Some(NewEntry {
                    package: false,
                    directory,
                    folder: true,
                    name: String::new(),
                    image: None,
                    focused: false,
                    preview: None,
                    rename: None,
                })
            }
            Some(TreeAction::NewPackage(directory)) => {
                self.new_entry = Some(NewEntry {
                    package: true,
                    directory,
                    folder: true,
                    name: String::new(),
                    image: None,
                    focused: false,
                    preview: None,
                    rename: None,
                })
            }
            Some(TreeAction::Paste(directory)) => self.paste(ui, registry, session, None, directory),
            Some(TreeAction::New(directory, choice)) => self.choose_new(ui, registry, session, directory, choice),
            Some(action) => self.tree_action(ui, registry, session, action),
            None => {}
        }
        self.tree_menu = frame.menu_for.take().map(|path| {
            let new_items = self.cached_new_items(session, &folder_of(&path));
            TreeMenuInfo { new_items, ..self.tree_menu_info(session, path) }
        });
        self.new_entry_dialog(ui, registry, session);
        self.delete_dialog(ui, registry, session);
        if let Some(path) = frame.preview_file.filter(|path| !zen_workbench::database::files::is_database_file(path)) {
            let _ =
                registry.dispatch(session, "file.document.open", serde_json::json!({ "path": path, "preview": true }));
        }
        if let Some(path) = frame.open_file.as_deref().and_then(bundle_view::bundle_at) {
            if let Err(error) = self.open_bundle(registry, session, &path) {
                self.post(ui, error);
            }
        } else if let Some(path) = frame.open_file
            && !self.open_database_file(registry, session, &path)
        {
            self.dispatch(ui, registry, session, "file.document.open", serde_json::json!({ "path": path }));
        }
    }

    fn tree_menu_info(&self, session: &Session, path: PathBuf) -> TreeMenuInfo {
        if let Some(info) = self.tree_menu.as_ref().filter(|info| info.path == path) {
            return info.clone();
        }
        let workbench = session.state::<Workbench>();
        TreeMenuInfo {
            can_paste: session.state::<FileClipboard>().is_some_and(FileClipboard::can_paste),
            formatter: path.is_file()
                && (zen_workbench::sql_format::is_sql(&path)
                    || session
                        .state::<zen_workbench::language::Languages>()
                        .is_some_and(|languages| languages.serves(&path))),
            editor: workbench
                .and_then(Workbench::active_document)
                .filter(|document| document.image().is_none())
                .map(|document| document.path().to_owned()),
            folder: self.folder_mark(session, &path),
            resource_root: self.tree_resource_roots.contains(&path),
            types: override_types(),
            new_items: Vec::new(),
            tools: session
                .state::<zen_workbench::external_tools::ExternalTools>()
                .map(zen_workbench::external_tools::ExternalTools::tools)
                .unwrap_or_default(),
            scratch_run: path.is_file().then(|| zen_workbench::run::scratch_run_name(session, &path)).flatten(),
            run_targets: zen_workbench::run::plugin_targets_at(session, &path),
            path,
        }
    }

    fn folder_mark(&self, session: &Session, path: &Path) -> FolderMark {
        let Some(root) = session.state::<Workbench>().and_then(Workbench::project) else { return FolderMark::Unknown };
        let settings = session.state::<zen_workbench::project_settings::ProjectSettingsState>();
        if settings.is_some_and(|state| state.settings().excluded.iter().any(|folder| folder == path)) {
            return FolderMark::Excluded;
        }
        let elsewhere = self.tree_exclusions.1.excludes(root, path) || zen_vfs::excluded_folders::excludes(root, path);
        if elsewhere { FolderMark::ExcludedOtherwise } else { FolderMark::Excludable }
    }

    fn tree_action(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session, action: TreeAction) {
        use serde_json::json;
        match action {
            TreeAction::New(..)
            | TreeAction::NewFile(_)
            | TreeAction::NewDirectory(_)
            | TreeAction::NewPackage(_)
            | TreeAction::Paste(_) => {}
            TreeAction::Copy(paths) | TreeAction::Cut(paths) | TreeAction::CopyPath(paths) if paths.is_empty() => {}
            TreeAction::Copy(paths) => {
                self.dispatch(ui, registry, session, "project.file.copy", json!({ "paths": paths }))
            }
            TreeAction::Cut(paths) => {
                self.dispatch(ui, registry, session, "project.file.cut", json!({ "paths": paths }))
            }
            TreeAction::CopyPath(paths) => {
                self.dispatch(ui, registry, session, "project.file.copyPath", json!({ "paths": paths }))
            }
            TreeAction::Delete(paths) => self.delete_entry = Some(paths),
            TreeAction::Rename(path) => self.open_rename_entry(path),
            TreeAction::Reformat(path) => {
                self.dispatch(ui, registry, session, "file.document.open", json!({ "path": path }));
                if let Err(error) = registry.dispatch_unrecorded(
                    session,
                    "editor.code.reformat",
                    json!({ "path": path, "wholeFile": true }),
                ) {
                    self.report(ui, error);
                }
            }
            TreeAction::OpenRightSplit(path) => {
                self.dispatch(ui, registry, session, "editor.split.openRight", json!({ "path": path }));
            }
            TreeAction::Reveal(path) => self.dispatch(ui, registry, session, "file.fs.reveal", json!({ "path": path })),
            TreeAction::OpenInTerminal(path) => {
                #[cfg(not(target_arch = "wasm32"))]
                self.dispatch(ui, registry, session, "terminal.session.new", json!({ "cwd": folder_of(&path) }));
                #[cfg(target_arch = "wasm32")]
                let _ = path;
            }
            TreeAction::ShowHistory(path) => self.local_history.open(path),
            TreeAction::RepairFile(path) => {
                self.dispatch(ui, registry, session, "ide.repair.file", json!({ "path": path }));
            }
            TreeAction::PutLabel => self.local_history.open_label(),
            TreeAction::VcsHistory(path) => match self.git_show_history(registry, session, &path) {
                Ok(()) => self.show_tool("git"),
                Err(message) => self.post(ui, message),
            },
            TreeAction::Annotate(path) => {
                self.dispatch(ui, registry, session, "file.document.open", json!({ "path": path }));
                self.dispatch(ui, registry, session, "vcs.blame.annotate", json!({ "path": path, "show": true }));
            }
            TreeAction::ShowDiff(path) => self.open_diff(ui.ctx(), registry, session, &path),
            TreeAction::Rollback(paths) => {
                self.refresh_changes(registry, session);
                let changed: Vec<PathBuf> = self
                    .commit_window
                    .changes()
                    .iter()
                    .filter(|change| paths.iter().any(|path| change.path.starts_with(path)))
                    .map(|change| change.path.clone())
                    .collect();
                if changed.is_empty() {
                    self.post(ui, t!("ide-tree-no-changes"));
                } else {
                    self.open_rollback(changed);
                }
            }
            TreeAction::GitAdd(paths) => {
                self.dispatch(ui, registry, session, "vcs.index.stage", json!({ "paths": paths }))
            }
            TreeAction::CommitFiles(paths) => self.open_commit_files(registry, session, &paths),
            TreeAction::Reload(paths) => {
                self.listings.clear();
                let open: Vec<PathBuf> = session
                    .state::<Workbench>()
                    .map(|workbench| {
                        workbench
                            .documents()
                            .iter()
                            .map(|document| document.path().to_owned())
                            .filter(|open| paths.iter().any(|path| open.starts_with(path)))
                            .collect()
                    })
                    .unwrap_or_default();
                if !open.is_empty() {
                    self.dispatch(ui, registry, session, "file.document.syncWithDisk", json!({ "paths": open }));
                }
            }
            TreeAction::CompareWith(path) => self.compare_with_dialog(ui, registry, session, path),
            TreeAction::CompareFolders(left, right) => self.show_folder_diff(ui.ctx(), session, left, right, false),
            TreeAction::CompareFiles(paths) => match self.compare_paths(registry, session, &paths) {
                Ok(_) => {}
                Err(agent::ActionError::Dispatch(error)) => self.report(ui, error),
                Err(agent::ActionError::Refused(message)) => self.post(ui, message),
            },
            TreeAction::ExternalTool(id, path) => self.run_chosen_tool(ui, registry, session, &id, Some(path)),
            TreeAction::CompareWithEditor(path) => {
                self.show_comparison(ui, registry, session, "file.compare.editor", json!({ "path": path }))
            }
            TreeAction::Ignore(paths) => {
                self.dispatch(ui, registry, session, "vcs.ignore.add", json!({ "paths": paths }))
            }
            TreeAction::MarkExcluded(path) => {
                self.dispatch(ui, registry, session, "project.directory.markExcluded", json!({ "path": path }))
            }
            TreeAction::CancelExclusion(path) => {
                self.dispatch(ui, registry, session, "project.directory.cancelExclusion", json!({ "path": path }))
            }
            TreeAction::MarkResourceRoot(path) => {
                self.dispatch(ui, registry, session, "project.directory.markResourceRoot", json!({ "path": path }))
            }
            TreeAction::UnmarkResourceRoot(path) => {
                self.dispatch(ui, registry, session, "project.directory.unmarkResourceRoot", json!({ "path": path }))
            }
            TreeAction::ToggleBookmarks(paths) | TreeAction::DeleteBookmarks(paths) if paths.is_empty() => {}
            TreeAction::ToggleBookmarks(paths) => {
                self.dispatch(ui, registry, session, "navigate.bookmark.toggleFiles", json!({ "paths": paths }))
            }
            TreeAction::BookmarkMnemonic(path) => self.bookmarks.open_mnemonic(registry, session, path, None),
            TreeAction::DeleteBookmarks(paths) => {
                self.dispatch(ui, registry, session, "navigate.bookmark.removeFiles", json!({ "paths": paths }))
            }
            TreeAction::ChangeLanguage(path) => {
                let target = scratch_popups::LanguageTarget::Change(path);
                if let Err(error) = self.scratch.open_languages(registry, session, target) {
                    self.post(ui, error);
                }
            }
            TreeAction::RunScratch(path) => {
                match registry.dispatch(session, "file.scratch.run", json!({ "path": path })) {
                    Ok(_) => self.show_tool("run"),
                    Err(error) => self.report(ui, error),
                }
            }
            TreeAction::RunTarget(path, line) => {
                let params = json!({ "context": { "path": path, "line": line + 1 } });
                if let Err(agent::ActionError::Dispatch(error)) = self.start_run(registry, session, params, false) {
                    self.report(ui, error);
                }
            }
            TreeAction::Move(path) => {
                if let Some(project) = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned) {
                    self.scratch.open_move(path, &project);
                }
            }
            TreeAction::Associate(path) => self.open_associate_dialog(path),
            TreeAction::OverrideType(paths, type_id) => self.dispatch(
                ui,
                registry,
                session,
                "editor.fileTypes.override",
                json!({ "paths": paths, "type": type_id }),
            ),
            TreeAction::RevertOverride(paths) => {
                self.dispatch(ui, registry, session, "editor.fileTypes.revertOverride", json!({ "paths": paths }))
            }
        }
    }

    fn refresh_tree_bookmarks(&mut self, registry: &Registry, session: &mut Session) {
        let revision = || session.state::<zen_workbench::bookmarks::Bookmarks>().map_or(0, |b| b.revision());
        if self.tree_bookmarks.0 == revision() {
            return;
        }
        let listed = registry.dispatch(session, "navigate.bookmark.list", serde_json::json!({})).unwrap_or_default();
        let marks = listed["bookmarks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|bookmark| bookmark["line"].is_null())
            .filter_map(|bookmark| {
                let path = PathBuf::from(bookmark["path"].as_str()?);
                Some((path, bookmark["mnemonic"].as_str().and_then(zen_workbench::bookmarks::mnemonic_of)))
            })
            .collect();
        let revision = session.state::<zen_workbench::bookmarks::Bookmarks>().map_or(0, |b| b.revision());
        self.tree_bookmarks = (revision, marks);
    }

    fn open_rename_entry(&mut self, path: PathBuf) {
        let Some(directory) = path.parent().map(Path::to_owned) else { return };
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        self.new_entry = Some(NewEntry {
            package: false,
            directory,
            rename: Some(path.clone()),
            folder: path.is_dir(),
            name,
            image: None,
            focused: false,
            preview: None,
        });
    }

    fn open_commit_files(&mut self, registry: &Registry, session: &mut Session, paths: &[PathBuf]) {
        self.refresh_changes(registry, session);
        self.commit_window.check_only(paths);
        self.open_commit();
        self.show_tool("commit");
    }

    fn compare_with_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session, path: PathBuf) {
        #[cfg(all(not(target_arch = "wasm32"), not(test)))]
        {
            let folder = folder_of(&path);
            if path.is_dir() {
                if let Some(other) = rfd::FileDialog::new().set_directory(&folder).pick_folder() {
                    self.show_folder_diff(ui.ctx(), session, path, other, false);
                }
                return;
            }
            if let Some(other) = rfd::FileDialog::new().set_directory(folder).pick_file() {
                let params = serde_json::json!({ "left": path, "right": other });
                self.show_comparison(ui, registry, session, "file.compare.files", params);
            }
        }
        #[cfg(any(target_arch = "wasm32", test))]
        let _ = (ui, registry, session, &path);
        #[cfg(test)]
        compare::ASKED_OTHER.with(|asked| asked.replace(Some(path)));
    }

    fn show_comparison(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        command: &str,
        params: serde_json::Value,
    ) {
        match registry.dispatch(session, command, params) {
            Ok(value) => {
                if let Ok(view) = serde_json::from_value(value) {
                    self.show_diff(session, view);
                }
            }
            Err(error) => self.report(ui, error),
        }
    }

    fn paste(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        sources: Option<Vec<PathBuf>>,
        directory: PathBuf,
    ) {
        if sources.as_ref().is_some_and(Vec::is_empty) {
            self.post(ui, t!("ide-paste-nothing"));
            return;
        }
        self.expanded.insert(directory.clone());
        match sources {
            Some(sources) => self.dispatch(
                ui,
                registry,
                session,
                "file.fs.copyInto",
                serde_json::json!({ "sources": sources, "directory": directory }),
            ),
            None => self.dispatch(
                ui,
                registry,
                session,
                "project.file.paste",
                serde_json::json!({ "directory": directory }),
            ),
        }
    }

    fn tree_keys(&mut self, ui: &Ui, project: &Path, frame: &mut TreeFrame) {
        let rows = self.visible_rows.clone();
        let keys: Vec<(Key, egui::Modifiers)> = ui.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                    _ => None,
                })
                .collect()
        });
        for (key, modifiers) in keys {
            let lead = self.selection.lead().cloned();
            match key {
                Key::ArrowUp | Key::ArrowDown => {
                    let step = if key == Key::ArrowUp { -1 } else { 1 };
                    self.selection.step(step, modifiers.shift, &rows);
                    self.reveal_lead = true;
                }
                Key::ArrowRight => {
                    if let Some(lead) = lead.filter(|lead| lead.is_dir() || libraries_tree::is_group(lead)) {
                        self.expanded.insert(lead);
                    }
                }
                Key::ArrowLeft => match lead {
                    Some(lead)
                        if (lead.is_dir() || libraries_tree::is_group(&lead))
                            && self.expanded.contains(&lead)
                            && lead != project =>
                    {
                        self.expanded.remove(&lead);
                    }
                    Some(lead) => {
                        if let Some(parent) = lead.parent().filter(|parent| rows.iter().any(|row| row == parent)) {
                            self.selection.select(parent.to_owned());
                            self.reveal_lead = true;
                        }
                    }
                    None => {}
                },
                Key::Enter if modifiers.shift => {
                    if let Some(lead) = lead.filter(|lead| lead.is_file()) {
                        frame.action = Some(TreeAction::OpenRightSplit(lead));
                    }
                }
                Key::D if modifiers.command && !modifiers.shift && !modifiers.alt => {
                    let files: Vec<PathBuf> =
                        self.selection.items(&rows).into_iter().filter(|path| path.is_file()).collect();
                    let folders: Vec<PathBuf> =
                        self.selection.items(&rows).into_iter().filter(|path| path.is_dir()).collect();
                    if (2..=3).contains(&files.len()) {
                        frame.action = Some(TreeAction::CompareFiles(files));
                    } else if let ([left, right], []) = (folders.as_slice(), files.as_slice()) {
                        frame.action = Some(TreeAction::CompareFolders(left.clone(), right.clone()));
                    } else if let Some(lead) = lead.filter(|lead| lead.is_file() || lead.is_dir()) {
                        frame.action = Some(TreeAction::CompareWith(lead));
                    }
                }
                Key::Enter => match lead {
                    Some(lead) if bundle_view::bundle_at(&lead).is_some() => frame.open_file = Some(lead),
                    Some(lead) if lead.is_dir() || libraries_tree::is_group(&lead) => {
                        if !self.expanded.remove(&lead) {
                            self.expanded.insert(lead);
                        }
                    }
                    Some(lead) if lead.is_file() => frame.open_file = Some(lead),
                    _ => {}
                },
                Key::A if modifiers.command => self.selection.select_all_of(rows.clone()),
                Key::Delete if modifiers.command => {}
                Key::Backspace | Key::Delete if !modifiers.shift && !modifiers.alt => {
                    let paths = self.deletable_selection_in(project);
                    if !paths.is_empty() {
                        self.delete_entry = Some(paths);
                    }
                }
                _ => {}
            }
        }
    }

    fn moves_scratch(&self, session: &Session, params: &serde_json::Value, agent: bool) -> bool {
        let given = params.get("path").and_then(serde_json::Value::as_str).map(PathBuf::from);
        let lead = (agent || self.tree_focused).then(|| self.selection.lead().cloned()).flatten();
        let scratches = session.state::<zen_workbench::scratch::Scratches>();
        given.or(lead).is_some_and(|path| scratches.is_some_and(|scratches| scratches.contains(&path)))
    }

    fn refactoring_place(
        &self,
        ctx: &egui::Context,
        session: &Session,
        params: &serde_json::Value,
        agent: bool,
        files: bool,
    ) -> Result<serde_json::Value, agent::ActionError> {
        use agent::ActionError::Refused;
        if let Some(context) = params.get("context") {
            return Ok(context.clone());
        }
        if files {
            if let Some(paths) = params.get("paths") {
                return Ok(serde_json::json!({ "paths": paths }));
            }
            if let Some(path) = params.get("path") {
                return Ok(serde_json::json!({ "paths": [path] }));
            }
            if !agent && self.tree_focused && self.project_open {
                let paths = self.deletable_selection();
                if paths.is_empty() {
                    return Err(Refused("select classes or a package of the project to move".to_owned()));
                }
                return Ok(serde_json::json!({ "paths": paths }));
            }
        }
        if !self.text_file_in_front(session) {
            return Err(Refused(format!(
                "the refactoring needs a text file in front in the editor{}; {} is there now (or pass context for \
                 refactor.dialog.form)",
                if files { " or classes selected in the Project tool window" } else { "" },
                self.front_kind(session)
            )));
        }
        if !agent && !self.editor_view.has_focus_in(ctx) {
            return Err(Refused(
                "the refactoring needs the editor or the Project tool window to have the keyboard".to_owned(),
            ));
        }
        session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .map(|document| serde_json::json!({ "path": document.path(), "offset": document.selections().primary().head }))
            .ok_or_else(|| Refused("no file is open".to_owned()))
    }

    fn open_refactoring_for(
        &mut self,
        session: &mut Session,
        action: &str,
        context: serde_json::Value,
    ) -> Result<serde_json::Value, agent::ActionError> {
        let providers =
            session.state_mut::<zen_workbench::language::refactoring_dialog::RefactoringProviders>().clone();
        let Some(provider) = providers.for_action(session, action, &context) else {
            return Err(agent::ActionError::Refused(format!(
                "no refactoring of the plug-ins answers {action} here (refactor.dialog.list names them with their action)"
            )));
        };
        let id = provider.id().to_owned();
        self.open_refactoring(id.clone(), context.clone());
        Ok(serde_json::json!({ "provider": id, "context": context }))
    }

    fn deletable_selection_in(&self, project: &Path) -> Vec<PathBuf> {
        selection::outermost(&self.selection.items(&self.visible_rows))
            .into_iter()
            .filter(|path| {
                path != project && !self.is_library_path(project, path) && !self.scratch_folders.contains(path)
            })
            .collect()
    }

    fn deletable_selection(&self) -> Vec<PathBuf> {
        match self.visible_rows.first().cloned() {
            Some(project) => self.deletable_selection_in(&project),
            None => Vec::new(),
        }
    }

    fn open_in_find_window(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        all: bool,
        new_tab: Option<bool>,
        wait_ms: Option<u64>,
    ) -> Result<serde_json::Value, agent::ActionError> {
        let Some(mut params) = self.find_in_files.search_params(registry, session) else {
            return Err(agent::ActionError::Refused(
                "Find in Files is not open with a query; open it with view.findInFiles.open and type one".to_owned(),
            ));
        };
        if all {
            params["limit"] = serde_json::json!(0);
        }
        params["newTab"] = serde_json::json!(new_tab.unwrap_or(self.find_in_files.new_tab()));
        if let Some(replacement) = self.find_in_files.replacement() {
            params["replacement"] = serde_json::json!(replacement);
        }
        if let Some(wait_ms) = wait_ms {
            params["waitMs"] = serde_json::json!(wait_ms);
        }
        let found =
            registry.dispatch(session, "search.text.occurrences", params).map_err(agent::ActionError::Dispatch)?;
        self.find_in_files.handed_over(registry, session);
        Ok(found)
    }

    fn tree_folder(&self, session: &Session) -> Option<String> {
        let project = session.state::<Workbench>()?.project()?.to_owned();
        let lead = self.selection.lead()?;
        let folder = if lead.is_dir() { lead.clone() } else { lead.parent()?.to_owned() };
        Some(folder.strip_prefix(&project).ok()?.to_string_lossy().into_owned())
    }

    fn front_document(session: &Session) -> (Option<usize>, u64) {
        session.state::<Workbench>().map_or((None, 0), |workbench| (workbench.active(), workbench.shown()))
    }

    pub(super) fn diff_in_front(&self, session: &Session) -> bool {
        self.is_in_front(FrontKind::Diff, session)
            || (self.diff_from_workspace
                && self.diff.is_some()
                && self.git_tab_in_front(session, zen_workbench::vcs::workspace::DIFF_TAB))
    }

    fn text_file_in_front(&self, session: &Session) -> bool {
        if let Some((_, tab)) = zen_workbench::workspaces::in_front(session) {
            let document = session.state::<Workbench>().and_then(Workbench::active_document);
            return tab.and_then(|tab| tab.path).is_some_and(|path| {
                document.is_some_and(|document| document.path() == path && document.image().is_none())
            });
        }
        !self.diff_in_front(session)
            && !self.folder_in_front(session)
            && !self.git_in_front(session)
            && !self.rebase_in_front(session)
            && !self.conflict_in_front(session)
            && session.state::<Workbench>().and_then(Workbench::active_document).is_some_and(|d| d.image().is_none())
    }

    fn front_kind(&self, session: &Session) -> String {
        if let Some((info, tab)) = zen_workbench::workspaces::in_front(session) {
            return match tab {
                Some(tab) if tab.path.is_none() => {
                    format!("the tab \"{}\" of the {} workspace", tab.title, info.title)
                }
                Some(tab) => format!("{} in the {} workspace, which is not a text file", tab.title, info.title),
                None => format!("the {} workspace with no tab open", info.title),
            };
        }
        if self.diff_in_front(session) {
            "a diff"
        } else if self.folder_in_front(session) {
            "Compare Directories"
        } else if self.git_in_front(session) {
            "the Git workspace"
        } else if self.rebase_in_front(session) {
            "the interactive rebase editor"
        } else if self.conflict_in_front(session) {
            "the conflict editor"
        } else if session.state::<Workbench>().and_then(Workbench::active_document).is_some() {
            "an image"
        } else {
            "no file"
        }
        .to_owned()
    }

    fn close_text_dialogs(&mut self) {
        self.goto_line = None;
        self.rename.close();
        self.paste_history.close();
    }

    fn bookmark_place(&self, session: &Session, params: &serde_json::Value) -> Result<(PathBuf, Option<u32>), String> {
        let given = params.get("path").and_then(serde_json::Value::as_str).map(PathBuf::from);
        let whole = params.get("whole").and_then(serde_json::Value::as_bool).unwrap_or_default();
        let active = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .filter(|document| whole || document.image().is_none());
        let path = given
            .or_else(|| active.map(|document| document.path().to_owned()))
            .ok_or_else(|| "a bookmark needs a text file in front in the editor, or `path` and `line`".to_owned())?;
        if whole {
            return Ok((path, None));
        }
        let line = match params.get("line").and_then(serde_json::Value::as_u64) {
            Some(0) => return Err("lines count from 1".to_owned()),
            Some(line) => u32::try_from(line - 1).map_err(|error| error.to_string())?,
            None => active
                .filter(|document| document.path() == path)
                .and_then(|document| document.snapshot().offset_to_point(document.selections().primary().head).ok())
                .and_then(|point| u32::try_from(point.line).ok())
                .ok_or_else(|| "give the `line` of the bookmark, from 1".to_owned())?,
        };
        Ok((path, Some(line)))
    }

    fn smart_step_popup(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let palette = self.palette;
        let polled = self.smart_step.poll(registry, session);
        let chosen = self.smart_step.show(ui, &palette);
        for (command, params) in polled.into_iter().chain(chosen) {
            self.dispatch(ui, registry, session, command, params);
        }
        self.editor_view.set_step_target(self.smart_step.highlight(session));
    }

    fn value_popups(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let palette = self.palette;
        let enabled = session.settings().get_or(zen_workbench::debug::VALUE_TOOLTIP, true);
        let delay = session
            .settings()
            .get_or(zen_workbench::debug::VALUE_TOOLTIP_DELAY, zen_workbench::debug::VALUE_TOOLTIP_DELAY_DEFAULT);
        self.evaluations.hover(ui, self.editor_view.hovered_value(ui.ctx()), (enabled, delay as f64 / 1000.0));
        for expression in self.evaluations.show(ui, registry, session, &palette) {
            let action = debug_window::DebugAction::AddWatch { expression };
            if let Err(message) = self.debug_window.perform(&action, registry, session) {
                self.post(ui, message);
            }
        }
    }

    fn breakpoint_place(&self, session: &Session, params: &serde_json::Value) -> Result<(PathBuf, u32), String> {
        let mut params = params.clone();
        if let Some(object) = params.as_object_mut() {
            object.remove("whole");
        }
        match self.bookmark_place(session, &params)? {
            (path, Some(line)) => Ok((path, line)),
            (_, None) => Err("give the `line` of the breakpoint, from 0".to_owned()),
        }
    }

    fn open_goto(&mut self) {
        let (line, column) = self.caret.unwrap_or((1, 1));
        self.goto_line = Some((format!("{line}:{column}"), false));
    }

    fn goto_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some((mut text, focused)) = self.goto_line.clone() else { return };
        let p = self.palette;
        let parsed = zen_workbench::editing::parse_line_column(&text);
        let (mut go, mut cancel) = (false, false);
        let mut now_focused = focused;
        let modal = egui::Modal::new(ui.scope_id().with("goto-line")).frame(dialog_frame(&p)).show(ui.ctx(), |ui| {
            ui.set_width(320.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(egui::RichText::new(t!("ide-goto-title")).font(theme::semibold(15.0)).color(p.text));
            ui.add_space(14.0);
            let label =
                ui.label(egui::RichText::new(t!("ide-goto-label")).font(theme::regular(12.5)).color(p.text_secondary));
            ui.add_space(6.0);
            let border = if parsed.is_none() && !text.trim().is_empty() { p.excluded } else { p.accent };
            let field = egui::Frame::new()
                .fill(p.text_field)
                .stroke(Stroke::new(1.5, border))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 7))
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut text)
                            .frame(egui::Frame::NONE)
                            .font(theme::regular(14.0))
                            .desired_width(f32::INFINITY),
                    )
                    .labelled_by(label.id)
                })
                .inner;
            if !now_focused {
                field.request_focus();
                now_focused = true;
            }
            if field.lost_focus() && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                if parsed.is_some() {
                    go = true;
                } else {
                    field.request_focus();
                }
            }
            ui.add_space(20.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let ok = egui::Button::new(
                    egui::RichText::new(t!("settings-ok")).font(theme::regular(13.0)).color(p.accent_text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(if parsed.is_some() { p.accent } else { p.accent.gamma_multiply(0.4) });
                if ui.add_enabled(parsed.is_some(), ok).clicked() {
                    go = true;
                }
                let back = egui::Button::new(
                    egui::RichText::new(t!("settings-cancel")).font(theme::regular(13.0)).color(p.text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, p.button_border));
                if ui.add(back).clicked() {
                    cancel = true;
                }
            });
        });
        if go && let Some((line, column)) = zen_workbench::editing::parse_line_column(&text) {
            self.goto_line = None;
            self.dispatch(
                ui,
                registry,
                session,
                "editor.caret.goTo",
                serde_json::json!({ "line": line, "column": column }),
            );
            self.editor_view.focus();
        } else if cancel || modal.should_close() {
            self.goto_line = None;
            self.editor_view.focus();
        } else {
            self.goto_line = Some((text, now_focused));
        }
    }

    fn ask_exit(&mut self, running: Vec<zen_workbench::exit::RunningWork>) {
        self.exit_prompt = Some(running);
    }

    fn certificate_dialog(&mut self, ui: &mut Ui) {
        let Some(untrusted) = zen_net::trust::pending().into_iter().next() else { return };
        if let Some(accept) = certificate_dialog::show(ui, &self.palette, &untrusted) {
            let _ = zen_workbench::network::certificates::answer(&untrusted.certificate.fingerprint, accept);
        }
    }

    fn exit_dialog(&mut self, ui: &mut Ui) {
        let Some(running) = &self.exit_prompt else { return };
        match exit_dialog::show(ui, &self.palette, exit_dialog::Ending::Exit, self.product.name, running) {
            Some(exit_dialog::Choice::Exit) => {
                self.exit_prompt = None;
                self.requests.push(Request::Exit { confirmed: true });
            }
            Some(exit_dialog::Choice::Cancel) => self.exit_prompt = None,
            None => {}
        }
    }

    fn ask_restart(
        &mut self,
        running: Vec<zen_workbench::exit::RunningWork>,
        invalidate: Option<zen_workbench::maintenance::InvalidateParams>,
    ) {
        self.restart_prompt = Some(RestartPrompt { running, invalidate });
    }

    fn restart_dialog(&mut self, ui: &mut Ui) {
        let Some(prompt) = &self.restart_prompt else { return };
        let ending = exit_dialog::Ending::Restart;
        match exit_dialog::show(ui, &self.palette, ending, self.product.name, &prompt.running) {
            Some(exit_dialog::Choice::Exit) => {
                let invalidate = prompt.invalidate;
                self.restart_prompt = None;
                self.requests.push(Request::RestartAsking(RestartAsk::Never, invalidate));
            }
            Some(exit_dialog::Choice::Cancel) => self.restart_prompt = None,
            None => {}
        }
    }

    fn delete_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(paths) = self.delete_entry.clone() else { return };
        let Some(path) = paths.first().cloned() else {
            self.delete_entry = None;
            return;
        };
        let p = self.palette;
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let folder = path.is_dir();
        let several = paths.len() > 1;
        let unsaved = session.state::<Workbench>().map_or(0, |workbench| {
            workbench
                .documents()
                .iter()
                .filter(|document| document.is_dirty() && paths.iter().any(|gone| document.path().starts_with(gone)))
                .count()
        });
        let (mut confirm, mut cancel) = (false, false);
        let modal = egui::Modal::new(ui.scope_id().with("delete-entry")).frame(dialog_frame(&p)).show(ui.ctx(), |ui| {
            ui.set_width(420.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.horizontal(|ui| {
                let (icon, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                if folder {
                    crate::file_icons::folder_icon(ui.painter(), icon, &name, false, false);
                } else {
                    crate::file_icons::file_icon(ui.painter(), icon, &name);
                }
                ui.add_space(8.0);
                ui.label(egui::RichText::new(t!("ide-delete-title")).font(theme::semibold(15.0)).color(p.text));
            });
            ui.add_space(14.0);
            let question = if several {
                t!("ide-delete-several-question", count = paths.len())
            } else if folder {
                t!("ide-delete-folder-question", name = name.as_str())
            } else {
                t!("ide-delete-file-question", name = name.as_str())
            };
            ui.add(egui::Label::new(egui::RichText::new(question).font(theme::regular(13.5)).color(p.text)).wrap());
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(t!("ide-delete-trash-note")).font(theme::regular(12.0)).color(p.text_secondary),
            );
            if unsaved > 0 {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(t!("ide-delete-unsaved", count = unsaved))
                        .font(theme::regular(12.0))
                        .color(p.excluded),
                );
            }
            ui.add_space(20.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let delete = egui::Button::new(
                    egui::RichText::new(t!("ide-delete-confirm")).font(theme::regular(13.0)).color(p.accent_text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(p.accent);
                if ui.add(delete).clicked() {
                    confirm = true;
                }
                let back = egui::Button::new(
                    egui::RichText::new(t!("settings-cancel")).font(theme::regular(13.0)).color(p.text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, p.button_border));
                if ui.add(back).clicked() {
                    cancel = true;
                }
            });
            if ui.input(|input| input.key_pressed(Key::Enter)) {
                confirm = true;
            }
        });
        if confirm {
            self.delete_entry = None;
            self.selection.retain(|selected| !paths.iter().any(|gone| selected.starts_with(gone)));
            self.dispatch(ui, registry, session, "file.fs.delete", serde_json::json!({ "paths": paths }));
        } else if cancel || modal.should_close() {
            self.delete_entry = None;
        }
    }

    fn new_entry_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(entry) = &mut self.new_entry else { return };
        let p = self.palette;
        let mut submit = false;
        let mut cancel = false;
        let name = entry.name.trim().to_owned();
        let same = entry.rename.as_ref().is_some_and(|old| {
            old.file_name().is_some_and(|old| old.to_string_lossy().to_lowercase() == name.to_lowercase())
        });
        let checked = if entry.package { name.replace('.', "/") } else { name.clone() };
        let hint = new_entry_hint(&entry.directory, &checked, same, entry.image.is_some());
        let taken = hint.is_some();
        let invalid = name.contains(['/', '\\']) && entry.rename.is_some();
        let blocked = name.is_empty() || invalid || (taken && entry.image.is_none());
        let modal = egui::Modal::new(ui.scope_id().with("new-entry")).frame(dialog_frame(&p)).show(ui.ctx(), |ui| {
            ui.set_width(420.0);
            ui.spacing_mut().item_spacing.y = 0.0;

            let (title, sample) = if entry.rename.is_some() {
                let title = if entry.folder { t!("ide-rename-directory-title") } else { t!("ide-rename-file-title") };
                (title, entry.name.as_str())
            } else if entry.image.is_some() {
                (t!("ide-paste-image-title"), "image.png")
            } else if entry.package {
                (t!("ide-new-package-title"), "")
            } else if entry.folder {
                (t!("ide-new-directory-title"), "")
            } else {
                (t!("ide-new-file-title"), entry.name.as_str())
            };
            ui.horizontal(|ui| {
                let (icon, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                if entry.folder {
                    crate::file_icons::folder_icon(ui.painter(), icon, "folder", false, false);
                } else {
                    crate::file_icons::file_icon(ui.painter(), icon, if sample.is_empty() { "file" } else { sample });
                }
                ui.add_space(8.0);
                ui.label(egui::RichText::new(title).font(theme::semibold(15.0)).color(p.text));
            });
            ui.add_space(16.0);

            if let Some(image) = &entry.image {
                let texture = entry.preview.get_or_insert_with(|| {
                    let pixels = egui::ColorImage::from_rgba_unmultiplied([image.width, image.height], &image.rgba);
                    ui.ctx().load_texture("paste-image-preview", pixels, egui::TextureOptions::LINEAR)
                });
                let size = texture.size_vec2();
                let scale = (380.0 / size.x).min(140.0 / size.y).min(1.0);
                ui.horizontal(|ui| {
                    egui::Frame::new().fill(p.frame).corner_radius(8).inner_margin(6).show(ui, |ui| {
                        ui.add(egui::Image::new((texture.id(), size * scale)).corner_radius(4));
                    });
                });
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!("{} × {} px", image.width, image.height))
                        .font(theme::regular(12.0))
                        .color(p.text_secondary),
                );
                ui.add_space(14.0);
            }

            ui.label(egui::RichText::new(t!("ide-new-entry-name")).font(theme::regular(12.5)).color(p.text_secondary));
            ui.add_space(6.0);
            let id = ui.scope_id().with("new-entry-name");
            let focused = ui.memory(|memory| memory.has_focus(id));
            let border = if blocked && !name.is_empty() {
                p.excluded
            } else if focused {
                p.accent
            } else {
                p.text_field_border
            };
            let field = egui::Frame::new()
                .fill(p.text_field)
                .stroke(Stroke::new(if focused { 1.5 } else { 1.0 }, border))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 7))
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut entry.name)
                            .id(id)
                            .frame(egui::Frame::NONE)
                            .font(theme::regular(14.0))
                            .hint_text(if entry.package {
                                t!("ide-new-package-hint")
                            } else {
                                t!("ide-new-entry-hint")
                            })
                            .desired_width(f32::INFINITY),
                    )
                })
                .inner;
            if !entry.focused {
                field.request_focus();
                entry.focused = true;
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
                    let stem = entry.name.rfind('.').filter(|dot| *dot > 0).unwrap_or(entry.name.len());
                    let stem = entry.name[..stem].chars().count();
                    let range =
                        egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(stem));
                    state.cursor.set_char_range(Some(range));
                    state.store(ui.ctx(), id);
                }
            }

            ui.add_space(8.0);
            let folder = home_relative(&entry.directory);
            ui.label(
                egui::RichText::new(t!("ide-new-entry-folder", folder = folder.as_str()))
                    .font(theme::regular(12.0))
                    .color(p.text_secondary),
            );
            if let Some(message) = &hint {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(message.as_str()).font(theme::regular(12.0)).color(p.excluded));
            }

            let enter =
                field.lost_focus() && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter));
            submit = enter && !blocked;
            if enter && blocked {
                field.request_focus();
            }
            ui.add_space(20.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let button = |text: String, primary: bool, enabled: bool| {
                    let color = if primary { p.accent_text } else { p.text };
                    let mut button =
                        egui::Button::new(egui::RichText::new(text).font(theme::regular(13.0)).color(color))
                            .min_size(vec2(84.0, 30.0))
                            .corner_radius(6);
                    button = if primary {
                        button.fill(if enabled { p.accent } else { p.accent.gamma_multiply(0.4) })
                    } else {
                        button.fill(Color32::TRANSPARENT).stroke(Stroke::new(1.0, p.button_border))
                    };
                    button
                };
                let ok_label = if entry.rename.is_some() {
                    t!("ide-rename-entry-ok")
                } else if entry.image.is_some() {
                    t!("ide-paste-image-save")
                } else {
                    t!("ide-new-entry-create")
                };
                if ui.add_enabled(!blocked, button(ok_label, true, !blocked)).clicked() {
                    submit = true;
                }
                if ui.add(button(t!("settings-cancel"), false, true)).clicked() {
                    cancel = true;
                }
            });
        });
        if cancel || (modal.should_close() && !submit) {
            self.new_entry = None;
            return;
        }
        if submit && !entry.name.trim().is_empty() && entry.image.is_some() {
            let Some(entry) = self.new_entry.take() else { return };
            let Some(image) = entry.image else { return };
            self.expanded.insert(entry.directory.clone());
            match save_image_for_paste(&image, entry.name.trim()) {
                Ok(staged) => {
                    let params = serde_json::json!({ "sources": [staged], "directory": entry.directory });
                    self.dispatch(ui, registry, session, "file.fs.copyInto", params);
                    if let Some(folder) = staged.parent() {
                        let _ = std::fs::remove_dir_all(folder);
                    }
                }
                Err(error) => self.post(ui, error),
            }
            return;
        }
        if submit && let Some(old) = entry.rename.clone() {
            let params = serde_json::json!({ "path": old, "name": entry.name.trim() });
            self.new_entry = None;
            let unchanged = old.file_name().is_some_and(|old| old.to_string_lossy() == name);
            if !unchanged {
                self.dispatch(ui, registry, session, "file.fs.rename", params);
            }
            return;
        }
        if submit && !entry.name.trim().is_empty() {
            let command = if entry.folder { "file.fs.newDirectory" } else { "file.fs.newFile" };
            let name = if entry.package { entry.name.trim().replace('.', "/") } else { entry.name.trim().to_owned() };
            let params = serde_json::json!({ "directory": entry.directory, "name": name });
            self.expanded.insert(entry.directory.clone());
            self.new_entry = None;
            self.dispatch(ui, registry, session, command, params);
        }
    }

    fn tree_row(
        &mut self,
        ui: &mut Ui,
        entry: &zen_vfs::Entry,
        depth: usize,
        active: Option<&Path>,
        frame: &mut TreeFrame,
        mode: libraries_tree::RowMode<'_>,
    ) {
        let p = self.palette;
        let excluded_here = excluded_entry(entry, depth, !mode.read_only, &self.tree_exclusions.1);
        if excluded_here && !self.show_excluded {
            return;
        }
        let width = ui.available_width().max(260.0);
        let (rect, response) = ui.allocate_exact_size(vec2(width, metrics::TREE_ROW), Sense::click());
        let missing = mode.kind == libraries_tree::RowKind::Missing;
        let bundle = mode.kind == libraries_tree::RowKind::Bundle;
        let group = mode.kind == libraries_tree::RowKind::Group || bundle;
        let response = if missing { response.on_hover_text(t!("ide-libraries-missing")) } else { response };
        let row = Rect::from_min_max(pos2(rect.left() + 12.0, rect.top()), pos2(rect.right() - 6.0, rect.bottom()));
        let excluded = mode.excluded || excluded_here;
        let dragging_files = ui.input(|input| !input.raw.hovered_files.is_empty());
        let pointer_here = ui.input(|input| input.pointer.hover_pos()).is_some_and(|pointer| rect.contains(pointer));
        if pointer_here && entry.is_dir && !mode.read_only {
            frame.hovered_folder = Some(entry.path.clone());
        }
        frame.visible.push(entry.path.clone());
        if self.reveal_lead && self.selection.lead() == Some(&entry.path) {
            response.scroll_to_me(None);
        }
        let is_selected = self.selection.contains(&entry.path) || active == Some(entry.path.as_path());
        if dragging_files && pointer_here && entry.is_dir && !mode.read_only {
            ui.painter().rect_stroke(row, 5, Stroke::new(1.5, p.accent), StrokeKind::Inside);
        }
        if is_selected {
            let index = self.visible_rows.iter().position(|row| row == &entry.path);
            let neighbour = |offset: isize| {
                index
                    .and_then(|index| index.checked_add_signed(offset))
                    .and_then(|index| self.visible_rows.get(index))
                    .is_some_and(|row| self.selection.contains(row))
            };
            let (above, below) = (neighbour(-1), neighbour(1));
            let corners = CornerRadius {
                nw: if above { 0 } else { 5 },
                ne: if above { 0 } else { 5 },
                sw: if below { 0 } else { 5 },
                se: if below { 0 } else { 5 },
            };
            ui.painter().rect_filled(row, corners, p.selection);
        } else if response.hovered() {
            ui.painter().rect_filled(row, 5, p.hover);
        } else if excluded_here && entry.is_dir {
            ui.painter().rect_filled(row, 5, p.excluded_background);
        } else if !entry.is_dir && crate::file_icons::is_test_file(&entry.name) {
            ui.painter().rect_filled(rect, 0, p.test_row);
        }
        let x = row.left() + depth as f32 * metrics::TREE_INDENT;
        let (arrow_x, icon_x) = (x + metrics::TREE_ARROW, x + metrics::TREE_ICON);
        let expanded = !missing && self.expanded.contains(&entry.path);
        let expandable = (entry.is_dir && !missing).then_some(expanded);
        accessibility::tree_item(&response, depth + 1, expandable, is_selected, mode.label.unwrap_or(&entry.name));
        let text_color = if excluded || missing { p.excluded } else { self.vcs_color(&entry.path).unwrap_or(p.text) };
        if bundle {
            let chevron = Rect::from_center_size(pos2(arrow_x, rect.center().y), Vec2::splat(11.0));
            if expanded {
                crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
            } else {
                crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary);
            }
            let icon = Rect::from_center_size(pos2(icon_x, rect.center().y), Vec2::splat(16.0));
            bundle_view::bundle_icon(ui.painter(), icon, &p);
        } else if entry.is_dir {
            let chevron = Rect::from_center_size(pos2(arrow_x, rect.center().y), Vec2::splat(11.0));
            if expanded {
                crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
            } else if !missing {
                crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary);
            }
            let icon = Rect::from_center_size(pos2(icon_x, rect.center().y), Vec2::splat(16.0));
            if !(excluded || missing) && !mode.read_only && self.tree_resource_roots.contains(&entry.path) {
                let kind = crate::file_icons::FolderKind::ResourceRoot;
                crate::file_icons::folder_icon_of(ui.painter(), icon, kind, false);
            } else {
                crate::file_icons::folder_icon(ui.painter(), icon, &entry.name, excluded || missing, false);
            }
        } else {
            crate::file_icons::file_icon_at(
                ui.painter(),
                Rect::from_center_size(pos2(icon_x, rect.center().y), Vec2::splat(16.0)),
                &entry.path,
            );
        }
        if entry.is_symlink {
            crate::file_icons::symlink_mark(
                ui.painter(),
                Rect::from_center_size(pos2(icon_x, rect.center().y), Vec2::splat(16.0)),
            );
        }
        let root_row = depth == 0 && mode.label.is_none();
        let label = ui.painter().text(
            pos2(x + metrics::TREE_TEXT, rect.center().y),
            Align2::LEFT_CENTER,
            mode.label.unwrap_or(&entry.name),
            if root_row || mode.module { theme::semibold(13.0) } else { theme::regular(13.0) },
            text_color,
        );
        let detail = if root_row {
            Some(home_relative(self.opened_as.as_deref().unwrap_or(&entry.path)))
        } else {
            mode.detail.map(str::to_owned).or_else(|| self.coverage_window.project_percent(&entry.path))
        };
        let mut after = label.right();
        if let Some(mnemonic) = self.tree_bookmarks.1.get(&entry.path) {
            let icon = Rect::from_center_size(pos2(label.right() + 13.0, rect.center().y), Vec2::splat(13.0));
            editor_view::bookmark_icon(ui.painter(), icon, *mnemonic, &p);
            after = icon.right() + 1.0;
            let hint = match mnemonic {
                Some(mnemonic) => t!("ide-bookmark-hint-mnemonic", mnemonic = mnemonic.to_string()),
                None => t!("ide-bookmark-hint"),
            };
            accessibility::describe(&response, &hint);
        }
        if let Some(detail) = detail {
            ui.painter().text(
                pos2(after + 7.0, rect.center().y),
                Align2::LEFT_CENTER,
                detail,
                theme::regular(12.5),
                p.text_secondary,
            );
        }

        let on_arrow = response.interact_pointer_pos().is_some_and(|pointer| (pointer.x - arrow_x).abs() < 9.0);
        if response.clicked() && !(entry.is_dir && on_arrow) {
            let modifiers = ui.input(|input| input.modifiers);
            self.selection.click(&entry.path, selection::Extend::from(modifiers), &self.visible_rows);
            if !entry.is_dir && modifiers.is_none() && self.overview.settings.editor_tabs.preview {
                frame.preview_file = Some(entry.path.clone());
            }
        }
        let toggle =
            entry.is_dir && !missing && ((response.clicked() && on_arrow) || (response.double_clicked() && !bundle));
        if bundle && response.double_clicked() {
            frame.open_file = Some(entry.path.clone());
        }
        if toggle {
            if expanded {
                self.expanded.remove(&entry.path);
            } else {
                self.expanded.insert(entry.path.clone());
            }
        }
        if !entry.is_dir && response.double_clicked() {
            frame.open_file = Some(entry.path.clone());
        }
        if response.secondary_clicked() && !self.selection.contains(&entry.path) {
            self.selection.select(entry.path.clone());
        }
        let menu_shown = response.secondary_clicked()
            || egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response))
            || self.menu_request.as_ref().is_some_and(|(path, _)| *path == entry.path);
        let selected = if menu_shown && self.selection.contains(&entry.path) {
            self.selection.items(&self.visible_rows)
        } else {
            vec![entry.path.clone()]
        };
        let targets = selection::outermost(&selected);
        let marks: Vec<PathBuf> = selected.into_iter().filter(|path| !libraries_tree::is_group(path)).collect();
        let marked = marks.iter().any(|path| self.tree_bookmarks.1.contains_key(path));
        let folder = folder_of(&entry.path);
        let in_repository = self.vcs.info.root.as_ref().is_some_and(|root| entry.path.starts_with(root));
        if group {
            return;
        }
        let info = self.tree_menu.clone().filter(|info| info.path == entry.path).unwrap_or_default();
        let root_row = self.visible_rows.first().is_some_and(|root| *root == entry.path) && !mode.read_only;
        let movable: Vec<PathBuf> = {
            let root = self.visible_rows.first();
            targets.iter().filter(|path| Some(*path) != root).cloned().collect()
        };
        let is_file = !entry.is_dir;
        let shortcut =
            |action: &str, fallback: &str| crate::keymap::shortcut(action).unwrap_or_else(|| fallback.to_owned());
        let menu = |ui: &mut Ui| {
            frame.menu_for = Some(entry.path.clone());
            ui.set_min_width(250.0);
            let mut chosen = None;
            let mut tool = None;
            let mut item = |ui: &mut Ui,
                            icon: Option<Glyph>,
                            text: String,
                            keys: &str,
                            enabled: bool,
                            action: &dyn Fn() -> TreeAction| {
                if menu_row(ui, &p, icon, text, keys, enabled) {
                    chosen = Some(action());
                    ui.close();
                }
            };
            if mode.scratch {
                let shown = |slot: &str| match slot {
                    customization::SCRATCH_RUN => info.scratch_run.is_some(),
                    "ide-tree-open-right-split" => is_file,
                    _ => true,
                };
                customization::popup(ui, customization::SCRATCH_POPUP, shown, |ui, slot| match slot {
                    customization::SCRATCH_RUN => {
                        let Some(name) = info.scratch_run.as_deref() else { return };
                        let keys = shortcut("run.process.start/context", "⌃⇧R");
                        item(ui, Some(glyph_run), t!("ide-gutter-run", name = name), &keys, true, &|| {
                            TreeAction::RunScratch(entry.path.clone())
                        });
                    }
                    "keymap-view-project-changeLanguage" => {
                        item(ui, None, t!("keymap-view-project-changeLanguage"), "", is_file, &|| {
                            TreeAction::ChangeLanguage(entry.path.clone())
                        });
                    }
                    "ide-tree-rename" => {
                        let keys = shortcut("view.rename.open", "⇧F6");
                        item(ui, None, t!("ide-tree-rename"), &keys, targets.len() == 1, &|| {
                            TreeAction::Rename(entry.path.clone())
                        });
                    }
                    "ide-tree-delete" => {
                        item(ui, None, t!("ide-tree-delete"), "⌫", true, &|| TreeAction::Delete(targets.clone()));
                    }
                    "keymap-view-project-move" => {
                        let keys = shortcut("view.project.move", "F6");
                        item(ui, None, t!("keymap-view-project-move"), &keys, is_file, &|| {
                            TreeAction::Move(entry.path.clone())
                        });
                    }
                    "ide-tree-copy-path" => {
                        item(ui, None, t!("ide-tree-copy-path"), "⇧⌘C", true, &|| {
                            TreeAction::CopyPath(targets.clone())
                        });
                    }
                    "ide-tree-open-right-split" => {
                        item(ui, Some(glyph_split_right), t!("ide-tree-open-right-split"), "⇧↵", true, &|| {
                            TreeAction::OpenRightSplit(entry.path.clone())
                        });
                    }
                    "ide-tree-open-in" => {
                        ui.menu_button(menu_label(t!("ide-tree-open-in")), |ui| {
                            if ui.button(file_manager_label()).clicked() {
                                frame.action = Some(TreeAction::Reveal(entry.path.clone()));
                                ui.close();
                            }
                        });
                    }
                    _ => {}
                });
                if chosen.is_some() {
                    frame.action = chosen;
                }
                return;
            }
            let two_folders =
                targets.iter().filter(|path| path.is_dir()).count() == 2 && !targets.iter().any(|path| path.is_file());
            let some_files = targets.iter().any(|path| !path.is_dir());
            let tools = !external_tools::menu(&info.tools).is_empty();
            let writable = !mode.read_only;
            let shown = |slot: &str| match slot {
                "ide-tree-new"
                | "ide-tree-cut"
                | "ide-tree-paste"
                | "ide-tree-rename"
                | "ide-tree-delete"
                | "menu-file-local-history"
                | "ide-tree-reload" => writable,
                "ide-tree-reformat" | "ide-tree-repair-file" => writable && is_file,
                "ide-tree-open-right-split" => is_file,
                "menu-git" => writable && in_repository,
                customization::COMPARE => writable && (is_file || two_folders),
                customization::TOOLS => tools,
                "ide-tree-mark-directory-as" => writable && entry.is_dir && !root_row,
                "ide-tree-associate" | "ide-tree-override-type" | "ide-tree-revert-override" => writable && some_files,
                "ide-tree-gitignore" => writable && in_repository && !movable.is_empty(),
                "ide-tree-run-targets" => !info.run_targets.is_empty() && targets.len() == 1,
                _ => true,
            };
            customization::popup(ui, customization::PROJECT_POPUP, shown, |ui, slot| match slot {
                "ide-tree-run-targets" => {
                    for (name, line) in &info.run_targets {
                        item(ui, Some(glyph_run), t!("ide-gutter-run", name = name.as_str()), "", writable, &|| {
                            TreeAction::RunTarget(entry.path.clone(), *line)
                        });
                    }
                }
                "ide-tree-new" => {
                    ui.menu_button(menu_label(t!("ide-tree-new")), |ui| {
                        for new in &info.new_items {
                            if ui.button(&new.title).clicked() {
                                frame.action = Some(match &new.choice {
                                    new_popup::NewChoice::File => TreeAction::NewFile(folder.clone()),
                                    new_popup::NewChoice::Directory => TreeAction::NewDirectory(folder.clone()),
                                    other => TreeAction::New(folder.clone(), other.clone()),
                                });
                                ui.close();
                            }
                        }
                    });
                }
                "ide-tree-cut" => {
                    let cut = !movable.is_empty();
                    item(ui, Some(glyph_cut), t!("ide-tree-cut"), "⌘X", cut, &|| TreeAction::Cut(movable.clone()));
                }
                "ide-tree-copy" => {
                    item(ui, Some(glyph_copy), t!("ide-tree-copy"), "⌘C", true, &|| {
                        TreeAction::Copy(targets.clone())
                    });
                }
                "ide-tree-copy-path" => {
                    item(ui, None, t!("ide-tree-copy-path"), "⇧⌘C", true, &|| {
                        TreeAction::CopyPath(targets.clone())
                    });
                }
                "ide-tree-paste" => {
                    let paste = info.can_paste;
                    item(ui, Some(glyph_paste), t!("ide-tree-paste"), "⌘V", paste, &|| {
                        TreeAction::Paste(folder.clone())
                    });
                }
                "ide-tree-rename" => {
                    let rename = !root_row && targets.len() == 1;
                    item(ui, None, t!("ide-tree-rename"), &shortcut("view.rename.open", "⇧F6"), rename, &|| {
                        TreeAction::Rename(entry.path.clone())
                    });
                }
                "ide-tree-reformat" => {
                    let keys = shortcut("editor.code.reformat", "⌥⌘L");
                    item(ui, Some(glyph_reformat), t!("ide-tree-reformat"), &keys, info.formatter, &|| {
                        TreeAction::Reformat(entry.path.clone())
                    });
                }
                "ide-tree-delete" => {
                    item(ui, None, t!("ide-tree-delete"), "⌫", !movable.is_empty(), &|| {
                        TreeAction::Delete(movable.clone())
                    });
                }
                "ide-tree-open-right-split" => {
                    item(ui, Some(glyph_split_right), t!("ide-tree-open-right-split"), "⇧↵", true, &|| {
                        TreeAction::OpenRightSplit(entry.path.clone())
                    });
                }
                "ide-tree-open-in" => {
                    ui.menu_button(menu_label(t!("ide-tree-open-in")), |ui| {
                        if ui.button(file_manager_label()).clicked() {
                            frame.action = Some(TreeAction::Reveal(entry.path.clone()));
                            ui.close();
                        }
                        if ui.button(t!("ide-tree-open-in-terminal")).clicked() {
                            frame.action = Some(TreeAction::OpenInTerminal(entry.path.clone()));
                            ui.close();
                        }
                    });
                }
                "ide-tool-bookmarks" => {
                    ui.menu_button(menu_label(t!("ide-tool-bookmarks")), |ui| {
                        ui.set_min_width(240.0);
                        let mut bookmark =
                            |ui: &mut Ui, text: String, keys: String, enabled: bool, action: TreeAction| {
                                if ui.add_enabled(enabled, egui::Button::new(text).shortcut_text(keys)).clicked() {
                                    frame.action = Some(action);
                                    ui.close();
                                }
                            };
                        let toggle = TreeAction::ToggleBookmarks(marks.clone());
                        bookmark(
                            ui,
                            t!("keymap-navigate-bookmark-toggle"),
                            shortcut("navigate.bookmark.toggle", "F3"),
                            true,
                            toggle,
                        );
                        let mnemonic = TreeAction::BookmarkMnemonic(entry.path.clone());
                        let keys = shortcut("view.bookmark.mnemonic", "⌥F3");
                        bookmark(ui, t!("ide-bookmark-toggle-mnemonic"), keys, true, mnemonic);
                        ui.separator();
                        let delete = TreeAction::DeleteBookmarks(marks.clone());
                        bookmark(ui, t!("ide-bookmarks-delete"), String::new(), marked, delete);
                    });
                }
                "menu-file-local-history" => {
                    ui.menu_button(menu_label(t!("menu-file-local-history")), |ui| {
                        if ui.button(t!("keymap-view-localHistory-show")).clicked() {
                            frame.action = Some(TreeAction::ShowHistory(entry.path.clone()));
                            ui.close();
                        }
                        if ui.button(t!("keymap-view-localHistory-putLabel")).clicked() {
                            frame.action = Some(TreeAction::PutLabel);
                            ui.close();
                        }
                    });
                }
                "ide-tree-repair-file" => {
                    if ui.button(t!("ide-tree-repair-file")).clicked() {
                        frame.action = Some(TreeAction::RepairFile(entry.path.clone()));
                        ui.close();
                    }
                }
                "menu-git" => {
                    ui.menu_button(menu_label(t!("menu-git")), |ui| {
                        ui.set_min_width(220.0);
                        let mut git = |ui: &mut Ui, text: String, keys: &str, enabled: bool, action: TreeAction| {
                            if ui.add_enabled(enabled, egui::Button::new(text).shortcut_text(keys.to_owned())).clicked()
                            {
                                frame.action = Some(action);
                                ui.close();
                            }
                        };
                        git(
                            ui,
                            t!("keymap-view-vcs-showHistory"),
                            "",
                            true,
                            TreeAction::VcsHistory(entry.path.clone()),
                        );
                        git(ui, t!("ide-tree-git-annotate"), "", is_file, TreeAction::Annotate(entry.path.clone()));
                        git(ui, t!("keymap-view-vcs-showDiff"), "", is_file, TreeAction::ShowDiff(entry.path.clone()));
                        let rollback = shortcut("view.vcs.rollback", "⌥⌘Z");
                        git(ui, t!("keymap-view-vcs-rollback"), &rollback, true, TreeAction::Rollback(targets.clone()));
                        ui.separator();
                        git(ui, t!("ide-tree-git-add"), "", true, TreeAction::GitAdd(targets.clone()));
                        git(ui, t!("ide-tree-commit-file"), "", true, TreeAction::CommitFiles(targets.clone()));
                    });
                }
                "ide-tree-reload" => {
                    item(ui, Some(glyph_refresh), t!("ide-tree-reload"), "", true, &|| {
                        TreeAction::Reload(targets.clone())
                    });
                }
                customization::COMPARE => {
                    let files: Vec<PathBuf> = targets.iter().filter(|path| path.is_file()).cloned().collect();
                    if is_file && (2..=3).contains(&files.len()) {
                        item(ui, Some(glyph_compare), t!("ide-tree-compare-files"), "⌘D", true, &|| {
                            TreeAction::CompareFiles(files.clone())
                        });
                    } else if let ([left, right], []) = (
                        targets.iter().filter(|path| path.is_dir()).cloned().collect::<Vec<_>>().as_slice(),
                        files.as_slice(),
                    ) {
                        item(ui, Some(glyph_compare), t!("ide-tree-compare-directories"), "⌘D", true, &|| {
                            TreeAction::CompareFolders(left.clone(), right.clone())
                        });
                    } else if is_file {
                        item(ui, Some(glyph_compare), t!("ide-tree-compare-with"), "⌘D", true, &|| {
                            TreeAction::CompareWith(entry.path.clone())
                        });
                        let editor = info.editor.is_some();
                        item(ui, None, t!("ide-tree-compare-editor"), "", editor, &|| {
                            TreeAction::CompareWithEditor(entry.path.clone())
                        });
                    }
                }
                customization::TOOLS => {
                    tool = external_tools::context_submenus(ui, &info.tools);
                }
                "ide-tree-mark-directory-as" => {
                    ui.menu_button(menu_label(t!("ide-tree-mark-directory-as")), |ui| {
                        ui.set_min_width(180.0);
                        let (text, enabled, action) = match info.folder {
                            FolderMark::Excluded => {
                                (t!("ide-tree-cancel-exclusion"), true, TreeAction::CancelExclusion(entry.path.clone()))
                            }
                            mark => (
                                t!("ide-tree-mark-excluded"),
                                mark == FolderMark::Excludable,
                                TreeAction::MarkExcluded(entry.path.clone()),
                            ),
                        };
                        if menu_row(ui, &p, Some(excluded_folder_glyph), text, "", enabled) {
                            frame.action = Some(action);
                            ui.close();
                        }
                        let markable = matches!(info.folder, FolderMark::Excludable | FolderMark::Excluded);
                        let (text, action) = if info.resource_root {
                            (t!("ide-tree-unmark-resource-root"), TreeAction::UnmarkResourceRoot(entry.path.clone()))
                        } else {
                            (t!("ide-tree-mark-resource-root"), TreeAction::MarkResourceRoot(entry.path.clone()))
                        };
                        let enabled = info.resource_root || markable;
                        if menu_row(ui, &p, Some(resource_root_glyph), text, "", enabled) {
                            frame.action = Some(action);
                            ui.close();
                        }
                    });
                }
                "ide-tree-associate" => {
                    item(ui, None, t!("ide-tree-associate"), "", is_file && targets.len() == 1, &|| {
                        TreeAction::Associate(entry.path.clone())
                    });
                }
                "ide-tree-override-type" => {
                    let files: Vec<PathBuf> = targets.iter().filter(|path| !path.is_dir()).cloned().collect();
                    ui.menu_button(menu_label(t!("ide-tree-override-type")), |ui| {
                        ui.set_min_width(200.0);
                        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                            for (id, name) in &info.types {
                                if ui.button(name).clicked() {
                                    frame.action = Some(TreeAction::OverrideType(files.clone(), id.clone()));
                                    ui.close();
                                }
                            }
                        });
                    });
                }
                "ide-tree-revert-override" => {
                    let files: Vec<PathBuf> = targets.iter().filter(|path| !path.is_dir()).cloned().collect();
                    let registry = zen_vfs::file_types::current();
                    let overridden = files.iter().any(|path| registry.overrides().contains_key(path));
                    item(ui, None, t!("ide-tree-revert-override"), "", overridden, &|| {
                        TreeAction::RevertOverride(files.clone())
                    });
                }
                "ide-tree-gitignore" => {
                    item(ui, Some(glyph_ignore), t!("ide-tree-gitignore"), "", true, &|| {
                        TreeAction::Ignore(movable.clone())
                    });
                }
                _ => {}
            });
            if let Some(id) = tool {
                frame.action = Some(TreeAction::ExternalTool(id, entry.path.clone()));
            }
            if chosen.is_some() {
                frame.action = chosen;
            }
        };
        match self.menu_request.as_mut().filter(|(path, _)| *path == entry.path) {
            Some((_, first)) => {
                let open = std::mem::take(first).then_some(egui::SetOpenCommand::Bool(true));
                let at = pos2(rect.left() + x + metrics::TREE_TEXT, rect.bottom());
                egui::Popup::menu(&response).open_memory(open).at_position(at).show(menu);
                if !egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response)) {
                    self.menu_request = None;
                }
            }
            None => {
                response.context_menu(menu);
            }
        }
        if entry.is_dir && expanded {
            let children = self
                .listings
                .entry(entry.path.clone())
                .or_insert_with(|| zen_vfs::list(&entry.path).unwrap_or_default())
                .clone();
            let scratch = mode.scratch || self.scratch_folders.contains(&entry.path);
            let child_mode =
                libraries_tree::RowMode { excluded, read_only: mode.read_only, scratch, ..Default::default() };
            let bundles = if self.group_bundles && !mode.read_only {
                let files: Vec<PathBuf> = children
                    .iter()
                    .filter(|child| !child.is_dir && child.name.ends_with(".properties"))
                    .map(|child| child.path.clone())
                    .collect();
                zen_workbench::resource_bundles::group(&files)
            } else {
                Vec::new()
            };
            let mut shown_bundles = Vec::new();
            for child in &children {
                if let Some(bundle) = bundles.iter().find(|bundle| bundle.contains(&child.path)) {
                    if !shown_bundles.contains(&bundle.name) {
                        shown_bundles.push(bundle.name.clone());
                        self.bundle_row(ui, bundle, depth + 1, active, frame, child_mode);
                    }
                    continue;
                }
                let module = (child.is_dir && !mode.read_only)
                    .then(|| self.libraries.as_ref().and_then(|libraries| libraries.module_at(&child.path)))
                    .flatten()
                    .map(str::to_owned);
                let detail = module.as_ref().filter(|name| **name != child.name).map(|name| format!("[{name}]"));
                let mode =
                    libraries_tree::RowMode { module: module.is_some(), detail: detail.as_deref(), ..child_mode };
                self.tree_row(ui, child, depth + 1, active, frame, mode);
            }
        }
    }

    fn editor_area(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        self.as_git(|view| view.git_workspace_frame(ui.ctx(), session));
        self.dropped_patch(ui);
        let covered = self.editor_covered(session);
        self.rebase_frame(session);
        self.conflict_frame(session);
        self.settle_front(session, covered);
        let text = match self.workspace_area(ui, registry, session) {
            Some(text) => text,
            None => self.split_editors(ui, registry, session),
        };
        if text {
            self.goto_dialog(ui, registry, session);
            self.rename_dialog(ui, registry, session);
        }
    }

    fn editor_group(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        group: zen_workbench::groups::GroupId,
        focused: bool,
    ) -> bool {
        self.editor_group_in(ui, registry, session, group, focused, None)
    }

    pub(super) fn workspace_strip(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        group: zen_workbench::groups::GroupId,
        expanded: &str,
    ) {
        self.editor_group_in(ui, registry, session, group, true, Some(expanded));
    }

    fn editor_group_in(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        group: zen_workbench::groups::GroupId,
        focused: bool,
        strip_only: Option<&str>,
    ) -> bool {
        let p = self.palette;
        let (names, active) = match session.state::<Workbench>() {
            Some(workbench) => {
                let shown = workbench.groups().group(group);
                (
                    shown
                        .map(|shown| {
                            shown
                                .tabs
                                .iter()
                                .filter_map(|path| workbench.documents().iter().find(|d| d.path() == path))
                                .map(|document| (document.name(), document.is_dirty(), document.path().to_owned()))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                    shown.and_then(|shown| shown.active),
                )
            }
            None => (Vec::new(), None),
        };
        let documents = names.len();
        let main = session.state::<Workbench>().is_some_and(|workbench| workbench.groups().main_focused() == group);
        let open: Vec<zen_workbench::workspaces::WorkspaceInfo> = session
            .state::<zen_workbench::workspaces::Workspaces>()
            .filter(|_| main)
            .map(|workspaces| workspaces.open_workspaces().into_iter().cloned().collect())
            .unwrap_or_default();
        let workspace_tab = |index: usize| index.checked_sub(documents).filter(|at| *at < open.len());
        let extras = group == self.groups_ui.extras_group && strip_only.is_none();
        let folder_open = extras && self.folder_diff.is_some();
        let folder_tab = folder_open.then_some(documents + open.len());
        let bundle_open = extras && self.bundle.is_some();
        let bundle_tab = bundle_open.then_some(documents + open.len() + usize::from(folder_open));
        let diff_tab = (extras && self.diff.is_some() && !self.diff_from_workspace)
            .then_some(documents + open.len() + usize::from(folder_open) + usize::from(bundle_open));
        let mut names = names;
        for info in &open {
            names.push((info.title.clone(), false, PathBuf::new()));
        }
        if let Some(folder) = self.folder_diff.as_ref().filter(|_| folder_open) {
            names.push((folder.title(), false, PathBuf::new()));
        }
        if let Some(bundle) = self.bundle.as_ref().filter(|_| bundle_open) {
            names.push((bundle.title(), false, bundle.path.clone()));
        }
        if let Some(diff) = self.diff.as_ref().filter(|_| diff_tab.is_some()) {
            names.push((diff.title(), false, PathBuf::new()));
        }
        let front_tab = |kind: FrontKind| match kind {
            FrontKind::Diff => diff_tab,
            FrontKind::Folder => folder_tab,
            FrontKind::Bundle => bundle_tab,
        };
        let front = self.front.map(|front| front.kind).filter(|kind| focused && front_tab(*kind).is_some());
        let active = match strip_only {
            Some(expanded) => open.iter().position(|info| info.id == expanded).map(|at| documents + at),
            None => front.map_or(active, front_tab),
        };
        if names.is_empty() {
            self.caret = None;
            let rect = ui.max_rect();
            ui.painter().text(
                rect.center() - vec2(0.0, 12.0),
                Align2::CENTER_CENTER,
                t!("ide-empty-editor"),
                theme::regular(13.0),
                p.text_secondary,
            );
            ui.painter().text(
                rect.center() + vec2(0.0, 12.0),
                Align2::CENTER_CENTER,
                t!("ide-empty-editor-hint"),
                theme::regular(12.0),
                p.gutter_text,
            );
            return false;
        }

        let tab_settings = self.overview.settings.editor_tabs.clone();
        let look = if strip_only.is_some() { tab_strip::Look::default() } else { tab_strip::Look::of(&tab_settings) };
        let hidden = self.appearance.hides_chrome() || look.placement == TabPlacement::None;
        let tabs_height = if hidden { 0.0 } else { metrics::TABS };
        let split = session.state::<Workbench>().is_some_and(|workbench| workbench.groups().is_split());
        let (pins, previews, opposite, reopen) = session
            .state::<Workbench>()
            .map(|workbench| {
                let groups = workbench.groups();
                let shown = groups.group(group);
                let file_tabs = names.iter().take(documents);
                let pins: Vec<bool> =
                    file_tabs.clone().map(|(_, _, path)| shown.is_some_and(|shown| shown.is_pinned(path))).collect();
                let previews: Vec<bool> =
                    file_tabs.map(|(_, _, path)| shown.is_some_and(|shown| shown.is_preview(path))).collect();
                (pins, previews, groups.opposite(group).is_some(), !workbench.closed_tabs().is_empty())
            })
            .unwrap_or_default();
        let titles = editor_groups::tab_titles(&names[..documents], &tab_settings);
        let under_vcs = self.vcs.info.root.is_some();
        let tabs: Vec<tab_strip::Tab<'_>> = names
            .iter()
            .enumerate()
            .map(|(index, (name, dirty, path))| {
                let icon: tab_strip::TabIcon<'_> = if Some(index) == diff_tab || Some(index) == folder_tab {
                    Box::new(move |painter, rect| diff_view::diff_icon(painter, rect, &p))
                } else if let Some(at) = workspace_tab(index) {
                    let glyph = workspace_host::workspace_glyph(&open[at].icon);
                    let color = workspace_host::workspace_glyph_color(&open[at].icon, &p);
                    Box::new(move |painter, rect| glyph(painter, rect.shrink(0.5), color))
                } else {
                    Box::new(move |painter, rect| crate::file_icons::file_icon_at(painter, rect, path))
                };
                let color = self.vcs_color(path).unwrap_or(if *dirty && !under_vcs { p.modified_tab } else { p.text });
                tab_strip::Tab {
                    title: titles.get(index).unwrap_or(name).clone(),
                    color,
                    icon,
                    dirty: *dirty,
                    closable: true,
                    draggable: index < documents,
                    strip: None,
                    mark: None,
                    pinned: pins.get(index).copied().unwrap_or(false),
                    preview: previews.get(index).copied().unwrap_or(false),
                }
            })
            .collect();
        let mut row = self.groups_ui.take_row(group);
        let options = tab_strip::Options { height: tabs_height, focused, reorder: false, look };
        let aside = tabs_height > 0.0 && look.placement != TabPlacement::Top;
        let mut body = None;
        let outcome = if aside {
            let whole = ui.available_rect_before_wrap();
            let needed = tab_strip::height(ui, &p, &tabs, whole.width(), options);
            let (strip_area, body_area) = tab_strip::areas(whole, look.placement, needed);
            let mut strip_ui = ui.new_child(UiBuilder::new().id_salt("editor-tab-row").max_rect(strip_area));
            let outcome = tab_strip::show(&mut strip_ui, &p, &mut row, &tabs, active, options);
            body = Some(ui.new_child(UiBuilder::new().scope_id(ui.scope_id()).max_rect(body_area)));
            ui.advance_cursor_after_rect(whole);
            outcome
        } else {
            tab_strip::show(ui, &p, &mut row, &tabs, active, options)
        };
        let ui = match body.as_mut() {
            Some(body) => body,
            None => ui,
        };
        drop(tabs);
        let held_place = row.place;
        self.groups_ui.put_row(group, row);
        let (select, close) = (outcome.select, outcome.close);
        if let Some(index) = outcome.keep_open.filter(|index| *index < documents) {
            let params = serde_json::json!({ "group": group, "index": index });
            self.dispatch(ui, registry, session, "editor.tab.keepOpen", params);
        }
        if let Some(index) = outcome.close_others.filter(|index| *index < documents) {
            let params = serde_json::json!({ "group": group, "index": index });
            self.dispatch(ui, registry, session, "file.document.closeOthers", params);
            return false;
        }
        if let Some(index) = outcome.unpin.filter(|index| *index < documents) {
            let params = serde_json::json!({ "group": group, "index": index, "pinned": false });
            self.dispatch(ui, registry, session, "editor.tab.pin", params);
            return false;
        }
        if let Some(index) = outcome.drag_started {
            self.groups_ui.drag_tab(group, index, names[index].0.clone());
        }
        let placed: Vec<(usize, Rect)> = outcome
            .places
            .iter()
            .take(documents)
            .enumerate()
            .filter_map(|(index, place)| place.map(|place| (index, place)))
            .collect();
        self.groups_ui.tab_rows.insert(group, (placed, look.vertical(), held_place));
        for (at, info) in open.iter().enumerate() {
            if let Some(response) = outcome.responses.get(documents + at).and_then(Option::as_ref) {
                self.groups_ui.workspace_tabs.push((info.id.clone(), response.rect));
            }
        }
        let mut tab_action = None;
        let mut open_in = None;
        for (index, response) in outcome.responses.iter().enumerate().take(documents) {
            let Some(response) = response else { continue };
            let path = &names[index].2;
            if tab_settings.full_path_on_hover {
                let _ = response.clone().on_hover_text(path.display().to_string());
            }
            let bookmarked = self.tree_bookmarks.1.contains_key(path);
            let menu = editor_groups::TabMenu {
                group,
                index,
                path,
                pins: &pins,
                previews: &previews,
                split,
                opposite,
                bookmarked,
                reopen,
            };
            response.context_menu(|ui| match editor_groups::tab_menu(ui, &menu) {
                Some(editor_groups::TabChoice::Command(command, params)) => tab_action = Some((command, params)),
                Some(editor_groups::TabChoice::Tree(action)) => open_in = Some(action),
                None => {}
            });
        }
        if let Some(action) = open_in {
            self.tree_action(ui, registry, session, action);
            return false;
        }
        if let Some((command, params)) = tab_action {
            if command.starts_with("view.") {
                self.run_action(ui, registry, session, command, params);
            } else {
                self.dispatch(ui, registry, session, command, params);
            }
            return false;
        }
        if let Some(index) = close {
            if Some(index) == folder_tab {
                if let Some(tab) = self.folder_diff.take() {
                    tab.cancel();
                }
                self.drop_front(FrontKind::Folder);
            } else if Some(index) == bundle_tab {
                self.bundle = None;
                self.drop_front(FrontKind::Bundle);
            } else if Some(index) == diff_tab {
                self.diff = None;
                self.drop_front(FrontKind::Diff);
            } else if let Some(at) = workspace_tab(index) {
                let params = serde_json::json!({ "id": open[at].id });
                self.dispatch(ui, registry, session, "workspace.host.close", params);
            } else {
                let params = serde_json::json!({ "group": group, "index": index });
                self.dispatch(ui, registry, session, "file.document.close", params);
            }
            return false;
        }
        if let Some(index) = select {
            if let Some(at) = workspace_tab(index) {
                let params = serde_json::json!({ "id": open[at].id });
                self.dispatch(ui, registry, session, "workspace.host.expand", params);
                return false;
            }
            if !focused && (Some(index) == diff_tab || Some(index) == folder_tab || Some(index) == bundle_tab) {
                let _ = registry.dispatch(session, "editor.split.focus", serde_json::json!({ "group": group }));
            }
            let kind = [FrontKind::Folder, FrontKind::Bundle, FrontKind::Diff]
                .into_iter()
                .find(|kind| front_tab(*kind) == Some(index));
            if let Some(kind) = kind {
                self.bring_front(kind, session);
                self.behind = None;
            } else {
                self.front = None;
                self.behind = None;
                let params = serde_json::json!({ "group": group, "index": index });
                self.dispatch(ui, registry, session, "editor.tab.select", params);
            }
        }
        if strip_only.is_some() {
            return false;
        }
        let front = self.front.map(|front| front.kind).filter(|kind| focused && front_tab(*kind).is_some());
        if front.is_some() {
            self.close_text_dialogs();
        }
        if front == Some(FrontKind::Folder) {
            self.caret = None;
            self.folder_frame(ui, registry, session);
            return false;
        }
        if front == Some(FrontKind::Bundle) {
            self.caret = None;
            self.bundle_frame(ui, registry, session);
            return false;
        }
        if front == Some(FrontKind::Diff) && self.diff.is_some() {
            self.caret = None;
            self.diff_body(ui, registry, session);
            return false;
        }

        self.group_text(ui, registry, session, group, focused)
    }

    pub(super) fn diff_body(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        self.caret = None;
        if self.diff.as_ref().is_some_and(|diff| {
            diff.view.part.is_some() && diff.revision.is_some_and(|seen| seen != self.vcs.info.revision)
        }) {
            self.refresh_diff(registry, session);
        }
        self.follow_diff_file(registry, session);
        let Some(diff) = &mut self.diff else { return };
        let mut outcome = diff.show(ui, &p);
        if outcome.switched {
            self.diff_unified = diff.unified;
        }
        if let Some((command, params)) = outcome.ask.take() {
            self.dispatch(ui, registry, session, command, params);
            let _ = registry.dispatch(session, "vcs.status.refresh", serde_json::json!({}));
            self.refresh_diff(registry, session);
        }
        self.after_diff(registry, session, outcome);
    }

    pub(super) fn group_text(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        group: zen_workbench::groups::GroupId,
        focused: bool,
    ) -> bool {
        if session.state::<Workbench>().and_then(Workbench::active_document).is_some_and(|d| d.image().is_some()) {
            if focused {
                self.close_text_dialogs();
            }
            self.image_viewer(ui, registry, session, focused);
            return false;
        }

        let Some(name) =
            session.state::<Workbench>().and_then(|workbench| workbench.active_document()).map(Document::name)
        else {
            if focused {
                self.close_text_dialogs();
            }
            return false;
        };
        let (path, first_line) = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .map(|document| (document.path().to_owned(), zen_vfs::file_types::head(document.text()).to_owned()))
            .unzip();
        let path = path.unwrap_or_else(|| PathBuf::from(&name));
        let highlighter = zen_vfs::file_types::known_name_at(&path, first_line.as_deref())
            .and_then(|known| self.highlighter_for(&known));
        let overridden = zen_vfs::file_types::current().overridden(&path).is_some();
        let palette = self.palette;
        if focused {
            if highlighter.is_none()
                && !overridden
                && let Some(offer) = self.plugins.offer(&name)
            {
                self.plugin_banner(ui, offer);
            }

            self.conflict_banner(ui, registry, session);
            self.library_banner(ui, registry, session, &path);
            if self.safe_mode {
                self.safe_mode_banner(ui, session);
            } else {
                self.language_server_banner(ui, registry, session, &name);
            }
            let in_workspace =
                session.state::<Workbench>().is_some_and(|workbench| workbench.groups().owner(group).is_some());
            if !in_workspace && registry.info("db.script.state").is_some() && editor_view::is_script(session, &path) {
                self.script_bar.show(ui, &palette, registry, session, &path);
            }

            if self.find_bar.show(ui, registry, session, &palette) == find_bar::Outcome::Closed {
                self.editor_view.focus();
            }
        }
        self.editor_view.passive = !focused || self.macro_dialogs.is_open();
        self.editor_view.split =
            session.state::<Workbench>().map(Workbench::groups).filter(|groups| groups.is_split()).and_then(|groups| {
                let number = groups.groups().iter().position(|shown| shown.id == group)?;
                Some((number + 1, groups.groups().len()))
            });
        self.editor_view.ligatures = self.overview.settings.ligatures;
        self.editor_view.run_icons = self.overview.settings.gutter_run_icons;
        self.editor_view.font = self.shown_editor_font(session);
        self.editor_view.hide_line_numbers = self.appearance.hides_line_numbers();
        self.editor_view.code_vision.enabled = self.overview.settings.code_vision.any();
        self.editor_view.code_vision.kinds = (self.overview.settings.code_vision.usages, self.code_authors_shown());
        crate::fonts::use_editor_font(ui.ctx(), self.overview.settings.editor_font.family.as_deref());
        self.overview.settings.apply_to_session(session.settings_mut());
        self.share_editor_font_size(session);
        ui.add_space(-ui.spacing().item_spacing.y);
        let area = ui.available_rect_before_wrap();
        let preview = session.state::<Workbench>().and_then(Workbench::active_document).and_then(|document| {
            Some((document.path().to_owned(), zen_workbench::preview::PreviewKind::of(document.path())?))
        });
        let caret = match preview {
            Some((path, kind)) => {
                self.text_with_preview(ui, registry, session, highlighter, &palette, area, (&path, kind), focused)
            }
            None => self.editor_with_breadcrumbs(ui, registry, session, highlighter, &palette),
        };
        self.font_zoom(ui, registry, session, group, area);
        if focused {
            self.caret = caret;
            self.build_changes_button(ui, registry, session, area);
        }
        if let Some(message) = self.editor_view.take_notice() {
            self.post(ui, message);
        }
        for action in self.editor_view.take_window_actions() {
            match action {
                "view.toolWindow.run" => self.show_tool("run"),
                "view.toolWindow.services" => self.show_tool("services"),
                other => self.run_action(ui, registry, session, other, serde_json::json!({})),
            }
        }
        for (action, params) in self.editor_view.take_gutter_actions() {
            self.run_action(ui, registry, session, &action, params);
        }
        for (action, params) in self.editor_view.take_problem_actions() {
            self.run_action(ui, registry, session, &action, params);
        }
        if let Some(commit) = self.editor_view.take_blame_request() {
            let file = session.state::<Workbench>().and_then(Workbench::active_document).map(|d| d.path().to_owned());
            let from_workspace = session
                .state::<zen_workbench::vcs::workspace::GitWorkspace>()
                .is_some_and(|workspace| workspace.is_open() && file.is_some_and(|file| workspace.blamed_here(&file)));
            if from_workspace {
                self.dispatch(ui, registry, session, "vcs.workspace.open", serde_json::json!({}));
                self.dispatch(ui, registry, session, "vcs.workspace.select", serde_json::json!({ "revision": commit }));
            } else {
                self.git_window.show_commit(commit);
                self.show_tool("git");
            }
        }
        if let Some(path) = self.editor_view.take_diff_request() {
            self.open_diff(ui.ctx(), registry, session, &path);
        }
        if self.editor_view.take_configure_templates() {
            self.settings.open_at(settings::PageId::LiveTemplates);
        }
        if let Some(params) = self.editor_view.take_debug_request() {
            self.run_action(ui, registry, session, "debug.session.start", params);
        }
        if let Some(id) = self.editor_view.take_external_tool_request() {
            self.run_chosen_tool(ui, registry, session, &id, None);
        }
        if let Some(file) = self.editor_view.take_starters_request() {
            self.run_action(ui, registry, session, starters_dialog::OPEN, serde_json::json!({ "file": file }));
        }
        customization::note_commands(registry);
        if let Some(action) = customization::take_chosen(ui.ctx()) {
            self.run_customized(ui, registry, session, &action);
        }
        true
    }

    fn plugin_banner(&mut self, ui: &mut Ui, offer: Offer) {
        let p = self.palette;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::BANNER), Sense::hover());
        let band =
            Rect::from_min_max(pos2(rect.left() + 6.0, rect.top()), pos2(rect.right() - 6.0, rect.bottom() - 4.0));
        ui.painter().rect_filled(band, 6, p.banner);
        let y = band.center().y;
        match offer {
            Offer::Installing { name } => {
                ui.painter().text(
                    pos2(band.left() + 14.0, y),
                    Align2::LEFT_CENTER,
                    t!("ide-plugin-installing", name = name.as_str()),
                    theme::regular(13.0),
                    p.text,
                );
            }
            Offer::Install { plugin, extension } => {
                let pattern = format!("*.{extension}");
                let logged = NewNotification::worded(
                    Group::Plugins,
                    Kind::Info,
                    zen_i18n::Message::new("ide-plugin-suggestion").with("pattern", pattern.as_str()),
                )
                .action(Action::new(
                    "install",
                    t!("ide-plugin-install", name = plugin.name.as_str()),
                    "view.plugin.install",
                    serde_json::json!({ "id": plugin.id }),
                ))
                .action(Action::new(
                    "ignore",
                    t!("ide-plugin-ignore"),
                    "view.plugin.ignore",
                    serde_json::json!({ "extension": extension }),
                ))
                .silent();
                self.notifications.queue_once(format!("plugin:{extension}"), logged);
                let text = ui.painter().text(
                    pos2(band.left() + 14.0, y),
                    Align2::LEFT_CENTER,
                    t!("ide-plugin-suggestion", pattern = pattern.as_str()),
                    theme::regular(13.0),
                    p.text,
                );
                let mut x = text.right() + 20.0;
                let install = t!("ide-plugin-install", name = plugin.name.as_str());
                if self.link(ui, &mut x, y, &install, "install") {
                    self.plugins.install_plugin(*plugin);
                }
                if self.link(ui, &mut x, y, &t!("ide-plugin-ignore"), "ignore") {
                    self.plugins.ignore(&extension);
                }
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn follow_disk(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session) -> bool {
        let project = session.state::<Workbench>().and_then(|workbench| workbench.project().map(Path::to_owned));
        let Some(project) = project else { return false };
        let now = std::time::Instant::now();
        if self.watcher.as_ref().is_none_or(|(root, _)| *root != project) && self.watch_retry.due(&project, now) {
            let wake = ctx.clone();
            match zen_vfs::Watcher::new(&project, move || wake.request_repaint()) {
                Ok(watcher) => {
                    self.watcher = Some((project.clone(), watcher));
                    self.watch_retry = zen_vfs::WatchRetry::default();
                }
                Err(_) => {
                    self.watcher = None;
                    if self.watch_retry.failed(&project, now) {
                        let message = t!("ide-watch-failed", project = project.display().to_string());
                        self.notify(NewNotification::message(Group::General, Kind::Warning, message));
                    }
                }
            }
        }
        let Some((_, watcher)) = &self.watcher else { return false };
        let mut changes = watcher.take_changes();
        let missed = watcher.take_missed();
        if missed {
            changes.push(project.clone());
        }
        if changes.is_empty() {
            return false;
        }
        self.listings.clear();
        self.libraries = None;
        self.todo_window.disk_changed();
        self.selection.retain(|path| path.exists());
        let model = if missed { serde_json::json!({}) } else { serde_json::json!({ "paths": changes }) };
        let _ = registry.dispatch(session, "project.model.refresh", model);
        session.state_mut::<zen_workbench::language::Languages>().files_changed(&changes);
        zen_workbench::database::detect::files_changed(session, &changes);
        zen_workbench::database::migrations::files_changed(session, &changes);
        zen_workbench::project_index::files_changed(session, &changes, std::time::Instant::now());
        let settings = if missed { serde_json::json!({}) } else { serde_json::json!({ "paths": changes }) };
        let _ = registry.dispatch(session, "project.settings.refresh", settings);
        let _ = registry.dispatch(session, "file.index.refresh", serde_json::json!({ "paths": changes }));
        let _ = registry.dispatch(session, "localHistory.changes.record", serde_json::json!({ "paths": changes }));
        zen_workbench::file_watchers::files_changed(session, &changes, std::time::Instant::now());
        let in_ignored_folders = session.state::<Workbench>().and_then(Workbench::file_index).is_some_and(|index| {
            changes.iter().all(|path| path.parent().is_some_and(|folder| index.git_ignores(folder)))
        });
        if !in_ignored_folders {
            self.vcs_files_changed(registry, session);
        }
        let paths = paths_to_compare(session, &project, changes);
        let _ = registry.dispatch(session, "file.document.syncWithDisk", serde_json::json!({ "paths": paths }));
        true
    }

    fn language_servers(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session) {
        if !std::mem::replace(&mut self.languages_wake, true) {
            let ctx = ctx.clone();
            let viewport = ctx.viewport_id();
            let waker: zen_run::Waker = Arc::new(move || {
                ctx.request_repaint_of(viewport);
                ctx.request_repaint();
            });
            session.state_mut::<zen_workbench::run::Runs>().set_waker(Arc::clone(&waker));
            session.state_mut::<zen_workbench::terminal::Terminals>().set_waker(Arc::clone(&waker));
            session.state_mut::<zen_workbench::http::HttpClient>().set_waker(Arc::clone(&waker));
            session.state_mut::<zen_workbench::database::Database>().set_waker(Arc::clone(&waker));
            session.state_mut::<zen_workbench::inspections::Inspections>().set_wake(Arc::clone(&waker));
            session.state_mut::<zen_workbench::tasks::Tasks>().set_waker(Arc::clone(&waker));
            session.state_mut::<zen_workbench::language::Languages>().set_waker(waker);
        }
        self.toolchain_watch.step(registry, session);
        zen_workbench::database::connections::step(session);
        zen_workbench::database::consoles::step(session);
        self.follow_inspections(ctx, registry, session);
        if session.state::<zen_workbench::inspections::Inspections>().is_some_and(|inspections| inspections.running()) {
            let _ = registry.dispatch(session, "problems.fix.status", serde_json::json!({}));
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        let Ok(report) = registry.dispatch(session, "language.servers.sync", serde_json::json!({})) else { return };
        for notice in report["notices"].as_array().into_iter().flatten() {
            if let Some(notice) = notice.as_str() {
                self.notify(NewNotification::message(Group::LanguageServers, Kind::Info, notice));
            }
        }
        if report["servers"]
            .as_object()
            .is_some_and(|servers| servers.values().any(|server| server["state"] == "starting"))
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    fn problems_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let title = ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            t!("ide-problems-title"),
            theme::semibold(13.0),
            p.text,
        );
        accessibility::heading(ui, ui.scope_id().with("problems-title"), title, || t!("ide-problems-title"));
        let mut x = title.right() + 14.0;
        let results = self.inspect_window.tab.is_some();
        for (project, label) in [(false, t!("ide-problems-file")), (true, t!("ide-problems-project"))] {
            let galley = ui.painter().layout_no_wrap(label, theme::regular(13.0), p.text);
            let tab = Rect::from_min_size(pos2(x, header.center().y - 13.0), vec2(galley.size().x + 20.0, 26.0));
            let shown = Arc::clone(&galley);
            let response = ui.interact(tab, ui.scope_id().with(("problems-tab", project)), Sense::click());
            let selected = !results && self.problems_project == project;
            accessibility::item(&response, egui::Role::Tab, selected, shown.text());
            if selected {
                ui.painter().rect_filled(tab, 6, p.selection);
            } else if response.hovered() {
                ui.painter().rect_filled(tab, 6, p.hover);
            }
            ui.painter().galley(tab.center() - galley.size() / 2.0, galley, p.text);
            if response.clicked() {
                self.problems_project = project;
                self.inspect_window.tab = None;
            }
            x = tab.right() + 6.0;
        }
        for (run, label) in self.inspect_tabs(session) {
            let galley = ui.painter().layout_no_wrap(label, theme::regular(13.0), p.text);
            let tab = Rect::from_min_size(pos2(x, header.center().y - 13.0), vec2(galley.size().x + 38.0, 26.0));
            let shown = Arc::clone(&galley);
            let response = ui.interact(tab, ui.scope_id().with(("inspect-tab", run)), Sense::click());
            let selected = self.inspect_window.tab == Some(run);
            accessibility::item(&response, egui::Role::Tab, selected, shown.text());
            if selected {
                ui.painter().rect_filled(tab, 6, p.selection);
            } else if response.hovered() {
                ui.painter().rect_filled(tab, 6, p.hover);
            }
            ui.painter().galley(pos2(tab.left() + 10.0, tab.center().y - galley.size().y / 2.0), galley, p.text);
            let close = Rect::from_center_size(pos2(tab.right() - 13.0, tab.center().y), Vec2::splat(16.0));
            let closing = ui.interact(close, ui.scope_id().with(("inspect-tab-close", run)), Sense::click());
            accessibility::button_with(&closing, || t!("ide-inspect-close-tab"));
            glyph_close(ui.painter(), close.shrink(3.0), p.text_secondary);
            if closing.clicked() {
                self.dispatch(ui, registry, session, "inspect.results.close", serde_json::json!({ "run": run }));
            } else if response.clicked() {
                self.inspect_window.tab = Some(run);
            }
            x = tab.right() + 6.0;
        }
        let hide = Rect::from_center_size(pos2(header.right() - 18.0, header.center().y), Vec2::splat(24.0));
        let response = ui.interact(hide, ui.scope_id().with("problems-hide"), Sense::click());
        accessibility::button_with(&response, || t!("a11y-hide"));
        hover(ui, &response, &p);
        glyph_minus(ui.painter(), hide.shrink(6.0), p.text_secondary);
        if response.clicked() {
            self.hide_tool("problems");
        }
        if self.inspect_window.tab.is_some() {
            self.inspect_results(ui, registry, session);
            return;
        }

        let active = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .map(|document| document.path().to_owned());
        let files: Vec<(PathBuf, Vec<serde_json::Value>)> = if self.problems_project {
            registry
                .dispatch(session, "problems.project.list", serde_json::json!({}))
                .map(|answer| zen_workbench::language::listed_problems(&answer))
                .unwrap_or_default()
                .into_iter()
                .filter_map(|file| Some((PathBuf::from(file["path"].as_str()?), file["problems"].as_array()?.clone())))
                .collect()
        } else {
            active
                .iter()
                .map(|path| {
                    let problems = registry
                        .dispatch(session, "problems.file.list", serde_json::json!({ "path": path }))
                        .map(|answer| zen_workbench::language::listed_problems(&answer))
                        .unwrap_or_default();
                    (path.clone(), problems)
                })
                .collect()
        };
        let mut go_to: Option<(PathBuf, usize)> = None;
        let mut suppress: Option<serde_json::Value> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let total: usize = files.iter().map(|(_, problems)| problems.len()).sum();
            if total == 0 {
                let status =
                    registry.dispatch(session, "language.servers.status", serde_json::json!({})).unwrap_or_default();
                let failed = status.as_object().and_then(|servers| {
                    servers.iter().find_map(|(name, server)| match server["state"].as_str() {
                        Some("failed") => Some(format!("{name}: {}", server["reason"].as_str().unwrap_or_default())),
                        Some("missing") => server["message"].as_str().map(str::to_owned),
                        _ => None,
                    })
                });
                let message = failed.unwrap_or_else(|| {
                    if self.problems_project { t!("ide-problems-none-project") } else { t!("ide-problems-none-file") }
                });
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    ui.label(egui::RichText::new(message).color(p.text_secondary));
                });
                return;
            }
            for (path, problems) in &files {
                if self.problems_project {
                    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
                    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                    crate::file_icons::file_icon_at(
                        ui.painter(),
                        Rect::from_center_size(pos2(row.left() + 20.0, row.center().y), Vec2::splat(15.0)),
                        path,
                    );
                    let label = ui.painter().text(
                        pos2(row.left() + 34.0, row.center().y),
                        Align2::LEFT_CENTER,
                        &name,
                        theme::semibold(13.0),
                        p.text,
                    );
                    let count = ui.painter().text(
                        pos2(label.right() + 8.0, row.center().y),
                        Align2::LEFT_CENTER,
                        t!("ide-problems-count", count = problems.len()),
                        theme::regular(12.5),
                        p.text_secondary,
                    );
                    accessibility::label(ui, ui.scope_id().with(("problems-file", path)), label.union(count), || {
                        format!("{name} {}", t!("ide-problems-count", count = problems.len()))
                    });
                }
                let indent = if self.problems_project { 34.0 } else { 14.0 };
                for problem in problems {
                    let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                    if response.hovered() {
                        ui.painter().rect_filled(row.shrink2(vec2(4.0, 0.0)), 4, p.hover);
                    }
                    let severity = problem["severity"].as_str().unwrap_or_default();
                    ui.painter().circle_filled(
                        pos2(row.left() + indent + 5.0, row.center().y),
                        4.0,
                        p.problem(severity),
                    );
                    let message = problem["message"].as_str().unwrap_or_default().lines().next().unwrap_or_default();
                    let text = ui.painter().with_clip_rect(row).text(
                        pos2(row.left() + indent + 16.0, row.center().y),
                        Align2::LEFT_CENTER,
                        message,
                        theme::regular(13.0),
                        p.text,
                    );
                    ui.painter().with_clip_rect(row).text(
                        pos2(text.right() + 10.0, row.center().y),
                        Align2::LEFT_CENTER,
                        format!(":{}", problem["line"].as_u64().unwrap_or(1)),
                        theme::regular(12.5),
                        p.text_secondary,
                    );
                    accessibility::name_with(&response, egui::Role::ListItem, || {
                        let line = problem["line"].as_u64().unwrap_or(1);
                        t!("a11y-problem", severity = severity, message = message, line = line)
                    });
                    if response.clicked() {
                        go_to = Some((path.clone(), problem["start"].as_u64().unwrap_or(0) as usize));
                    }
                    response.context_menu(|ui| {
                        let place = serde_json::json!({ "path": path, "offset": problem["start"] });
                        let options = registry
                            .dispatch_unrecorded(session, "inspect.suppress.list", place)
                            .ok()
                            .and_then(|list| list.as_array().cloned())
                            .unwrap_or_default();
                        if options.is_empty() {
                            ui.add_enabled(false, egui::Button::new(t!("ide-problems-no-suppression")));
                        }
                        for option in options {
                            if ui.button(option["title"].as_str().unwrap_or_default()).clicked() {
                                suppress = Some(option);
                                ui.close();
                            }
                        }
                    });
                }
            }
        });
        if let Some(option) = suppress {
            self.dispatch(ui, registry, session, "inspect.suppress.apply", option);
        }
        if let Some((path, offset)) = go_to {
            if active.as_ref() != Some(&path) {
                self.dispatch(ui, registry, session, "file.document.open", serde_json::json!({ "path": path }));
            }
            let caret = serde_json::json!({ "selections": [{ "head": offset }] });
            if let Err(error) = registry.dispatch_unrecorded(session, "editor.selection.set", caret) {
                self.report(ui, error);
            }
            self.editor_view.focus();
        }
    }

    fn conflict_banner(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let conflict =
            session.state::<Workbench>().and_then(Workbench::active_document).is_some_and(Document::has_conflict);
        if !conflict {
            return;
        }
        let p = self.palette;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::BANNER), Sense::hover());
        let band =
            Rect::from_min_max(pos2(rect.left() + 6.0, rect.top()), pos2(rect.right() - 6.0, rect.bottom() - 4.0));
        ui.painter().rect_filled(band, 6, p.banner);
        let y = band.center().y;
        let text = ui.painter().text(
            pos2(band.left() + 14.0, y),
            Align2::LEFT_CENTER,
            t!("ide-disk-conflict"),
            theme::regular(13.0),
            p.text,
        );
        accessibility::announce(ui, ui.scope_id().with("disk-conflict"), text, || t!("ide-disk-conflict"));
        let mut x = text.right() + 20.0;
        if self.link(ui, &mut x, y, &t!("ide-disk-load"), "disk-load") {
            self.dispatch(ui, registry, session, "file.document.reloadFromDisk", serde_json::json!({}));
        }
        if self.link(ui, &mut x, y, &t!("ide-disk-keep"), "disk-keep") {
            self.dispatch(ui, registry, session, "file.document.keepMine", serde_json::json!({}));
        }
    }

    fn link(&self, ui: &mut Ui, x: &mut f32, y: f32, label: &str, salt: &str) -> bool {
        let galley = ui.painter().layout_no_wrap(label.to_owned(), theme::regular(13.0), self.palette.link);
        let rect = Rect::from_min_size(pos2(*x, y - galley.size().y / 2.0), galley.size());
        let response = ui
            .interact(rect, ui.scope_id().with(("banner-link", salt)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        accessibility::name(&response, egui::Role::Link, label);
        if response.hovered() {
            ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, self.palette.link));
        }
        ui.painter().galley(rect.min, galley, self.palette.link);
        *x = rect.right() + 20.0;
        response.clicked()
    }

    fn status_bar(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let rect = ui.max_rect();
        let y = rect.center().y - 1.0;
        let mut right = self.code_authors_toggle(ui, rect.right() - 8.0, y) - 10.0;
        let memory = self.memory_widget(ui, right, y);
        if memory < right {
            right = memory - 8.0;
        }
        let power_save = self.power_save_widget(ui, right, y);
        if power_save < right {
            right = power_save - 8.0;
        }
        if let Some(url) = self.overview.automation.as_ref().filter(|_| self.widget_shown(StatusWidget::McpServer)) {
            let galley = ui.painter().layout_no_wrap("MCP".to_owned(), theme::semibold(11.5), p.text);
            let pill =
                Rect::from_min_size(pos2(right - galley.size().x - 14.0, y - 9.0), vec2(galley.size().x + 14.0, 18.0));
            ui.painter().rect_filled(pill, 9, p.selection);
            ui.painter().galley(pill.center() - galley.size() / 2.0, galley, p.text);
            let response = ui
                .interact(pill, ui.scope_id().with("automation"), Sense::hover())
                .on_hover_text(t!("ide-automation-on", url = url.as_str()));
            accessibility::name_with(&response, egui::Role::Label, || t!("ide-automation-on", url = url.as_str()));
            right = pill.left() - 16.0;
        }
        right = self.errors_indicator(ui, session, right, y);
        right = self.macro_status(ui, session, right, y);
        if self.widget_shown(StatusWidget::ReadOnly) {
            right = self.read_only_widget(ui, registry, session, right, y) - 8.0;
        }
        right = selection_figures(ui, session, p, right, y);
        let active = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .filter(|_| zen_workbench::workspaces::tab_in_front(session).is_none());
        let has_document = active.is_some();
        let is_image = active.is_some_and(|document| document.image().is_some());
        let items: Vec<(String, Option<StatusWidget>)> =
            match (self.caret.or(has_document.then_some((1, 1))).filter(|_| !is_image), active) {
                (Some(caret), Some(document)) => status_items(session, document, caret)
                    .into_iter()
                    .filter(|(_, widget)| widget.is_none_or(|widget| self.widget_shown(widget)))
                    .collect(),
                _ => Vec::new(),
            };
        let item_of = |wanted: StatusWidget| items.iter().position(|(_, widget)| *widget == Some(wanted));
        let caret_item = item_of(StatusWidget::LineColumn);
        let separator_item = item_of(StatusWidget::LineSeparator);
        let encoding_item = item_of(StatusWidget::FileEncoding);
        let indent_item = item_of(StatusWidget::Indentation);
        let editor_config_styled =
            active.is_some_and(|document| zen_workbench::editor_config::style_for(session, document.path()).is_some());
        let current_separator = active.map(Document::line_separator);
        let mut chosen_separator = None;
        let mut indent_action = None;
        let mut encoding_list = None;
        for (index, (item, _)) in items.into_iter().enumerate() {
            let galley = ui.painter().layout_no_wrap(item, theme::regular(12.5), p.text_secondary);
            let text = Align2::RIGHT_CENTER.anchor_size(pos2(right, y), galley.size());
            let shown = Arc::clone(&galley);
            ui.painter().galley(text.min, galley, p.text_secondary);
            let mut left_edge = text.left();
            if Some(index) == indent_item && editor_config_styled {
                let icon = Rect::from_center_size(pos2(text.left() - 10.0, y), Vec2::splat(13.0));
                let area = text.union(icon).expand(4.0);
                left_edge = icon.left();
                let response = ui.interact(area, ui.scope_id().with("status-indent"), Sense::click());
                if response.hovered() {
                    ui.painter().rect_filled(area, 4, p.hover);
                    ui.painter().galley(text.min, Arc::clone(&shown), p.text_secondary);
                }
                editor_config_view::mark(ui.painter(), icon, p.text_secondary);
                hint(ui, &response, &t!("ide-status-indent-editorconfig-hint"), None, Side::Left);
                accessibility::button(&response, &t!("ide-status-indent-editorconfig", indent = shown.text()));
                crate::components::popup_menu(&response, |ui| {
                    for (action, separated) in [
                        ("project.editorConfig.open", false),
                        ("view.editorConfig.showFiles", false),
                        ("view.editorConfig.disable", true),
                    ] {
                        if separated {
                            ui.separator();
                        }
                        if crate::components::menu_item(ui, keymap_page::action_title(action)) {
                            indent_action = Some(action);
                            ui.close();
                        }
                    }
                });
            } else if Some(index) == indent_item {
                let response = ui.interact(text.expand(4.0), ui.scope_id().with("status-indent"), Sense::click());
                self.indent_widget(ui, registry, session, &response);
            } else if Some(index) == separator_item {
                let response =
                    ui.interact(text.expand(4.0), ui.scope_id().with("status-line-separator"), Sense::click());
                if response.hovered() {
                    ui.painter().rect_filled(text.expand(4.0), 4, p.hover);
                    ui.painter().galley(text.min, Arc::clone(&shown), p.text_secondary);
                }
                hint(ui, &response, &t!("ide-status-line-separator"), None, Side::Left);
                accessibility::button(&response, &t!("ide-status-line-separator-named", separator = shown.text()));
                crate::components::popup_menu(&response, |ui| {
                    for separator in zen_text::LineSeparator::ALL {
                        let title = keymap_page::action_title(&format!("file.lineSeparator.set/{}", separator.id()));
                        if Some(separator) == current_separator {
                            crate::components::menu_item_disabled(ui, title, None);
                        } else if crate::components::menu_item(ui, title) {
                            chosen_separator = Some(separator);
                            ui.close();
                        }
                    }
                });
            } else if Some(index) == encoding_item {
                let response = ui.interact(text.expand(4.0), ui.scope_id().with("status-encoding"), Sense::click());
                if response.hovered() {
                    ui.painter().rect_filled(text.expand(4.0), 4, p.hover);
                    ui.painter().galley(text.min, Arc::clone(&shown), p.text_secondary);
                }
                hint(ui, &response, &t!("ide-status-encoding"), None, Side::Left);
                accessibility::button(&response, &t!("ide-status-encoding-named", charset = shown.text()));
                if response.clicked() {
                    encoding_list = Some(text.left_top());
                }
            } else if Some(index) != caret_item {
                accessibility::label(ui, ui.scope_id().with(("status-item", index)), text, || shown.text().to_owned());
            }
            if Some(index) == caret_item {
                let response = ui.interact(text.expand(4.0), ui.scope_id().with("status-caret"), Sense::click());
                hint(
                    ui,
                    &response,
                    &t!("ide-goto-title"),
                    crate::keymap::shortcut("view.gotoLine.open").as_deref(),
                    Side::Left,
                );
                accessibility::button(&response, shown.text());
                accessibility::edit(&response, |node| node.set_description(t!("ide-goto-title")));
                if response.clicked() {
                    self.open_goto();
                }
            }
            right = left_edge - 16.0;
        }
        if let Some(action) = indent_action {
            self.run_action(ui, registry, session, action, serde_json::json!({}));
        }
        if let Some(at) = encoding_list {
            self.open_encoding_popup(registry, session, at);
        }
        if let Some(separator) = chosen_separator {
            self.dispatch(
                ui,
                registry,
                session,
                "file.lineSeparator.set",
                serde_json::json!({ "separator": separator }),
            );
        }
        let left = if self.widget_shown(StatusWidget::LanguageServices) {
            self.language_server_widget(ui, registry, session, right, y)
        } else {
            right
        };
        let left = self.tasks_indicator(ui, registry, session, left, y);
        if self.overview.settings.navigation_bar == NavigationBarPlacement::StatusBar {
            let bar = Rect::from_min_max(pos2(rect.left() + 8.0, y - 12.0), pos2(left - 12.0, y + 12.0));
            if bar.width() > 0.0 {
                self.navigation_bar_in(ui, bar, registry, session, true);
                let segments = self.navigation_bar.segments();
                accessibility::label(ui, ui.scope_id().with("status-path"), bar, || {
                    navigation_bar::spoken_path(&segments)
                });
            }
        }
        self.status_bar_context(ui, rect);
        self.status_menu(ui);
    }

    fn dispatch(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        command: &str,
        params: serde_json::Value,
    ) {
        let answer = zen_workbench::vcs::off_the_frame(session, |session| registry.dispatch(session, command, params));
        if let Err(error) = answer {
            self.report(ui, error);
        }
    }

    fn report(&mut self, ui: &Ui, error: zen_core::DispatchError) {
        self.report_at(ui.ctx(), error);
    }

    fn report_at(&mut self, ctx: &egui::Context, error: zen_core::DispatchError) {
        log::warn!("{error}");
        if let Some(dialog) = transaction_dialog::dialog(&error) {
            self.open_transaction_dialog(ctx, dialog);
            return;
        }
        if debugger_notice::is_debugger_missing(&error)
            && let Some(offer) = self.product.debugger
        {
            self.notify(debugger_notice::notification(&offer));
            return;
        }
        if let zen_core::DispatchError::Failed { error, .. } = &error
            && error.code.as_deref() == Some(zen_workbench::file_types::UNKNOWN_FILE_TYPE)
            && let Some(path) =
                error.data.as_ref().and_then(|data| data.get("path")).and_then(serde_json::Value::as_str)
        {
            self.open_associate_dialog(PathBuf::from(path));
            return;
        }
        let (message, group) = match error {
            zen_core::DispatchError::Failed { error, .. }
                if error.code.as_deref() == Some(zen_workbench::vcs::CONFLICTS) =>
            {
                return;
            }
            zen_core::DispatchError::Failed { error, .. }
                if error.code.as_deref() == Some(zen_workbench::bookmarks::FOLDER_BOOKMARK)
                    && self.reveal_folder(error.data.as_ref().and_then(|data| data.get("path"))) =>
            {
                return;
            }
            zen_core::DispatchError::Failed { error, id } => (error.message, Group::of_command(id.as_str())),
            zen_core::DispatchError::Unavailable { .. } => return,
            _ => (t!("ide-action-failed"), None),
        };
        let group = group.unwrap_or(Group::Errors);
        self.notify(NewNotification::message(group, Kind::Error, message));
    }

    fn tree_bookmark_targets(&self) -> Vec<PathBuf> {
        let mut paths = self.selection.items(&self.visible_rows);
        paths.retain(|path| !libraries_tree::is_group(path) && path.exists());
        if let Some(lead) = self.selection.lead().filter(|lead| paths.contains(lead)).cloned() {
            paths.retain(|path| *path != lead);
            paths.push(lead);
        }
        paths
    }

    fn reveal_folder(&mut self, path: Option<&serde_json::Value>) -> bool {
        let folder = path.and_then(serde_json::Value::as_str).map(PathBuf::from);
        let (Some(folder), Some(project)) = (folder, self.visible_rows.first().cloned()) else { return false };
        if self.select_paths(&project, vec![folder]).is_err() {
            return false;
        }
        self.show_tool("project");
        true
    }

    fn post(&mut self, ui: &Ui, message: String) {
        self.post_at(ui.ctx(), message);
    }

    fn post_at(&mut self, _ctx: &egui::Context, message: String) {
        let group = self.notifications.group();
        self.notify(NewNotification::message(group, Kind::Info, message));
    }
}

impl IdeView {
    fn toggle_code_authors(&mut self) -> bool {
        let mut settings = self.overview.settings.clone();
        let shown = !(settings.code_vision.enabled && settings.code_vision.authors);
        settings.code_vision.authors = shown;
        if shown {
            settings.code_vision.enabled = true;
        }
        self.overview.settings = settings.clone();
        self.requests.push(Request::Settings(Box::new(settings)));
        shown
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code, reason = "the macOS main menu uses it"))]
    fn separator_choosable(&self, session: &Session, command: &str, params: &serde_json::Value) -> bool {
        if command != "file.lineSeparator.set" || self.tree_focused {
            return true;
        }
        let current = session.state::<Workbench>().and_then(Workbench::active_document).map(Document::line_separator);
        let chosen = params.get("separator").and_then(serde_json::Value::as_str);
        current.is_none_or(|current| Some(current.id()) != chosen)
    }

    fn quick_switch_groups(&self, registry: &Registry, session: &mut Session) -> Vec<quick_switch::Group> {
        let themes = registry.dispatch(session, "ui.theme.list", serde_json::json!({})).ok();
        let keymaps = registry.dispatch(session, "ui.keymap.list", serde_json::json!({})).ok();
        let view_modes = quick_switch::VIEW_MODES
            .iter()
            .map(|action| (*action, self.appearance_title(action).unwrap_or_default()))
            .collect();
        quick_switch::groups(&quick_switch::Sources {
            themes: themes.as_ref(),
            keymaps: keymaps.as_ref(),
            code_style_scheme: self.overview.settings.code_style_scheme,
            view_modes,
            ide_zoom: self.ide_zoom(),
        })
    }

    fn quick_switch_run(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, run: quick_switch::Run) {
        match run {
            quick_switch::Run::Command(command, params) => self.dispatch(ui, registry, session, command, params),
            quick_switch::Run::Action(action, params) => self.run_action(ui, registry, session, action, params),
            quick_switch::Run::CodeStyleScheme(scheme) => {
                let mut settings = self.overview.settings.clone();
                settings.code_style_scheme = scheme;
                self.overview.settings = settings.clone();
                self.requests.push(Request::Settings(Box::new(settings)));
            }
            quick_switch::Run::IdeZoom(percent) => {
                self.set_ide_zoom(percent);
            }
        }
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn action_checked(&self, action: &str, session: &Session) -> bool {
        if let Some(mode) = tool_windows::ViewMode::of_action(action) {
            return self.tool_windows.active().and_then(|id| self.tool_windows.mode_of(id)) == Some(mode);
        }
        let document = || session.state::<Workbench>().and_then(Workbench::active_document);
        match action {
            "view.codeVision.authors.toggle" => self.code_authors_shown(),
            "view.project.excludedFiles" => self.show_excluded,
            "editor.whitespaces.toggle" => document()
                .is_some_and(|document| zen_workbench::visual_aids::shows_whitespaces(session.settings(), document)),
            "editor.indentGuides.toggle" => document()
                .is_some_and(|document| zen_workbench::visual_aids::shows_indent_guides(session.settings(), document)),
            "editor.lineNumbers.toggle" => self.editor_view.line_numbers_on,
            _ if action.starts_with("view.navigationBar.") => self.navigation_bar_checked(action),
            "view.powerSaveMode.toggle" => self.power_save_on(),
            _ if action.starts_with("view.statusBarWidgets.open/") => action
                .strip_prefix("view.statusBarWidgets.open/")
                .and_then(StatusWidget::from_id)
                .is_some_and(|widget| self.widget_shown(widget)),
            _ => false,
        }
    }

    fn code_authors_shown(&self) -> bool {
        self.overview.settings.code_vision.enabled && self.overview.settings.code_vision.authors
    }

    fn code_authors_toggle(&mut self, ui: &mut Ui, right: f32, y: f32) -> f32 {
        let p = self.palette;
        let on = self.code_authors_shown();
        let rect = Rect::from_center_size(pos2(right - 11.0, y), Vec2::splat(22.0));
        let response = ui.interact(rect, ui.scope_id().with("status-code-authors"), Sense::click());
        if on {
            ui.painter().rect_filled(rect, 4, p.selection);
        } else if response.hovered() {
            ui.painter().rect_filled(rect, 4, p.hover);
        }
        let color = if on { p.text } else { p.text_secondary };
        code_vision::person_icon(ui.painter(), rect.shrink(4.0), color);
        let name = keymap_page::action_title("view.codeVision.authors.toggle");
        hint(ui, &response, &name, crate::keymap::shortcut("view.codeVision.authors.toggle").as_deref(), Side::Left);
        accessibility::button(&response, &name);
        accessibility::pressed(&response, on);
        if response.clicked() {
            self.toggle_code_authors();
        }
        rect.left()
    }
}

fn status_items(
    session: &Session,
    document: &Document,
    (line, column): (usize, usize),
) -> Vec<(String, Option<StatusWidget>)> {
    let unit = zen_workbench::project_settings::indent_unit_of(session, document);
    let indent = if unit == "\t" { t!("ide-status-tab") } else { t!("ide-status-spaces", count = unit.len()) };
    vec![
        (std::env::consts::ARCH.to_owned() + "-" + std::env::consts::OS, None),
        (indent, Some(StatusWidget::Indentation)),
        (document.charset().to_owned(), Some(StatusWidget::FileEncoding)),
        (document.line_separator().label().to_owned(), Some(StatusWidget::LineSeparator)),
        (format!("{line}:{column}"), Some(StatusWidget::LineColumn)),
    ]
}

fn text_field_focused(ctx: &egui::Context) -> bool {
    ctx.memory(|memory| memory.focused()).is_some_and(|id| egui::text_edit::TextEditState::load(ctx, id).is_some())
}

fn draw_notice(ui: &mut Ui, p: &IdePalette, notice: &mut Option<(String, f64)>, bottom: f32) {
    let Some((message, posted)) = notice.as_ref() else { return };
    if ui.input(|input| input.time) - posted > 5.0 {
        *notice = None;
        return;
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    let screen = ui.ctx().content_rect();
    let galley = ui.painter().layout(message.clone(), theme::regular(13.0), p.text, 380.0);
    let size = galley.size() + vec2(28.0, 20.0);
    let rect = Rect::from_min_size(pos2(screen.right() - size.x - 24.0, screen.bottom() - size.y - bottom), size);
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, ui.scope_id().with("ide-notice")));
    painter.rect_filled(rect, 8, p.hover);
    painter.galley(rect.min + vec2(14.0, 10.0), galley, p.text);
    accessibility::announce(ui, ui.scope_id().with("ide-notice"), rect, || message.clone());
}

fn selection_figures(ui: &Ui, session: &Session, p: IdePalette, right: f32, y: f32) -> f32 {
    let pass = ui.ctx().cumulative_pass_nr();
    let Some(figures) = session
        .state::<database_view::SelectionFigures>()
        .filter(|figures| figures.pass + 1 >= pass && !figures.text.is_empty())
    else {
        return right;
    };
    let galley = ui.painter().layout_no_wrap(figures.text.clone(), theme::regular(12.5), p.text_secondary);
    let text = Align2::RIGHT_CENTER.anchor_size(pos2(right, y), galley.size());
    let shown = galley.text().to_owned();
    ui.painter().galley(text.min, galley, p.text_secondary);
    accessibility::label(ui, ui.scope_id().with("status-database-figures"), text, || shown);
    text.left() - 16.0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Right,
    Left,
    Below,
}

fn hint(ui: &Ui, response: &egui::Response, name: &str, detail: Option<&str>, side: Side) {
    match (name.is_empty(), detail) {
        (false, detail) => {
            accessibility::button(response, name);
            accessibility::describe(response, detail.unwrap_or_default());
        }
        (true, Some(detail)) => accessibility::button(response, detail),
        (true, None) => {}
    }
    if !response.hovered() {
        return;
    }
    let p = IdePalette::of(ui.ctx());
    let font = theme::regular(13.0);
    let name = (!name.is_empty()).then(|| ui.painter().layout_no_wrap(name.to_owned(), font.clone(), p.text));
    let detail = detail.map(|detail| ui.painter().layout_no_wrap(detail.to_owned(), font.clone(), p.text_secondary));
    let gap = if name.is_some() && detail.is_some() { 10.0 } else { 0.0 };
    let width = name.as_ref().map_or(0.0, |galley| galley.size().x)
        + gap
        + detail.as_ref().map_or(0.0, |galley| galley.size().x);
    let size = vec2(width + 22.0, 30.0);
    let anchor = response.rect;
    let screen = ui.ctx().content_rect();
    let min = match side {
        Side::Right => pos2(anchor.right() + 8.0, anchor.center().y - size.y / 2.0),
        Side::Left => pos2(anchor.left() - 8.0 - size.x, anchor.center().y - size.y / 2.0),
        Side::Below => pos2(
            (anchor.center().x - size.x / 2.0)
                .clamp(screen.left() + 4.0, (screen.right() - size.x - 4.0).max(screen.left() + 4.0)),
            anchor.bottom() + 6.0,
        ),
    };
    let plate = Rect::from_min_size(min, size);
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, response.id.with("hint")));
    painter.rect(plate, 7, p.popup, Stroke::new(1.0, p.popup_border), StrokeKind::Inside);
    let mut x = plate.left() + 11.0;
    if let Some(name) = name {
        let w = name.size().x;
        painter.galley(pos2(x, plate.center().y - name.size().y / 2.0), name, p.text);
        x += w + gap;
    }
    if let Some(detail) = detail {
        painter.galley(pos2(x, plate.center().y - detail.size().y / 2.0), detail, p.text_secondary);
    }
}

fn in_development(ui: &Ui, response: &egui::Response, name: &str, side: Side) -> bool {
    hint(ui, response, name, Some(&t!("ide-in-development")), side);
    response.clicked()
}

struct RowResponse {
    clicked: bool,
    remove: bool,
}

fn paths_to_compare(session: &Session, project: &Path, changes: Vec<PathBuf>) -> Vec<PathBuf> {
    let outside = session.state::<Workbench>().into_iter().flat_map(|workbench| {
        workbench.documents().iter().map(Document::path).filter(|path| !path.starts_with(project)).map(Path::to_owned)
    });
    let outside: Vec<PathBuf> = outside.collect();
    changes.into_iter().chain(outside).collect()
}

fn project_row(
    ui: &mut Ui,
    p: &IdePalette,
    path: &Path,
    current: bool,
    removable: Option<bool>,
    facts: welcome::ProjectFacts,
) -> RowResponse {
    let name = path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
    let branch = removable.and(facts.branch);
    let exists = facts.exists;
    let x_offset = 2.0 + 8.0 + 22.0 + 10.0;
    let text_width = (ui.available_width() - x_offset - if removable.is_some() { 40.0 } else { 12.0 }).max(40.0);
    let line = |text: String, font: FontId, color: Color32| {
        let mut job = LayoutJob::single_section(text, TextFormat::simple(font, color));
        job.wrap = egui::text::TextWrapping {
            max_width: text_width,
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('…'),
        };
        ui.painter().layout_job(job)
    };
    let name_galley = line(name.clone(), theme::regular(14.0), if exists { p.text } else { p.text_secondary });
    let path_galley = line(home_relative(path), theme::regular(12.5), p.text_secondary);
    let branch_galley = branch.map(|branch| line(branch, theme::regular(12.5), p.text_secondary));
    const GAP: f32 = 3.0;
    let text_height = name_galley.size().y
        + GAP
        + path_galley.size().y
        + branch_galley.as_ref().map_or(0.0, |galley| GAP + galley.size().y);
    let height = (text_height + 18.0).max(44.0);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    accessibility::item(&response, egui::Role::ListBoxOption, current, &name);
    accessibility::describe(&response, path_galley.text());
    let row = rect.shrink2(vec2(2.0, 1.0));
    let in_menu = removable.is_none();
    if response.hovered() {
        ui.painter().rect_filled(row, 6, if in_menu { p.selection } else { p.hover });
    } else if current {
        ui.painter().rect_filled(row, 6, if in_menu { p.popup_hover } else { p.selection });
    }
    let top = row.center().y - text_height / 2.0;
    let badge =
        Rect::from_min_size(pos2(row.left() + 8.0, top + (name_galley.size().y - 22.0) / 2.0 + 2.0), Vec2::splat(22.0));
    project_badge(ui, p, badge, &name);
    let x = badge.right() + 10.0;
    let mut y = top;
    let name_height = name_galley.size().y;
    ui.painter().galley(pos2(x, y), name_galley, p.text);
    y += name_height + GAP;
    let path_height = path_galley.size().y;
    ui.painter().galley(pos2(x, y), path_galley, p.text_secondary);
    y += path_height + GAP;
    if let Some(galley) = branch_galley {
        glyph_branch(
            ui.painter(),
            Rect::from_min_size(pos2(x, y + (galley.size().y - 11.0) / 2.0), Vec2::splat(11.0)),
            p.text_secondary,
        );
        ui.painter().galley(pos2(x + 17.0, y), galley, p.text_secondary);
    }
    let mut remove = false;
    let mut menu_clicked = false;
    if removable.is_some() && (response.hovered() || current) {
        let more = Rect::from_center_size(pos2(row.right() - 22.0, row.center().y), Vec2::splat(24.0));
        let more_response = ui.interact(more, ui.scope_id().with(("project-more", path)), Sense::click());
        accessibility::button_with(&more_response, || t!("a11y-options"));
        hover(ui, &more_response, p);
        glyph_more(ui.painter(), more.shrink(6.0), p.text_secondary);
        menu_clicked = more_response.clicked();
        egui::Popup::menu(&more_response).show(|ui| {
            if ui.button(t!("ide-recent-remove")).clicked() {
                remove = true;
            }
        });
    }
    RowResponse { clicked: response.clicked() && !menu_clicked && exists, remove }
}

fn hover(ui: &Ui, response: &egui::Response, p: &IdePalette) {
    if response.hovered() {
        ui.painter().rect_filled(response.rect, 6, p.hover);
    }
}

fn project_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}

fn project_badge(ui: &Ui, p: &IdePalette, rect: Rect, name: &str) {
    ui.painter().rect_filled(rect, 5, p.project(name));
    let initials = project_initials(name);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        initials.to_uppercase(),
        theme::bold(rect.height() * 0.48),
        p.project_text,
    );
}

fn project_glow(painter: &egui::Painter, rect: Rect, p: &IdePalette, color: Color32) {
    let tint = |amount: f32| {
        let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * amount).round() as u8;
        Color32::from_rgb(mix(p.frame.r(), color.r()), mix(p.frame.g(), color.g()), mix(p.frame.b(), color.b()))
    };
    let stops = [(0.0, 0.16), (0.16, 0.30), (0.36, 0.12), (0.6, 0.0)];
    let mut mesh = egui::Mesh::default();
    for (index, (fraction, strength)) in stops.iter().enumerate() {
        let x = rect.left() + rect.width() * fraction;
        let color = tint(*strength);
        mesh.colored_vertex(pos2(x, rect.top()), color);
        mesh.colored_vertex(pos2(x, rect.bottom()), color);
        if index > 0 {
            let base = (index as u32 - 1) * 2;
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base + 1, base + 3, base + 2);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

fn project_initials(name: &str) -> String {
    let mut words: Vec<char> = Vec::new();
    let mut previous: Option<char> = None;
    for c in name.chars() {
        let boundary = match previous {
            None => true,
            Some(p) => matches!(p, '-' | '_' | '.' | ' ') || (p.is_lowercase() && c.is_uppercase()),
        };
        if boundary && c.is_alphanumeric() {
            words.push(c);
        }
        previous = Some(c);
    }
    words.iter().take(2).collect::<String>().to_uppercase()
}

fn clipboard_image() -> Option<ClipboardImage> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let image = arboard::Clipboard::new().ok()?.get().image().ok()?;
        (image.width > 0 && image.height > 0).then(|| ClipboardImage {
            width: image.width,
            height: image.height,
            rgba: image.bytes.into_owned(),
        })
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

fn save_image_for_paste(image: &ClipboardImage, name: &str) -> Result<PathBuf, String> {
    let name = if name.to_ascii_lowercase().ends_with(".png") { name.to_owned() } else { format!("{name}.png") };
    if name.contains(['/', '\\']) {
        return Err(t!("ide-paste-image-bad-name"));
    }
    let folder = std::env::temp_dir().join(format!("zen-paste-{}-{}", std::process::id(), image.width * image.height));
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let path = folder.join(name);
    let file = std::fs::File::create(&path).map_err(|error| error.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), image.width as u32, image.height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
    writer.write_image_data(&image.rgba).map_err(|error| error.to_string())?;
    Ok(path)
}

fn file_manager_label() -> String {
    if cfg!(target_os = "macos") {
        t!("ide-tree-open-in-finder")
    } else if cfg!(target_os = "windows") {
        t!("ide-tree-open-in-explorer")
    } else {
        t!("ide-tree-open-in-files")
    }
}

const MENU_ICON: f32 = 16.0;

fn menu_row(ui: &mut Ui, p: &IdePalette, icon: Option<Glyph>, text: String, keys: &str, enabled: bool) -> bool {
    let id = egui::IdSalt::new("menu-row-icon");
    let button =
        egui::Button::new((egui::Atom::custom(id, Vec2::splat(MENU_ICON)), text)).shortcut_text(keys.to_owned());
    let response = ui.add_enabled_ui(enabled, |ui| button.atom_ui(ui)).inner;
    if let (Some(icon), Some(rect)) = (icon, response.rect(id)) {
        let color = if enabled { p.text_secondary } else { p.text_secondary.gamma_multiply(0.45) };
        icon(ui.painter(), rect.shrink(1.5), color);
    }
    response.clicked()
}

fn excluded_folder_glyph(painter: &egui::Painter, rect: Rect, _color: Color32) {
    crate::file_icons::folder_icon(painter, rect, "", true, false);
}

fn resource_root_glyph(painter: &egui::Painter, rect: Rect, _color: Color32) {
    crate::file_icons::folder_icon_of(painter, rect, crate::file_icons::FolderKind::ResourceRoot, false);
}

fn menu_label<'a>(text: String) -> (egui::Atom<'a>, String) {
    (egui::Atom::custom(egui::IdSalt::new("menu-row-space"), Vec2::splat(MENU_ICON)), text)
}

fn plain_notes(notes: &str) -> Vec<String> {
    notes
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let line = line
                .strip_prefix("- ")
                .or_else(|| line.strip_prefix("* "))
                .map_or(line.to_owned(), |item| format!("• {item}"));
            line.replace("**", "").replace('`', "")
        })
        .collect()
}

fn folder_of(path: &Path) -> PathBuf {
    if path.is_dir() { path.to_owned() } else { path.parent().map_or_else(|| path.to_owned(), Path::to_owned) }
}

fn home_relative(path: &Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(home) if path.starts_with(&home) => format!("~/{}", path.strip_prefix(&home).unwrap_or(path).display()),
        _ => path.display().to_string(),
    }
}

fn git_branch(project: &Path) -> Option<String> {
    let head = std::fs::read_to_string(project.join(".git").join("HEAD")).ok()?;
    let head = head.trim();
    Some(head.strip_prefix("ref: refs/heads/").map_or_else(|| head.chars().take(8).collect(), str::to_owned))
}

fn dialog_dispatch(
    registry: &Registry,
    session: &mut Session,
    agent: bool,
    id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let answer = if agent {
        zen_core::macros::as_agent(session, |session| registry.dispatch(session, id, params))
    } else {
        registry.dispatch(session, id, params)
    };
    answer.map_err(|error| match error {
        zen_core::DispatchError::Failed { error, .. } => error.message,
        other => other.to_string(),
    })
}

fn dialog_frame(p: &IdePalette) -> egui::Frame {
    p.popup_frame().corner_radius(12).inner_margin(egui::Margin::same(20)).shadow(egui::Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 0,
        color: p.popup_shadow,
    })
}

fn excluded_entry(entry: &zen_vfs::Entry, depth: usize, in_project: bool, project: &zen_index::Exclusions) -> bool {
    let built_in = entry.is_dir && zen_vfs::excluded_folders::is_excluded(&entry.path);
    depth > 0 && (built_in || (in_project && project.excludes_entry(&entry.path)))
}

#[cfg(test)]
pub(crate) fn trusting_everything() -> zen_workbench::build_tools::BuildEnv {
    let env = zen_workbench::build_tools::BuildEnv::default();
    let _ = env.set_settings(zen_workbench::trust::LOCATIONS, &[PathBuf::from("/")]);
    env
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use egui::{Vec2, vec2};

    use super::{excluded_entry, locked_scroll, project_initials, status_items};

    #[test]
    fn the_tree_marks_built_in_and_project_exclusions() {
        let entry = |path: &str, is_dir: bool| zen_vfs::Entry {
            path: PathBuf::from(path),
            name: path.rsplit('/').next().unwrap().to_owned(),
            is_dir,
            is_symlink: false,
        };
        let project = zen_index::Exclusions::new(&[PathBuf::from("/p/gen")], &["*.tmp".to_owned()]);
        let none = zen_index::Exclusions::default();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        for folder in ["node_modules", "vendor", "build", "dist", "target"] {
            std::fs::create_dir(dir.path().join(folder)).unwrap();
        }
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert!(excluded_entry(&entry(&format!("{root}/node_modules"), true), 1, true, &none), "beside package.json");
        assert!(!excluded_entry(&entry(&format!("{root}/node_modules"), false), 1, true, &none));
        for folder in ["vendor", "build", "dist", "target"] {
            assert!(!excluded_entry(&entry(&format!("{root}/{folder}"), true), 1, true, &none), "{folder} by its name");
        }
        assert!(!excluded_entry(&entry("/p/gen", true), 1, true, &none));
        assert!(excluded_entry(&entry("/p/gen", true), 1, true, &project));
        assert!(excluded_entry(&entry("/p/src/a.tmp", false), 2, true, &project));
        assert!(!excluded_entry(&entry("/p/src/a.go", false), 2, true, &project));
        assert!(!excluded_entry(&entry("/p/gen", true), 1, false, &project), "not in External Libraries");
        assert!(!excluded_entry(&entry("/p/gen", true), 0, true, &project), "never the project's root");
    }

    #[test]
    fn the_status_bar_shows_the_indent_the_encoding_and_a_read_only_file() {
        let mut registry = zen_core::Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.go"), "package main\n").unwrap();
        std::fs::write(dir.path().join("a.json"), "{}\n").unwrap();
        std::fs::write(dir.path().join("odd.txt"), [b'a', 0xff, b'\n']).unwrap();
        std::fs::write(dir.path().join("win.txt"), "a\r\nb\n").unwrap();
        std::fs::write(dir.path().join("mac.txt"), "a\rb\r").unwrap();
        let mut session = zen_core::Session::new();
        let texts = |session: &mut zen_core::Session, name: &str| {
            let path = dir.path().join(name);
            registry.dispatch(session, "file.document.open", serde_json::json!({ "path": path })).unwrap();
            let document = session.state::<zen_workbench::Workbench>().unwrap().active_document().unwrap();
            status_items(session, document, (1, 1)).into_iter().map(|(text, _)| text).collect::<Vec<_>>()
        };
        let go = texts(&mut session, "main.go");
        assert_eq!(go[1..], [zen_i18n::t!("ide-status-tab"), "UTF-8".to_owned(), "LF".to_owned(), "1:1".to_owned()]);
        assert_eq!(texts(&mut session, "a.json")[1], zen_i18n::t!("ide-status-spaces", count = 2));
        let odd = texts(&mut session, "odd.txt");
        assert_eq!(texts(&mut session, "win.txt")[3], "CRLF", "a mixed file shows its first separator");
        assert_eq!(texts(&mut session, "mac.txt")[3], "CR");
        assert_eq!(odd.len(), go.len(), "the lock widget, not an item, shows a read-only file");
        texts(&mut session, "odd.txt");
        let document = session.state::<zen_workbench::Workbench>().unwrap().active_document().unwrap();
        assert!(super::read_only::LockState::of(document).closed);
    }

    #[test]
    fn a_disk_change_compares_what_changed_and_files_the_watcher_cannot_see() {
        let mut registry = zen_core::Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (project, outside) = (dir.path().join("project"), dir.path().join("outside.go"));
        std::fs::create_dir(&project).unwrap();
        let inside = project.join("main.go");
        std::fs::write(&inside, "package main\n").unwrap();
        std::fs::write(&outside, "package main\n").unwrap();
        let mut session = zen_core::Session::new();
        registry.dispatch(&mut session, "file.project.open", serde_json::json!({ "path": project })).unwrap();
        for file in [&inside, &outside] {
            registry.dispatch(&mut session, "file.document.open", serde_json::json!({ "path": file })).unwrap();
        }
        let changed = project.join("other.go");
        assert_eq!(super::paths_to_compare(&session, &project, vec![changed.clone()]), [changed, outside.clone()]);

        std::fs::write(&inside, "package missed\n").unwrap();
        let paths = super::paths_to_compare(&session, &project, vec![project.clone()]);
        assert_eq!(paths, [project.clone(), outside]);
        registry.dispatch(&mut session, "file.document.syncWithDisk", serde_json::json!({ "paths": paths })).unwrap();
        let workbench = session.state::<zen_workbench::Workbench>().unwrap();
        let document = workbench.documents().iter().find(|document| document.path() == inside).unwrap();
        assert_eq!(document.text(), "package missed\n", "events the system dropped still reload the open files");
    }

    #[test]
    fn the_status_bar_shows_what_tab_inserts() {
        let mut registry = zen_core::Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in [
            ("main.go", "package main\n\nfunc main() {\n    x()\n}\n"),
            ("tabbed.txt", "a\n\tb\n\tc\n\td\n"),
            ("plain.txt", "a\n"),
            ("a.yaml", "a:\n  b: 1\n"),
            ("b.yaml", "a:\n  b: 1\n"),
            ("c.yaml", "a:\n  b: 1\n"),
            ("d.yaml", "a:\n  b: 1\n"),
            (".idea/codeStyles/codeStyleConfig.xml", IDEA_CONFIG),
            (".idea/codeStyles/Project.xml", IDEA_YAML_TABS),
        ] {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let mut session = zen_core::Session::new();
        let indent = |session: &mut zen_core::Session, name: &str| {
            let path = dir.path().join(name);
            registry.dispatch(session, "file.document.open", serde_json::json!({ "path": path })).unwrap();
            let document = session.state::<zen_workbench::Workbench>().unwrap().active_document().unwrap();
            let shown = status_items(session, document, (1, 1))[1].0.clone();
            registry.dispatch(session, "editor.edit.tab", serde_json::json!({})).unwrap();
            let text =
                session.state::<zen_workbench::Workbench>().unwrap().active_document().unwrap().snapshot().to_string();
            let typed = text.chars().take_while(|c| c.is_whitespace()).collect::<String>();
            (shown, typed)
        };
        let tab = zen_i18n::t!("ide-status-tab");
        let spaces = |count: usize| zen_i18n::t!("ide-status-spaces", count = count);
        assert_eq!(indent(&mut session, "main.go"), (tab.clone(), "\t".to_owned()));
        assert_eq!(indent(&mut session, "tabbed.txt"), (tab.clone(), "\t".to_owned()));
        assert_eq!(indent(&mut session, "plain.txt"), (spaces(4), "    ".to_owned()));
        assert_eq!(indent(&mut session, "a.yaml"), (spaces(2), "  ".to_owned()));

        session
            .state_mut::<zen_workbench::project_settings::ProjectSettingsState>()
            .set_providers(vec![std::sync::Arc::new(zen_jb_import::IdeaProject::with_user_home(None))]);
        registry.dispatch(&mut session, "file.project.open", serde_json::json!({ "path": dir.path() })).unwrap();
        assert_eq!(indent(&mut session, "b.yaml"), (tab.clone(), "\t".to_owned()), "the project's .idea code style");

        std::fs::write(dir.path().join(".editorconfig"), "[*.yaml]\nindent_style = space\nindent_size = 3\n").unwrap();
        let changed = serde_json::json!({ "paths": [dir.path().join(".editorconfig")] });
        registry.dispatch(&mut session, "project.settings.refresh", changed).unwrap();
        assert_eq!(indent(&mut session, "c.yaml"), (spaces(3), "   ".to_owned()), ".editorconfig above .idea");
        session.settings_mut().set(zen_workbench::editor_config::ENABLED, false).unwrap();
        assert_eq!(indent(&mut session, "d.yaml"), (tab, "\t".to_owned()), "EditorConfig support off");
    }

    #[test]
    fn the_status_bar_shows_the_detected_indent_until_detection_is_off() {
        let mut registry = zen_core::Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, "a\n  b\n  c\n    d\n").unwrap();
        let mut session = zen_core::Session::new();
        registry.dispatch(&mut session, "file.document.open", serde_json::json!({ "path": path })).unwrap();
        let shown = |session: &zen_core::Session| {
            let document = session.state::<zen_workbench::Workbench>().unwrap().active_document().unwrap();
            status_items(session, document, (1, 1))[1].0.clone()
        };
        assert_eq!(shown(&session), zen_i18n::t!("ide-status-spaces", count = 2));
        let mut settings = super::settings::IdeSettings { detect_indents: false, ..Default::default() };
        settings.apply_to_session(session.settings_mut());
        assert_eq!(shown(&session), zen_i18n::t!("ide-status-spaces", count = 4));
        settings.detect_indents = true;
        settings.apply_to_session(session.settings_mut());
        registry.dispatch(&mut session, "editor.indent.useConfigured", serde_json::json!({})).unwrap();
        assert_eq!(shown(&session), zen_i18n::t!("ide-status-spaces", count = 4), "switched in the status bar");
    }

    const IDEA_CONFIG: &str = "<!-- Synthetic fixture. -->\n<component name=\"ProjectCodeStyleConfiguration\"><state>\
        <option name=\"USE_PER_PROJECT_SETTINGS\" value=\"true\" /></state></component>\n";

    const IDEA_YAML_TABS: &str = "<!-- Synthetic fixture. -->\n<component name=\"ProjectCodeStyleConfiguration\">\
        <code_scheme name=\"Project\" version=\"173\"><codeStyleSettings language=\"yaml\"><indentOptions>\
        <option name=\"USE_TAB_CHARACTER\" value=\"true\" /></indentOptions></codeStyleSettings></code_scheme></component>\n";

    #[test]
    fn a_gesture_keeps_to_one_axis_except_where_scrolling_is_free() {
        let mut lock = None;
        assert_eq!(locked_scroll(&mut lock, vec2(3.0, 10.0), 0.0, false), vec2(0.0, 10.0));
        assert_eq!(locked_scroll(&mut lock, vec2(20.0, 4.0), 0.1, false), vec2(0.0, 4.0));
        assert_eq!(locked_scroll(&mut lock, Vec2::ZERO, 0.5, false), Vec2::ZERO);
        assert_eq!(lock, None);
        assert_eq!(locked_scroll(&mut lock, vec2(20.0, 4.0), 0.6, false), vec2(20.0, 0.0));
        assert_eq!(locked_scroll(&mut lock, vec2(6.0, 5.0), 0.7, true), vec2(6.0, 5.0));
        assert_eq!(lock, None, "no axis is held there");
    }

    #[test]
    fn initials_take_the_first_letters_of_the_first_two_words() {
        assert_eq!(project_initials("zen-ide"), "ZI");
        assert_eq!(project_initials("tinyapp"), "T");
        assert_eq!(project_initials("DataHub"), "DH");
        assert_eq!(project_initials("fast-path-log"), "FP");
        assert_eq!(project_initials("k3s-flux"), "KF");
        assert_eq!(project_initials(""), "");
    }

    #[test]
    fn the_status_bar_toggle_shows_and_hides_the_code_authors() {
        use egui::{Event, PointerButton, Pos2};
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let mut registry = zen_core::Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = zen_core::Session::new();
        registry.dispatch(&mut session, "file.project.open", serde_json::json!({ "path": dir.path() })).unwrap();
        let product = super::IdeProduct {
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
        let mut view = super::IdeView::new(std::sync::Arc::new(product), crate::plugins::Plugins::new(None));
        view.project_open = false;
        let (width, height) = (900.0, 400.0);
        let mut screen = crate::offscreen::Offscreen::new(vec2(width, height), 1.5);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let mut frame = |events: Vec<Event>, view: &mut super::IdeView, session: &mut zen_core::Session| {
            let image = screen.frame(events, |ui| view.show(ui, &registry, session));
            view.requests.clear();
            image
        };
        for _ in 0..2 {
            frame(Vec::new(), &mut view, &mut session);
        }
        assert!(!view.code_authors_shown(), "off by default");
        let at = Pos2::new(width - 8.0 - 11.0, height - super::metrics::STATUS_BAR / 2.0);
        let click = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let off = frame(vec![Event::PointerMoved(at)], &mut view, &mut session);
        frame(vec![click(true)], &mut view, &mut session);
        frame(vec![click(false)], &mut view, &mut session);
        assert!(view.code_authors_shown(), "a click shows them");
        assert!(view.overview.settings.code_vision.authors);
        let on = frame(Vec::new(), &mut view, &mut session);
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join("code-authors-off.png"), &off).unwrap();
            crate::offscreen::write_png(&dir.join("code-authors-on.png"), &on).unwrap();
        }
        let corner = |image: &egui::ColorImage| {
            let (x, y) =
                (((width - 19.0) * 1.5) as usize, ((height - super::metrics::STATUS_BAR / 2.0 + 8.0) * 1.5) as usize);
            image.pixels[y * image.size[0] + x]
        };
        assert_ne!(corner(&on), corner(&off), "the toggle is lit while on");
        frame(vec![click(true)], &mut view, &mut session);
        frame(vec![click(false)], &mut view, &mut session);
        assert!(!view.code_authors_shown(), "a second click hides them");
        assert!(!view.action_checked("view.codeVision.authors.toggle", &session));
    }

    #[test]
    fn the_encoding_widget_searches_asks_reload_or_convert_and_applies_the_choice() {
        use egui::accesskit::Role;
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("old.txt");
        std::fs::write(&file, [0xcf, 0xf0, 0xe8, b'\n']).unwrap();
        let mut registry = zen_core::Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = zen_core::Session::new();
        registry.dispatch(&mut session, "file.project.open", serde_json::json!({ "path": dir.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", serde_json::json!({ "path": file })).unwrap();
        let mut view = super::IdeView::new(
            std::sync::Arc::new(super::IdeProduct {
                folding_options: Vec::new(),
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
                debugger: None,
                project_models: Vec::new(),
                outliners: Vec::new(),
                run_providers: Vec::new(),
                language_servers: Vec::new(),
                templates: Vec::new(),
                inspectors: Vec::new(),
            }),
            crate::plugins::Plugins::new(None),
        );
        let mut screen = crate::offscreen::Offscreen::new(vec2(1000.0, 500.0), 1.0);
        let click = |screen: &mut crate::offscreen::Offscreen,
                     view: &mut super::IdeView,
                     session: &mut zen_core::Session,
                     name: &str| {
            let tree = screen.accessibility_tree(|ui| view.show(ui, &registry, session));
            let node = crate::accessibility::testing::find(&tree, Role::Button, name)
                .unwrap_or_else(|| panic!("no button named {name:?}"));
            let bounds = node.bounds().expect("the node has bounds");
            let at = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
                screen.step(events, |ui| view.show(ui, &registry, session));
            }
        };
        session.state_mut::<zen_workbench::Workbench>().take_messages();
        for _ in 0..2 {
            screen.render(|ui| view.show(ui, &registry, &mut session));
        }
        let widget = zen_i18n::t!("ide-status-encoding-named", charset = "UTF-8");
        click(&mut screen, &mut view, &mut session, &widget);
        assert!(view.encoding_popup.is_some(), "a click opens the list");
        view.encoding_popup.as_mut().unwrap().query = "1251".to_owned();
        assert_eq!(view.encoding_popup.as_ref().unwrap().matching(), ["windows-1251"], "the list is searchable");
        for _ in 0..2 {
            screen.render(|ui| view.show(ui, &registry, &mut session));
        }
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            let image = screen.render(|ui| view.show(ui, &registry, &mut session));
            crate::offscreen::write_png(&std::path::Path::new(&dir).join("encoding-list.png"), &image).unwrap();
        }
        click(&mut screen, &mut view, &mut session, "windows-1251");
        assert!(view.encoding_popup.is_none());
        assert!(view.encoding_choice.is_some(), "a change that is not lossless asks");
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            let image = screen.render(|ui| view.show(ui, &registry, &mut session));
            crate::offscreen::write_png(&std::path::Path::new(&dir).join("encoding-choice.png"), &image).unwrap();
        }
        click(&mut screen, &mut view, &mut session, &zen_i18n::t!("ide-encoding-reload"));
        assert!(view.encoding_choice.is_none());
        let document = session.state::<zen_workbench::Workbench>().unwrap().active_document().unwrap();
        assert_eq!(document.text(), "\u{41f}\u{440}\u{438}\n");
        assert_eq!(document.charset(), "windows-1251");
        assert_eq!(std::fs::read(&file).unwrap(), [0xcf, 0xf0, 0xe8, b'\n'], "Reload leaves the file");
    }

    #[test]
    fn the_line_separator_widget_changes_the_files_separator() {
        use egui::accesskit::Role;
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("win.txt");
        std::fs::write(&file, "one\r\ntwo\r\n").unwrap();
        let mut registry = zen_core::Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = zen_core::Session::new();
        registry.dispatch(&mut session, "file.project.open", serde_json::json!({ "path": dir.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", serde_json::json!({ "path": file })).unwrap();
        registry.dispatch(&mut session, "editor.caret.goTo", serde_json::json!({ "line": 2, "column": 4 })).unwrap();
        let product = super::IdeProduct {
            folding_options: Vec::new(),
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
            debugger: None,
            project_models: Vec::new(),
            outliners: Vec::new(),
            run_providers: Vec::new(),
            language_servers: Vec::new(),
            templates: Vec::new(),
            inspectors: Vec::new(),
        };
        let mut view = super::IdeView::new(std::sync::Arc::new(product), crate::plugins::Plugins::new(None));
        let mut screen = crate::offscreen::Offscreen::new(vec2(1000.0, 500.0), 1.0);
        let click = |screen: &mut crate::offscreen::Offscreen,
                     view: &mut super::IdeView,
                     session: &mut zen_core::Session,
                     name: &str| {
            let tree = screen.accessibility_tree(|ui| view.show(ui, &registry, session));
            let node = crate::accessibility::testing::find(&tree, Role::Button, name)
                .unwrap_or_else(|| panic!("no button named {name:?}"));
            let bounds = node.bounds().expect("the node has bounds");
            let at = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
                screen.step(events, |ui| view.show(ui, &registry, session));
            }
        };
        for _ in 0..2 {
            screen.render(|ui| view.show(ui, &registry, &mut session));
        }
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            let image = screen.render(|ui| view.show(ui, &registry, &mut session));
            crate::offscreen::write_png(&std::path::Path::new(&dir).join("line-separator-status-crlf.png"), &image)
                .unwrap();
        }
        let current = serde_json::json!({ "separator": "crlf" });
        assert!(!view.separator_choosable(&session, "file.lineSeparator.set", &current), "the current one is off");
        assert!(view.separator_choosable(
            &session,
            "file.lineSeparator.set",
            &serde_json::json!({ "separator": "lf" })
        ));
        click(
            &mut screen,
            &mut view,
            &mut session,
            &zen_i18n::t!("ide-status-line-separator-named", separator = "CRLF"),
        );
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            let image = screen.render(|ui| view.show(ui, &registry, &mut session));
            crate::offscreen::write_png(&std::path::Path::new(&dir).join("line-separator-menu.png"), &image).unwrap();
        }
        click(&mut screen, &mut view, &mut session, &super::keymap_page::action_title("file.lineSeparator.set/lf"));
        let document = session.state::<zen_workbench::Workbench>().unwrap().active_document().unwrap();
        assert_eq!(document.text(), "one\ntwo\n");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\n", "written at once");
        screen.render(|ui| view.show(ui, &registry, &mut session));
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            let image = screen.render(|ui| view.show(ui, &registry, &mut session));
            crate::offscreen::write_png(&std::path::Path::new(&dir).join("line-separator-status-lf.png"), &image)
                .unwrap();
        }
        let tree = screen.accessibility_tree(|ui| view.show(ui, &registry, &mut session));
        assert!(
            crate::accessibility::testing::find(
                &tree,
                Role::Button,
                &zen_i18n::t!("ide-status-line-separator-named", separator = "LF")
            )
            .is_some()
        );
    }

    #[test]
    fn the_main_toolbar_draws_its_customized_list_and_skips_an_unknown_action() {
        use egui::accesskit::Role;
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("win.txt");
        std::fs::write(&file, "one\r\ntwo\r\n").unwrap();
        let mut registry = zen_core::Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = zen_core::Session::new();
        registry.dispatch(&mut session, "file.project.open", serde_json::json!({ "path": dir.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", serde_json::json!({ "path": file })).unwrap();
        registry.dispatch(&mut session, "editor.caret.goTo", serde_json::json!({ "line": 2, "column": 4 })).unwrap();
        let product = super::IdeProduct {
            folding_options: Vec::new(),
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
            debugger: None,
            project_models: Vec::new(),
            outliners: Vec::new(),
            run_providers: Vec::new(),
            language_servers: Vec::new(),
            templates: Vec::new(),
            inspectors: Vec::new(),
        };
        let mut view = super::IdeView::new(std::sync::Arc::new(product), crate::plugins::Plugins::new(None));
        let mut screen = crate::offscreen::Offscreen::new(vec2(1000.0, 500.0), 1.0);
        let reformat = "editor.code.reformat";
        view.overview.settings.customization.groups.insert(
            super::customization::MAIN_TOOLBAR.to_owned(),
            vec![reformat.to_owned(), "gone.action".to_owned(), super::customization::SEPARATOR.to_owned()],
        );
        super::customization::note_commands(&registry);
        for _ in 0..2 {
            screen.render(|ui| view.show(ui, &registry, &mut session));
        }
        let tree = screen.accessibility_tree(|ui| view.show(ui, &registry, &mut session));
        let named = |name: &str| crate::accessibility::testing::find(&tree, Role::Button, name).is_some();
        assert!(named(&super::keymap_page::action_title(reformat)));
        assert!(!named("gone.action"));
        let at = egui::pos2(500.0, 10.0);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
            screen.step(events, |ui| view.show(ui, &registry, &mut session));
        }
        let tree = screen.accessibility_tree(|ui| view.show(ui, &registry, &mut session));
        let customize =
            crate::accessibility::testing::find(&tree, Role::Button, &zen_i18n::t!("ide-toolbar-customize"));
        let bounds = customize.expect("the toolbar has a context menu").bounds().unwrap();
        let at = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
            screen.step(events, |ui| view.show(ui, &registry, &mut session));
        }
        assert!(view.settings.is_open(), "Customize Toolbar… opens the page");
    }
}

#[cfg(test)]
mod file_type_tests {
    use std::path::Path;
    use std::sync::Arc;

    use egui::{Color32, vec2};
    use serde_json::json;
    use zen_i18n::t;

    struct GoWords;

    impl zen_syntax::Highlighter for GoWords {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".go")
        }

        fn highlight(&self, text: &str) -> Vec<zen_syntax::Span> {
            let mut spans = Vec::new();
            for word in ["package", "func"] {
                for (at, _) in text.match_indices(word) {
                    spans.push(zen_syntax::Span {
                        range: at..at + word.len(),
                        kind: zen_syntax::HighlightKind::Keyword,
                    });
                }
            }
            spans.sort_by_key(|span| span.range.start);
            spans
        }
    }

    pub(super) fn product() -> super::IdeProduct {
        super::IdeProduct {
            folding_options: Vec::new(),
            name: "Test",
            id: "test",
            version: "0.0.0",
            highlighters: vec![Arc::new(GoWords)],
            plugins: None,
            mcp_port: 0,
            modules: Vec::new(),
            areas: Vec::new(),
            settings_pages: Vec::new(),
            code_styles: Vec::new(),
            debugger: None,
            project_models: Vec::new(),
            outliners: Vec::new(),
            run_providers: Vec::new(),
            language_servers: Vec::new(),
            templates: Vec::new(),
            inspectors: Vec::new(),
        }
    }

    pub(super) fn near(pixel: Color32, wanted: Color32) -> bool {
        let close = |a: u8, b: u8| a.abs_diff(b) <= 12;
        close(pixel.r(), wanted.r()) && close(pixel.g(), wanted.g()) && close(pixel.b(), wanted.b())
    }

    pub(super) fn labels(tree: &egui::accesskit::TreeUpdate) -> Vec<String> {
        tree.nodes.iter().filter_map(|(_, node)| node.label().or(node.value()).map(str::to_owned)).collect()
    }

    #[test]
    fn a_changed_type_applies_to_the_open_file_and_ignored_names_leave_the_tree_at_once() {
        let _exclusive = super::settings::file_types::testing::exclusive();
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(std::path::PathBuf::from);
        let languages: &[&str] = if snapshots.is_some() { &["en", "ru"] } else { &["en"] };
        for language in languages {
            zen_vfs::file_types::set_changes(zen_vfs::file_types::Changes::default());
            zen_i18n::set_language(Some(language));
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().canonicalize().unwrap();
            let page = root.join("page.zt-tmpl");
            std::fs::write(&page, "package main\n\nfunc main() {\n}\n").unwrap();
            std::fs::create_dir_all(root.join("zt-cache")).unwrap();
            std::fs::write(root.join("zt-cache/a.go"), "package cache\n").unwrap();
            std::fs::write(root.join("notes.zt-skip"), "notes\n").unwrap();
            std::fs::write(root.join("main.go"), "package main\n").unwrap();
            let mut registry = zen_core::Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let mut session = zen_core::Session::new();
            session.set_state(zen_workbench::file_types::FileTypeStore::load(None));
            registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
            registry.dispatch(&mut session, "file.document.open", json!({ "path": page })).unwrap();
            let mut view = super::IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
            let keyword = view.palette.keyword;
            let mut screen = crate::offscreen::Offscreen::new(vec2(1100.0, 520.0), 1.5);
            let mut frame = |view: &mut super::IdeView, session: &mut zen_core::Session| {
                let mut image = screen.frame(Vec::new(), |ui| view.show(ui, &registry, session));
                for _ in 0..3 {
                    image = screen.frame(Vec::new(), |ui| view.show(ui, &registry, session));
                }
                let tree = screen.accessibility_tree(|ui| view.show(ui, &registry, session));
                view.requests.clear();
                (image, labels(&tree))
            };
            let indent = |session: &zen_core::Session| {
                let workbench = session.state::<zen_workbench::Workbench>().unwrap();
                zen_workbench::project_settings::indent_unit_of(session, workbench.active_document().unwrap())
            };
            let colored = |image: &egui::ColorImage| image.pixels.iter().filter(|pixel| near(**pixel, keyword)).count();

            let (before, shown) = frame(&mut view, &mut session);
            assert_eq!(indent(&session), "    ", "an unknown type: four spaces");
            assert!(shown.iter().any(|label| label.contains("zt-cache")), "{shown:?}");
            assert!(shown.iter().any(|label| label.contains("notes.zt-skip")), "{shown:?}");
            assert!(!shown.iter().any(|label| label == ".git"));
            let plain = colored(&before);

            registry
                .dispatch(
                    &mut session,
                    "editor.fileTypes.addPattern",
                    json!({ "type": "zt-go", "pattern": "*.zt-tmpl" }),
                )
                .unwrap();
            let mut ignored: Vec<String> =
                zen_vfs::file_types::DEFAULT_IGNORED.iter().map(|name| (*name).to_owned()).collect();
            ignored.extend(["zt-cache".to_owned(), "*.zt-skip".to_owned()]);
            registry.dispatch(&mut session, "editor.fileTypes.setIgnored", json!({ "patterns": ignored })).unwrap();
            let (after, shown) = frame(&mut view, &mut session);
            assert_eq!(indent(&session), "\t", "Go's code style, without reopening the file");
            assert_eq!(zen_workbench::code_style::language_of(&page), "go");
            assert!(colored(&after) > plain + 50, "the open file is highlighted as Go: {} > {plain}", colored(&after));
            assert!(!shown.iter().any(|label| label.contains("zt-cache")), "{shown:?}");
            assert!(!shown.iter().any(|label| label.contains("notes.zt-skip")), "{shown:?}");
            assert!(shown.iter().any(|label| label.contains("main.go")), "{shown:?}");
            assert!(shown.iter().any(|label| *label == t!("ide-status-tab")), "the status bar says Tab: {shown:?}");
            if let Some(dir) = &snapshots {
                crate::offscreen::write_png(&dir.join(format!("open-file-before-{language}.png")), &before).unwrap();
                crate::offscreen::write_png(&dir.join(format!("open-file-after-{language}.png")), &after).unwrap();
            }
            assert!(Path::new(&page).exists());
        }
        zen_vfs::file_types::set_changes(zen_vfs::file_types::Changes::default());
        zen_i18n::set_language(Some("en"));
    }

    struct BrokenMember(std::sync::Mutex<bool>);

    impl zen_workbench::model::ModelProvider for BrokenMember {
        fn id(&self) -> &str {
            "broken"
        }

        fn watches(&self, file_name: &str) -> bool {
            file_name == "Cargo.toml"
        }

        fn load(
            &self,
            root: &Path,
            _env: &zen_workbench::model::ModelEnv,
        ) -> Option<zen_workbench::model::ProjectModel> {
            use zen_workbench::model::{Library, LibraryKind, ModelProblem, Module, ProjectModel};
            let member = |name: &str, folder: &str| Module {
                name: name.to_owned(),
                root: root.join(folder),
                file: root.join(folder).join("Cargo.toml"),
                ..Module::default()
            };
            let broken = *self.0.lock().unwrap();
            Some(ProjectModel {
                modules: vec![member("calc", "calc"), member("app", "app")],
                libraries: vec![Library {
                    kind: LibraryKind::Dependency,
                    name: "serde".to_owned(),
                    version: Some("1.0.228".to_owned()),
                    root: root.join("registry/serde-1.0.228"),
                    exists: true,
                    group: Some("Crates".to_owned()),
                    label: None,
                }],
                problems: if broken {
                    vec![ModelProblem {
                        file: root.join("app/Cargo.toml"),
                        line: 3,
                        message: "string values must be quoted, expected literal string".to_owned(),
                        warning: false,
                    }]
                } else {
                    Vec::new()
                },
                ..ProjectModel::default()
            })
        }
    }

    #[test]
    fn a_broken_manifest_is_a_problem_and_crates_has_no_stray_name() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_owned();
        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = [\"app\", \"calc\"]\n").unwrap();
        for folder in ["app", "calc", "registry/serde-1.0.228"] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        let manifest = root.join("app/Cargo.toml");
        std::fs::write(&manifest, "[package]\nname = \"app\"\nversion = \n").unwrap();
        std::fs::write(root.join("calc/Cargo.toml"), "[package]\nname = \"calc\"\n").unwrap();
        let mut registry = zen_core::Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = zen_core::Session::new();
        let provider = Arc::new(BrokenMember(std::sync::Mutex::new(true)));
        session
            .state_mut::<zen_workbench::project_model::ProjectModels>()
            .set_providers(vec![provider.clone()], zen_workbench::model::ModelEnv::default());
        registry.dispatch(&mut session, "file.project.open", serde_json::json!({ "path": root })).unwrap();
        registry.dispatch(&mut session, "project.model.refresh", serde_json::json!({})).unwrap();
        registry.dispatch(&mut session, "file.document.open", serde_json::json!({ "path": manifest })).unwrap();
        let product = super::IdeProduct {
            folding_options: Vec::new(),
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
            debugger: None,
            project_models: Vec::new(),
            outliners: Vec::new(),
            run_providers: Vec::new(),
            language_servers: Vec::new(),
            templates: Vec::new(),
            inspectors: Vec::new(),
        };
        let mut view = super::IdeView::new(Arc::new(product), crate::plugins::Plugins::new(None));
        view.show_tool("problems");
        view.problems_project = true;
        view.expanded.insert(root.clone());
        view.expanded.insert(super::libraries_tree::group_path(&root, "libraries"));
        view.expanded.insert(super::libraries_tree::group_path(&root, "modules"));
        let mut screen = crate::offscreen::Offscreen::new(vec2(1100.0, 640.0), 1.5);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(std::path::PathBuf::from);
        let mut frame = |view: &mut super::IdeView, session: &mut zen_core::Session| {
            for _ in 0..3 {
                screen.frame(Vec::new(), |ui| view.show(ui, &registry, session));
                view.requests.clear();
            }
            screen.frame(Vec::new(), |ui| view.show(ui, &registry, session))
        };
        let broken = frame(&mut view, &mut session);
        let libraries = view.libraries.as_ref().expect("External Libraries is read");
        assert_eq!(libraries.modules_label, "Crates");
        assert_eq!(libraries.module_name, None, "no 'calc' after Crates");
        let listed = registry.dispatch(&mut session, "problems.project.list", serde_json::json!({})).unwrap();
        assert_eq!(listed[0]["path"], serde_json::json!(manifest));
        assert_eq!(listed[0]["problems"][0]["line"], 3);
        let marked =
            registry.dispatch(&mut session, "problems.file.list", serde_json::json!({ "path": manifest })).unwrap();
        assert_eq!(marked[0]["severity"], "error", "the editor underlines it");
        assert_eq!(marked[0]["source"], "project model");

        let unsaved = "# one\n# two\n[package]\nname = \"app\"\nversion = \n";
        registry
            .dispatch(&mut session, "editor.text.write", serde_json::json!({ "path": manifest, "text": unsaved }))
            .unwrap();
        let moved = frame(&mut view, &mut session);
        let marked =
            registry.dispatch(&mut session, "problems.file.list", serde_json::json!({ "path": manifest })).unwrap();
        assert_eq!(marked[0]["line"], 5, "the mark moves with the lines added above it");
        registry
            .dispatch(
                &mut session,
                "editor.text.write",
                serde_json::json!({ "path": manifest, "text": "[package]\nname = \"app\"\nversion = \n" }),
            )
            .unwrap();

        std::fs::write(&manifest, "[package]\nname = \"app\"\nversion = \"0.1.0\"\n").unwrap();
        *provider.0.lock().unwrap() = false;
        registry.dispatch(&mut session, "file.document.syncWithDisk", serde_json::json!({ "paths": [] })).unwrap();
        registry.dispatch(&mut session, "project.model.refresh", serde_json::json!({ "paths": [manifest] })).unwrap();
        let fixed = frame(&mut view, &mut session);
        let listed = registry.dispatch(&mut session, "problems.project.list", serde_json::json!({})).unwrap();
        assert_eq!(listed, serde_json::json!([]), "fixed");
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join("model-problem-broken.png"), &broken).unwrap();
            crate::offscreen::write_png(&dir.join("model-problem-moved.png"), &moved).unwrap();
            crate::offscreen::write_png(&dir.join("model-problem-fixed.png"), &fixed).unwrap();
        }
    }
}

#[cfg(test)]
mod associate_tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use egui::{Event, PointerButton, Pos2, vec2};
    use serde_json::json;
    use zen_i18n::t;

    use super::file_type_tests::{labels, near, product};

    fn center(tree: &egui::accesskit::TreeUpdate, label: &str, right_of: f64) -> Pos2 {
        let bounds = tree
            .nodes
            .iter()
            .filter(|(_, node)| node.label().is_some_and(|shown| shown.trim_end() == label))
            .filter_map(|(_, node)| node.bounds())
            .filter(|bounds| bounds.x0 >= right_of)
            .max_by(|a, b| a.x0.total_cmp(&b.x0))
            .unwrap_or_else(|| panic!("no {label:?} in {:?}", labels(tree)));
        Pos2::new(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32)
    }

    fn left_of(tree: &egui::accesskit::TreeUpdate, label: &str) -> f64 {
        tree.nodes
            .iter()
            .filter(|(_, node)| node.label().is_some_and(|shown| shown.trim_end() == label))
            .filter_map(|(_, node)| node.bounds())
            .map(|bounds| bounds.x1)
            .fold(0.0, f64::max)
    }

    fn click(at: Pos2) -> Vec<Vec<Event>> {
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        vec![vec![Event::PointerMoved(at)], vec![button(true)], vec![button(false)]]
    }

    #[test]
    fn an_unknown_file_asks_how_to_open_it_and_overrides_change_only_the_chosen_files() {
        let _exclusive = super::settings::file_types::testing::exclusive();
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let languages: &[&str] = if snapshots.is_some() { &["en", "ru"] } else { &["en"] };
        for language in languages {
            zen_vfs::file_types::set_overrides(Default::default());
            zen_vfs::file_types::set_changes(zen_vfs::file_types::Changes::default());
            zen_i18n::with_language(language, || {
                let data = tempfile::tempdir().unwrap();
                let dir = tempfile::tempdir().unwrap();
                let root = dir.path().canonicalize().unwrap();
                let blob = root.join("dump.zt-bin");
                std::fs::write(&blob, [0x7f, b'Z', b'T', 0, 1, 2, b'x', 0xff]).unwrap();
                let (a, b, c) = (root.join("a.go"), root.join("b.go"), root.join("c.go"));
                for path in [&a, &b, &c] {
                    std::fs::write(path, "package main\n\nfunc main() {\n}\n").unwrap();
                }
                let mut registry = zen_core::Registry::new();
                zen_workbench::register(&mut registry).unwrap();
                let mut session = zen_core::Session::new();
                session.set_state(zen_workbench::file_types::FileTypeStore::load(Some(
                    data.path().join(zen_workbench::file_types::FILE_NAME),
                )));
                let opened = Arc::new(Mutex::new(Vec::new()));
                let seen = Arc::clone(&opened);
                session.set_state(zen_workbench::file_types::SystemOpener::new(move |path| {
                    seen.lock().unwrap().push(path.to_owned());
                    Ok(())
                }));
                registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
                let mut view = super::IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
                let keyword = view.palette.keyword;
                let mut screen = crate::offscreen::Offscreen::new(vec2(1100.0, 620.0), 1.5);
                let look = |screen: &mut crate::offscreen::Offscreen,
                            view: &mut super::IdeView,
                            session: &mut zen_core::Session,
                            paint: bool| {
                    for _ in 0..3 {
                        screen.step(Vec::new(), |ui| view.show(ui, &registry, session));
                    }
                    let image = paint.then(|| screen.render(|ui| view.show(ui, &registry, session)));
                    let tree = screen.accessibility_tree(|ui| view.show(ui, &registry, session));
                    view.requests.clear();
                    (
                        image
                            .map(crate::offscreen::Frame::into_image)
                            .unwrap_or_else(|| egui::ColorImage::new([1, 1], vec![egui::Color32::BLACK])),
                        tree,
                    )
                };
                let shots = snapshots.is_some();
                let snap = |name: &str, image: &egui::ColorImage| {
                    if let Some(dir) = &snapshots {
                        crate::offscreen::write_png(&dir.join(format!("{name}-{language}.png")), image).unwrap();
                    }
                };

                let refused =
                    registry.dispatch(&mut session, "file.document.open", json!({ "path": blob })).unwrap_err();
                view.report_at(&egui::Context::default(), refused);
                assert!(view.associate_dialog.is_some(), "the file's open asks how to open it");
                let (image, tree) = look(&mut screen, &mut view, &mut session, shots);
                snap("associate-dialog", &image);
                let shown = labels(&tree);
                for wanted in [
                    t!("ide-associate-title"),
                    t!("ide-associate-pattern"),
                    t!("ide-associate-open-in", product = "Test"),
                    t!("ide-associate-detect"),
                    t!("ide-associate-type"),
                    t!("ide-associate-application"),
                    zen_i18n::t!("file-type-text"),
                ] {
                    assert!(shown.contains(&wanted), "{wanted:?} in {shown:?}");
                }
                assert!(shown.iter().any(|label| label == "*.zt-bin"), "{shown:?}");
                let dialog = view.associate_dialog.as_mut().unwrap();
                dialog.way = super::associate_dialog::Way::Type;
                dialog.chosen = Some(zen_vfs::file_types::TEXT.to_owned());
                let (chosen, _) = look(&mut screen, &mut view, &mut session, shots);
                snap("associate-dialog-type", &chosen);
                let enter = Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                };
                screen.step(vec![enter.clone()], |ui| view.show(ui, &registry, &mut session));
                assert!(view.associate_dialog.is_none(), "OK closes it");
                let workbench = session.state::<zen_workbench::Workbench>().unwrap();
                assert_eq!(workbench.active_document().map(|document| document.path().to_owned()), Some(blob.clone()));
                let typed =
                    registry.dispatch(&mut session, "editor.fileTypes.typeOf", json!({ "path": blob })).unwrap();
                assert_eq!(typed["id"], "text");

                view.perform_action(
                    &egui::Context::default(),
                    &registry,
                    &mut session,
                    "view.associateFileType.open",
                    json!({ "path": c }),
                    true,
                )
                .unwrap();
                assert_eq!(view.associate_dialog.as_ref().map(|dialog| dialog.pattern.as_str()), Some("*.go"));
                let dialog = view.associate_dialog.as_mut().unwrap();
                dialog.way = super::associate_dialog::Way::Application;
                dialog.pattern = "c.go".to_owned();
                screen.step(vec![enter.clone()], |ui| view.show(ui, &registry, &mut session));
                assert_eq!(*opened.lock().unwrap(), std::slice::from_ref(&c), "the fake system opener got it");
                let layout = super::workspace::ProjectLayout { documents: vec![c.clone()], ..Default::default() };
                super::workspace::restore(&mut view, &registry, &mut session, &layout);
                assert_eq!(opened.lock().unwrap().len(), 1, "reopening the project's tabs starts no application");
                registry
                    .dispatch(
                        &mut session,
                        "editor.fileTypes.removePattern",
                        json!({ "type": "native", "pattern": "c.go" }),
                    )
                    .unwrap();

                registry.dispatch(&mut session, "file.document.open", json!({ "path": c })).unwrap();
                let (go, _) = look(&mut screen, &mut view, &mut session, true);
                let colored =
                    |image: &egui::ColorImage| image.pixels.iter().filter(|pixel| near(**pixel, keyword)).count();
                let highlighted = colored(&go);

                view.selection.select_all_of(vec![a.clone(), b.clone()]);
                view.perform_action(
                    &egui::Context::default(),
                    &registry,
                    &mut session,
                    "view.project.contextMenu",
                    json!({ "path": a }),
                    true,
                )
                .unwrap();
                let (menu, tree) = look(&mut screen, &mut view, &mut session, shots);
                snap("tree-menu", &menu);
                let shown = labels(&tree);
                for wanted in [t!("ide-tree-associate"), t!("ide-tree-override-type"), t!("ide-tree-revert-override")] {
                    assert!(shown.iter().any(|label| label.starts_with(wanted.as_str())), "{wanted:?} in {shown:?}");
                }
                let item = center(&tree, &t!("ide-tree-override-type"), 0.0);
                screen.step(vec![Event::PointerMoved(item)], |ui| view.show(ui, &registry, &mut session));
                let (submenu, tree) = look(&mut screen, &mut view, &mut session, shots);
                snap("tree-menu-override", &submenu);
                let text = center(&tree, &t!("file-type-text"), left_of(&tree, &t!("ide-tree-override-type")) - 4.0);
                for events in click(text) {
                    screen.step(events, |ui| view.show(ui, &registry, &mut session));
                }
                view.requests.clear();
                let own = registry.dispatch(&mut session, "editor.fileTypes.overrides", json!({})).unwrap();
                assert_eq!(
                    own,
                    json!([
                        { "path": a, "type": "text", "name": t!("file-type-text") },
                        { "path": b, "type": "text", "name": t!("file-type-text") }
                    ]),
                    "only the selected files"
                );
                assert_eq!(crate::file_icons::icon_at(&a), crate::file_icons::icon_of("file.txt"), "every list's icon");
                assert_eq!(crate::file_icons::icon_at(&c), crate::file_icons::icon_of("c.go"));
                assert_ne!(crate::file_icons::icon_at(&a), crate::file_icons::icon_at(&c));
                registry.dispatch(&mut session, "file.document.open", json!({ "path": a })).unwrap();
                let (plain, _) = look(&mut screen, &mut view, &mut session, true);
                snap("overridden-as-text", &plain);
                assert!(
                    colored(&plain) + 50 < highlighted,
                    "Text: not highlighted, {} < {highlighted}",
                    colored(&plain)
                );
                let saved =
                    std::fs::read_to_string(data.path().join(zen_workbench::file_types::OVERRIDES_FILE_NAME)).unwrap();
                assert!(saved.contains("a.go") && saved.contains("b.go") && !saved.contains("c.go"), "{saved}");

                view.perform_action(
                    &egui::Context::default(),
                    &registry,
                    &mut session,
                    "view.project.contextMenu",
                    json!({ "path": a }),
                    true,
                )
                .unwrap();
                let (_, tree) = look(&mut screen, &mut view, &mut session, shots);
                for events in click(center(&tree, &t!("ide-tree-revert-override"), 0.0)) {
                    screen.step(events, |ui| view.show(ui, &registry, &mut session));
                }
                view.requests.clear();
                let own = registry.dispatch(&mut session, "editor.fileTypes.overrides", json!({})).unwrap();
                assert_eq!(own, json!([]), "Revert File Type Override undoes it");
                let (again, _) = look(&mut screen, &mut view, &mut session, true);
                assert!(colored(&again) + 50 > highlighted, "Go again: {} vs {highlighted}", colored(&again));
            });
        }
    }

    #[test]
    fn create_test_writes_the_test_from_its_dialog_and_test_history_reopens_a_run() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let subject = root.join("cart.go");
        std::fs::write(&subject, "package shop\n\nfunc Total() int {\n\treturn 0\n}\n").unwrap();
        let mut registry = zen_core::Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = zen_core::Session::new();
        session.set_state(super::trusting_everything());
        registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
        session.state_mut::<zen_workbench::test_history::TestHistory>().set_data_dir(data.path().to_owned());
        let declared = json!([{
            "id": "go-like", "extensions": ["go"], "pairs": [{ "subject": "{name}.go", "test": "{name}_test.go" }],
            "members": "^func (?P<name>\\w+)", "tests": ["Test{Name}"],
            "frameworks": [{ "id": "go", "name": "Go", "file": "package shop\n${METHODS}", "method": "\nfunc ${TEST}(t *testing.T) {\n}\n" }]
        }]);
        let support = zen_run::test_support::DeclaredTests::parse_list(&declared).pop().unwrap();
        session.state_mut::<zen_run::test_support::TestSupports>().add(Arc::new(support));
        registry.dispatch(&mut session, "file.document.open", json!({ "path": subject })).unwrap();
        let mut view = super::IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
        let mut screen = crate::offscreen::Offscreen::new(vec2(1100.0, 620.0), 1.5);
        let ctx = egui::Context::default();
        let opened =
            view.perform_action(&ctx, &registry, &mut session, "view.createTest.open", json!({ "line": 3 }), true);
        assert_eq!(opened.unwrap()["options"]["name"], "cart_test");
        for _ in 0..3 {
            screen.step(Vec::new(), |ui| view.show(ui, &registry, &mut session));
        }
        let shown = labels(&screen.accessibility_tree(|ui| view.show(ui, &registry, &mut session)));
        for wanted in [
            t!("ide-create-test-title"),
            t!("ide-create-test-members"),
            t!("ide-create-test-framework"),
            "Total".to_owned(),
        ] {
            assert!(shown.contains(&wanted), "{wanted:?} in {shown:?}");
        }
        let enter = Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        screen.step(vec![enter.clone()], |ui| view.show(ui, &registry, &mut session));
        assert!(view.create_test.is_none(), "OK closes it");
        let written = std::fs::read_to_string(root.join("cart_test.go")).unwrap();
        assert_eq!(written, "package shop\n\nfunc TestTotal(t *testing.T) {\n}\n");

        let fake = zen_run::fake::FakeProvider::new();
        fake.set_script(
            &[(zen_run::Stream::Stdout, "@fail app TestTotal 9"), (zen_run::Stream::Stdout, "@fail app - 9")],
            1,
        );
        session.state_mut::<zen_workbench::run::Runs>().set_providers(vec![Arc::new(fake)], zen_run::RunEnv::default());
        let configuration = serde_json::to_value(zen_run::fake::FakeProvider::configuration("cart tests")).unwrap();
        registry
            .dispatch(&mut session, "run.process.start", json!({ "configuration": configuration, "waitMs": 10_000 }))
            .unwrap();
        screen.step(Vec::new(), |ui| view.show(ui, &registry, &mut session));
        let listed =
            view.perform_action(&ctx, &registry, &mut session, "view.testHistory.open", json!({}), true).unwrap();
        assert_eq!(listed["entries"].as_array().map(Vec::len), Some(1), "{listed}");
        assert!(view.test_history.is_open());
        let runs = |session: &zen_core::Session| {
            session.state::<zen_workbench::run::Runs>().map_or(0, |runs| runs.runs().len())
        };
        assert_eq!(runs(&session), 1);
        screen.step(vec![enter], |ui| view.show(ui, &registry, &mut session));
        assert!(!view.test_history.is_open());
        assert_eq!(runs(&session), 2, "the entry is reopened as a run of its own");
        for _ in 0..2 {
            screen.step(Vec::new(), |ui| view.show(ui, &registry, &mut session));
        }
        let shown = labels(&screen.accessibility_tree(|ui| view.show(ui, &registry, &mut session)));
        assert!(shown.iter().filter(|label| label.as_str() == "cart tests").count() >= 2, "two Run tabs: {shown:?}");
        assert!(shown.contains(&"TestTotal".to_owned()), "the reopened tree: {shown:?}");
    }
}

#[cfg(test)]
mod exclusion_tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use egui::{Event, Pos2, vec2};
    use serde_json::json;
    use zen_i18n::t;

    fn product() -> super::IdeProduct {
        super::IdeProduct {
            folding_options: Vec::new(),
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
            debugger: None,
            project_models: Vec::new(),
            outliners: Vec::new(),
            run_providers: Vec::new(),
            language_servers: Vec::new(),
            templates: Vec::new(),
            inspectors: Vec::new(),
        }
    }

    struct Window {
        registry: zen_core::Registry,
        session: zen_core::Session,
        view: super::IdeView,
        screen: crate::offscreen::Offscreen,
    }

    impl Window {
        fn frame(&mut self, events: Vec<Event>) {
            let (view, registry, session) = (&mut self.view, &self.registry, &mut self.session);
            self.screen.step(events, |ui| view.show(ui, registry, session));
            view.requests.clear();
        }

        fn settle(&mut self) {
            for _ in 0..4 {
                self.frame(Vec::new());
            }
        }

        fn image(&mut self) -> crate::offscreen::Frame {
            let (view, registry, session) = (&mut self.view, &self.registry, &mut self.session);
            let image = self.screen.frame(Vec::new(), |ui| view.show(ui, registry, session));
            view.requests.clear();
            image
        }

        fn labels(&mut self) -> Vec<(String, Pos2)> {
            let (view, registry, session) = (&mut self.view, &self.registry, &mut self.session);
            let tree = self.screen.accessibility_tree(|ui| view.show(ui, registry, session));
            view.requests.clear();
            tree.nodes
                .iter()
                .filter_map(|(_, node)| {
                    let label = node.label().or(node.value())?.to_owned();
                    let bounds = node.bounds()?;
                    Some((
                        label,
                        Pos2::new(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32),
                    ))
                })
                .collect()
        }

        fn shown(&mut self, label: &str) -> bool {
            self.labels().iter().any(|(found, _)| found.trim() == label)
        }

        fn at(&mut self, label: &str) -> Pos2 {
            let labels = self.labels();
            labels.iter().rev().find(|(found, _)| found.trim() == label).map(|(_, at)| *at).unwrap_or_else(|| {
                panic!("no {label:?} in {:?}", labels.iter().map(|(found, _)| found).collect::<Vec<_>>())
            })
        }

        fn hover(&mut self, label: &str) {
            let at = self.at(label);
            self.frame(vec![Event::PointerMoved(at)]);
            self.settle();
        }

        fn click(&mut self, label: &str) {
            let at = self.at(label);
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
            self.settle();
        }

        fn open_menu(&mut self, path: &Path) {
            self.view.selection.select(path.to_owned());
            self.view.menu_request = Some((path.to_owned(), true));
            self.settle();
            self.hover(&t!("ide-tree-mark-directory-as"));
        }

        fn close_menu(&mut self) {
            let away = Pos2::new(700.0, 520.0);
            let button = |pressed| Event::PointerButton {
                pos: away,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(away), button(true)]);
            self.frame(vec![button(false)]);
            self.settle();
        }

        fn excluded(&self) -> Vec<PathBuf> {
            let state = self.session.state::<zen_workbench::project_settings::ProjectSettingsState>().unwrap();
            state.settings().excluded.clone()
        }
    }

    #[test]
    fn mark_directory_as_excluded_and_cancel_exclusion_from_the_tree_and_hide_excluded_files() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let languages: &[&str] = if snapshots.is_some() { &["en", "ru"] } else { &["current"] };
        for language in languages {
            if snapshots.is_some() {
                zen_i18n::set_language(Some(language));
            }
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().canonicalize().unwrap().join("shop");
            for (name, text) in [
                ("go.mod", "module shop\n"),
                ("main.go", "package main\n"),
                ("gen/x.go", "package gen\n"),
                ("vendor/modules.txt", "# example.com/m v1.0.0\n"),
                ("package.json", "{}\n"),
                ("node_modules/left-pad/index.js", "\n"),
            ] {
                let path = root.join(name);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, text).unwrap();
            }
            let gen_folder = root.join("gen");
            let mut registry = zen_core::Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let mut session = zen_core::Session::new();
            session
                .state_mut::<zen_workbench::project_settings::ProjectSettingsState>()
                .set_providers(vec![Arc::new(zen_jb_import::IdeaProject::with_user_home(None))]);
            registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
            let view = super::IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
            let screen = crate::offscreen::Offscreen::new(vec2(900.0, 560.0), 2.0);
            let mut window = Window { registry, session, view, screen };
            let snapshot = |name: &str, window: &mut Window| {
                if let Some(dir) = &snapshots {
                    let image = window.image();
                    crate::offscreen::write_png(&dir.join(format!("{name}-{language}.png")), &image).unwrap();
                }
            };
            window.settle();
            assert!(window.shown("gen") && window.shown("node_modules") && window.shown("vendor"));

            window.open_menu(&gen_folder);
            assert!(window.shown(&t!("ide-tree-mark-excluded")));
            assert!(!window.shown(&t!("ide-tree-cancel-exclusion")));
            snapshot("exclusion-menu-mark", &mut window);
            window.click(&t!("ide-tree-mark-excluded"));
            assert_eq!(window.excluded(), std::slice::from_ref(&gen_folder));
            let module = std::fs::read_to_string(root.join(".idea/shop.iml")).unwrap();
            assert!(module.contains("<excludeFolder url=\"file://$MODULE_DIR$/gen\" />"), "{module}");
            window.settle();
            assert!(window.shown(".idea"), "the tree lists the .idea folder the marking created");
            snapshot("exclusion-tree-shown", &mut window);

            window.open_menu(&gen_folder);
            assert!(window.shown(&t!("ide-tree-cancel-exclusion")));
            assert!(!window.shown(&t!("ide-tree-mark-excluded")));
            snapshot("exclusion-menu-cancel", &mut window);
            window.close_menu();

            window.open_menu(&root.join("node_modules"));
            assert!(window.shown(&t!("ide-tree-mark-excluded")), "excluded as an npm package's folder, not markable");
            window.close_menu();

            window.click(&t!("ide-project-options"));
            assert!(window.shown(&t!("ide-project-excluded-files")));
            snapshot("exclusion-options", &mut window);
            window.click(&t!("ide-project-excluded-files"));
            assert!(!window.view.show_excluded);
            window.close_menu();
            snapshot("exclusion-tree-hidden", &mut window);
            assert!(!window.shown("gen") && !window.shown("node_modules"), "excluded folders are hidden");
            assert!(window.shown("vendor") && window.shown("main.go"));

            window.view.show_excluded = true;
            window.settle();
            window.open_menu(&gen_folder);
            window.click(&t!("ide-tree-cancel-exclusion"));
            assert!(window.excluded().is_empty());
            let module = std::fs::read_to_string(root.join(".idea/shop.iml")).unwrap();
            assert!(module.contains("<content url=\"file://$MODULE_DIR$\" />"), "{module}");
        }
        if snapshots.is_some() {
            zen_i18n::set_language(Some("en"));
        }
    }

    #[test]
    fn mark_directory_as_resource_root_and_unmark_it_from_the_tree() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap().join("site");
        for (name, text) in [("main.go", "package main\n"), ("static/app.css", "\n")] {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let folder = root.join("static");
        let mut registry = zen_core::Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = zen_core::Session::new();
        session
            .state_mut::<zen_workbench::project_settings::ProjectSettingsState>()
            .set_providers(vec![Arc::new(zen_jb_import::IdeaProject::with_user_home(None))]);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
        let view = super::IdeView::new(Arc::new(product()), crate::plugins::Plugins::new(None));
        let screen = crate::offscreen::Offscreen::new(vec2(900.0, 560.0), 1.0);
        let mut window = Window { registry, session, view, screen };
        window.settle();

        window.open_menu(&folder);
        assert!(window.shown(&t!("ide-tree-mark-resource-root")));
        assert!(!window.shown(&t!("ide-tree-unmark-resource-root")));
        window.click(&t!("ide-tree-mark-resource-root"));
        let module = std::fs::read_to_string(root.join(".idea/site.iml")).unwrap();
        assert!(
            module.contains("<sourceFolder url=\"file://$MODULE_DIR$/static\" type=\"java-resource\" />"),
            "{module}"
        );
        window.settle();
        assert_eq!(window.view.tree_resource_roots, std::slice::from_ref(&folder), "the tree shows it as one");

        window.open_menu(&folder);
        assert!(window.shown(&t!("ide-tree-unmark-resource-root")));
        window.click(&t!("ide-tree-unmark-resource-root"));
        window.settle();
        assert!(window.view.tree_resource_roots.is_empty());
        let module = std::fs::read_to_string(root.join(".idea/site.iml")).unwrap();
        assert!(!module.contains("sourceFolder url"), "{module}");
    }
}

fn free_image_name(directory: &Path, name: &str) -> String {
    if !directory.join(name).exists() {
        return name.to_owned();
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, format!(".{extension}")),
        _ => (name, String::new()),
    };
    (1..=u32::MAX)
        .map(|number| format!("{stem}_{number}{extension}"))
        .find(|candidate| !directory.join(candidate).exists())
        .unwrap_or_else(|| name.to_owned())
}

fn new_entry_hint(directory: &Path, name: &str, same: bool, image: bool) -> Option<String> {
    let taken = !name.is_empty() && directory.join(name).exists() && !same;
    taken.then(|| if image { t!("ide-new-entry-exists-copy") } else { t!("ide-new-entry-exists") })
}

#[cfg(test)]
mod paste_image_tests {
    use super::*;

    #[test]
    fn a_pasted_image_is_offered_the_first_free_name() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(free_image_name(dir.path(), "image.png"), "image.png");
        std::fs::write(dir.path().join("image.png"), "").unwrap();
        assert_eq!(free_image_name(dir.path(), "image.png"), "image_1.png");
        std::fs::write(dir.path().join("image_1.png"), "").unwrap();
        assert_eq!(free_image_name(dir.path(), "image.png"), "image_2.png");
        std::fs::write(dir.path().join("image_3.png"), "").unwrap();
        assert_eq!(free_image_name(dir.path(), "image.png"), "image_2.png");
    }

    #[test]
    fn a_typed_name_that_exists_still_shows_the_hint() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("image.png"), "").unwrap();
        let hint = new_entry_hint(dir.path(), "image.png", false, true).unwrap();
        assert!(hint.contains("free name"), "{hint}");
        assert_eq!(new_entry_hint(dir.path(), "image_1.png", false, true), None);
        assert!(new_entry_hint(dir.path(), "image.png", false, false).is_some());
    }
}

#[cfg(test)]
mod dialog_tests {
    #[test]
    fn enter_in_a_single_line_field_is_noticed() {
        let ctx = egui::Context::default();
        let mut name = "image.png".to_owned();
        let mut seen = (false, false, false);
        let mut frame = |events: Vec<egui::Event>, focus: bool, seen: &mut (bool, bool, bool)| {
            let input = egui::RawInput { events, ..Default::default() };
            let mut output = ctx.run_ui(input, |ui| {
                let field = ui.add(egui::TextEdit::singleline(&mut name).id_salt("name"));
                if focus {
                    field.request_focus();
                }
                let consumed = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                *seen = (field.lost_focus(), consumed, ui.input(|i| i.key_pressed(egui::Key::Enter)));
            });
            output.textures_delta.clear();
        };
        frame(Vec::new(), true, &mut seen);
        frame(Vec::new(), false, &mut seen);
        let enter = egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![enter], false, &mut seen);
        assert!(seen.0, "the field gives up the keyboard on Enter");
        assert!(seen.1, "Enter is there to consume (lost_focus, consumed, pressed after = {seen:?})");
        assert!(!seen.2, "and nothing after sees it");
    }
}
