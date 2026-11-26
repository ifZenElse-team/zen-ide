use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zen_i18n::t;
use zen_project::settings::{CodeStyle, CodeStyleGeneral, DEFAULT_LANGUAGE};
use zen_workbench::language::code_vision;
use zen_workbench::language::inlay_hints::{self, HintKind};

pub(crate) const GUTTER_RUN_ICONS: &str = "editor.gutter.runIcons";

const DECLARED_PREFIX: &str = "declared:";

pub const DEFAULT_UI_FONT_SIZE: f32 = 13.0;

pub const UI_FONT_SIZES: RangeInclusive<f32> = 8.0..=32.0;

pub(crate) const DEFAULT_IDE_ZOOM: u16 = 100;

pub(crate) const IDE_ZOOMS: RangeInclusive<u16> = 50..=300;

pub(crate) const IDE_ZOOM_PRESETS: [u16; 9] = [70, 80, 90, 100, 110, 125, 150, 175, 200];

pub(crate) const DEFAULT_PRESENTATION_FONT_SIZE: f32 = 24.0;

pub(crate) const PRESENTATION_FONT_SIZES: RangeInclusive<f32> = 8.0..=96.0;

const DEFAULT_AUTOSAVE_IDLE_SECONDS: u32 = 15;

pub(crate) const AUTOSAVE_IDLE_SECONDS: RangeInclusive<u32> = 1..=3600;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct IdeSettings {
    pub mcp_enabled: bool,
    pub mcp_port: u16,
    pub mcp_language: String,
    pub mcp_redact_files: Vec<String>,
    pub open_in_new_window: bool,
    pub open_project_in: Option<OpenProjectIn>,
    pub default_project_directory: String,
    pub generators_network: bool,
    pub generator_settings: BTreeMap<String, String>,
    pub confirm_exit: bool,
    pub proxy: zen_net::ProxySettings,
    pub accept_untrusted_certificates: bool,
    pub whats_new_on_update: bool,
    pub show_tips_on_startup: bool,
    pub tip_index: u32,
    pub autosave_on_idle: bool,
    pub autosave_idle_seconds: u32,
    pub autosave_on_frame_deactivation: bool,
    pub language: Option<String>,
    pub show_fold_arrows: bool,
    pub fold_arrows: FoldArrows,
    pub fold_bottom_arrows: bool,
    pub fold_file_header: bool,
    pub fold_imports: bool,
    pub fold_doc_comments: bool,
    pub fold_method_bodies: bool,
    pub fold_custom_regions: bool,
    pub camel_humps: bool,
    pub camel_humps_on_double_click: bool,
    pub closing_tags: bool,
    pub gutter_run_icons: bool,
    pub highlight_usages_at_caret: bool,
    pub soft_wrap_files: String,
    pub editor_appearance: zen_workbench::visual_aids::VisualAidSettings,
    pub editor_tabs: zen_workbench::tabs::EditorTabSettings,
    pub todo: zen_workbench::todo_patterns::TodoSettings,
    pub breadcrumbs: bool,
    pub line_numbers: bool,
    pub line_numbers_mode: zen_workbench::line_numbers::LineNumbersMode,
    pub method_separators: bool,
    pub navigation_bar: NavigationBarPlacement,
    pub navigation_bar_members: bool,
    pub sticky_lines: bool,
    pub sticky_lines_max: usize,
    pub ligatures: bool,
    pub wheel_font_size: bool,
    pub wheel_font_size_in: WheelFontSize,
    pub zoom_indicator: bool,
    pub clipboard_limit: usize,
    pub console_buffer_override: bool,
    pub console_buffer_kb: usize,
    pub remove_trailing_spaces: bool,
    pub remove_trailing_spaces_on: zen_workbench::on_save::TrailingSpaces,
    pub keep_trailing_spaces_on_caret_line: bool,
    pub remove_trailing_blank_lines: bool,
    pub ensure_line_break_at_end: bool,
    pub theme: Option<String>,
    pub theme_follow_system: bool,
    pub theme_dark: Option<String>,
    pub theme_light: Option<String>,
    pub keymap: Option<String>,
    pub reformat_on_save: bool,
    pub optimize_imports_on_save: bool,
    pub reformat_on_save_files: Vec<String>,
    pub reformat_on_save_scope: zen_workbench::actions_on_save::ReformatScope,
    pub optimize_imports_on_save_files: Vec<String>,
    pub ui_font_size: Option<f32>,
    pub ide_zoom: u16,
    pub presentation_font_size: f32,
    pub editor_font: EditorFont,
    pub code_style: BTreeMap<String, CodeStyle>,
    pub line_separator: Option<zen_text::LineSeparator>,
    pub global_encoding: Option<String>,
    pub editor_config: bool,
    pub open_declaration_in_main: bool,
    pub detect_indents: bool,
    pub code_style_scheme: CodeStyleScheme,
    pub code_style_general: CodeStyleGeneral,
    pub code_style_copy: Option<String>,
    pub code_style_schemes: BTreeMap<String, IdeCodeStyle>,
    pub code_style_options: BTreeMap<String, zen_workbench::code_style_options::Values>,
    pub rearrange_on_reformat: bool,
    pub customization: crate::ide::customization::Customization,
    pub sql_format: zen_db::format::FormatOptions,
    pub notification_balloons: bool,
    pub balloon_groups_off: BTreeSet<String>,
    pub inlay_hints: InlayHintSettings,
    pub code_vision: CodeVisionSettings,
    pub spelling: SpellingSettings,
    pub git_avatars: bool,
    #[serde(default)]
    pub git_avatars_default_on: bool,
    pub search_wait_for_all: bool,
    pub extend_selection_whitespace: bool,
    pub search_text_on_all: bool,
    pub bookmarks_popup_line_only: bool,
    pub find_in_files_limit: usize,
    pub find_in_files_too_many: usize,
    pub project_view_scratches: bool,
    pub disable_double_modifiers: bool,
    pub markdown_hide_floating_toolbar: bool,
    pub run_confirm_rerun: bool,
    pub run_temporary_limit: usize,
    pub caffeine: zen_workbench::caffeine::CaffeineSettings,
    pub markdown: zen_workbench::preview::MarkdownSettings,
    pub terminal_links: zen_workbench::terminal::LinkChoices,
    pub terminal_restore: bool,
    pub login_shell_environment: bool,
    pub terminal_agents: Vec<zen_workbench::terminal::agents::AgentAdapter>,
    pub build_reload_after: zen_workbench::build_sync::ReloadAfter,
    pub power_save_mode: bool,
    pub remove_breakpoint_by_drag: bool,
    pub coroutine_debug_mode: bool,
    pub value_tooltip: bool,
    pub value_tooltip_delay: u64,
    pub recent_expressions: usize,
    pub inline_values: bool,
    pub run_to_cursor_inlay: bool,
    pub collection_views: bool,
    pub status_bar_widgets: BTreeMap<String, bool>,
    pub declared_settings: BTreeMap<String, bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OpenProjectIn {
    NewWindow,
    CurrentWindow,
    Ask,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FoldArrows {
    #[default]
    OnHover,
    Always,
}

impl FoldArrows {
    pub fn name(self) -> &'static str {
        match self {
            FoldArrows::OnHover => "onHover",
            FoldArrows::Always => "always",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WheelFontSize {
    #[default]
    ActiveEditor,
    AllEditors,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NavigationBarPlacement {
    Top,
    #[default]
    StatusBar,
    Hidden,
}

impl NavigationBarPlacement {
    pub fn name(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::StatusBar => "statusBar",
            Self::Hidden => "hidden",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct IdeCodeStyle {
    pub languages: BTreeMap<String, CodeStyle>,
    pub general: CodeStyleGeneral,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodeStyleScheme {
    #[default]
    Project,
    Default,
}

impl CodeStyleScheme {
    pub const ALL: [CodeStyleScheme; 2] = [CodeStyleScheme::Default, CodeStyleScheme::Project];

    pub fn id(self) -> &'static str {
        match self {
            CodeStyleScheme::Project => "project",
            CodeStyleScheme::Default => "default",
        }
    }

    pub fn name(self) -> String {
        match self {
            CodeStyleScheme::Project => zen_i18n::t!("settings-code-style-scheme-project"),
            CodeStyleScheme::Default => zen_i18n::t!("settings-code-style-scheme-default"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CodeVisionSettings {
    pub enabled: bool,
    pub usages: bool,
    pub authors: bool,
    #[serde(default)]
    pub authors_default_v2: bool,
}

impl Default for CodeVisionSettings {
    fn default() -> Self {
        Self { enabled: true, usages: true, authors: code_vision::AUTHORS_BY_DEFAULT, authors_default_v2: true }
    }
}

impl CodeVisionSettings {
    pub fn any(&self) -> bool {
        self.enabled && (self.usages || self.authors)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InlayHintSettings {
    pub enabled: bool,
    pub parameter_names: bool,
    pub field_names: bool,
    pub variable_types: bool,
    pub literal_types: bool,
    pub type_arguments: bool,
    pub constant_values: bool,
}

impl Default for InlayHintSettings {
    fn default() -> Self {
        let mut settings = Self {
            enabled: true,
            parameter_names: false,
            field_names: false,
            variable_types: false,
            literal_types: false,
            type_arguments: false,
            constant_values: false,
        };
        for kind in HintKind::ALL {
            *settings.kind_mut(kind) = kind.on_by_default();
        }
        settings
    }
}

impl InlayHintSettings {
    pub fn kind_mut(&mut self, kind: HintKind) -> &mut bool {
        match kind {
            HintKind::ParameterNames => &mut self.parameter_names,
            HintKind::FieldNames => &mut self.field_names,
            HintKind::VariableTypes => &mut self.variable_types,
            HintKind::LiteralTypes => &mut self.literal_types,
            HintKind::TypeArguments => &mut self.type_arguments,
            HintKind::ConstantValues => &mut self.constant_values,
        }
    }

    pub fn is_on(&self, kind: HintKind) -> bool {
        *self.clone().kind_mut(kind)
    }
}

impl Default for IdeSettings {
    fn default() -> Self {
        Self {
            mcp_enabled: false,
            mcp_port: 0,
            mcp_language: zen_i18n::SOURCE.to_owned(),
            mcp_redact_files: super::super::redact::DEFAULT_PATTERNS
                .iter()
                .map(|pattern| (*pattern).to_owned())
                .collect(),
            open_in_new_window: false,
            open_project_in: None,
            default_project_directory: String::new(),
            generators_network: true,
            generator_settings: BTreeMap::new(),
            confirm_exit: true,
            proxy: zen_net::ProxySettings::default(),
            accept_untrusted_certificates: false,
            whats_new_on_update: true,
            show_tips_on_startup: false,
            tip_index: 0,
            autosave_on_idle: true,
            autosave_idle_seconds: DEFAULT_AUTOSAVE_IDLE_SECONDS,
            autosave_on_frame_deactivation: true,
            language: None,
            show_fold_arrows: true,
            fold_arrows: FoldArrows::OnHover,
            fold_bottom_arrows: false,
            fold_file_header: false,
            fold_imports: true,
            fold_doc_comments: false,
            fold_method_bodies: false,
            fold_custom_regions: false,
            camel_humps: false,
            camel_humps_on_double_click: true,
            closing_tags: true,
            gutter_run_icons: true,
            highlight_usages_at_caret: true,
            soft_wrap_files: zen_workbench::soft_wrap::DEFAULT_SOFT_WRAP_FILES.to_owned(),
            editor_appearance: zen_workbench::visual_aids::VisualAidSettings::default(),
            breadcrumbs: true,
            line_numbers: true,
            line_numbers_mode: zen_workbench::line_numbers::LineNumbersMode::Absolute,
            method_separators: false,
            navigation_bar: NavigationBarPlacement::default(),
            navigation_bar_members: true,
            sticky_lines: true,
            sticky_lines_max: zen_workbench::sticky::DEFAULT_MAX,
            ligatures: false,
            wheel_font_size: false,
            wheel_font_size_in: WheelFontSize::ActiveEditor,
            zoom_indicator: true,
            clipboard_limit: zen_workbench::clipboard_history::DEFAULT_LIMIT,
            console_buffer_override: false,
            console_buffer_kb: zen_run::DEFAULT_BUFFER_KB,
            remove_trailing_spaces: true,
            remove_trailing_spaces_on: zen_workbench::on_save::TrailingSpaces::Modified,
            keep_trailing_spaces_on_caret_line: true,
            remove_trailing_blank_lines: false,
            ensure_line_break_at_end: false,
            theme: None,
            theme_follow_system: false,
            theme_dark: None,
            theme_light: None,
            keymap: None,
            reformat_on_save: false,
            optimize_imports_on_save: false,
            reformat_on_save_files: Vec::new(),
            reformat_on_save_scope: zen_workbench::actions_on_save::ReformatScope::WholeFile,
            optimize_imports_on_save_files: Vec::new(),
            ui_font_size: None,
            ide_zoom: DEFAULT_IDE_ZOOM,
            presentation_font_size: DEFAULT_PRESENTATION_FONT_SIZE,
            editor_font: EditorFont::default(),
            code_style: BTreeMap::new(),
            line_separator: None,
            global_encoding: None,
            editor_config: true,
            open_declaration_in_main: true,
            detect_indents: true,
            code_style_scheme: CodeStyleScheme::Project,
            code_style_general: CodeStyleGeneral::default(),
            code_style_copy: None,
            code_style_schemes: BTreeMap::new(),
            code_style_options: BTreeMap::new(),
            rearrange_on_reformat: false,
            customization: crate::ide::customization::Customization::default(),
            sql_format: zen_db::format::FormatOptions::default(),
            notification_balloons: true,
            balloon_groups_off: BTreeSet::new(),
            declared_settings: BTreeMap::new(),
            search_wait_for_all: false,
            extend_selection_whitespace: false,
            search_text_on_all: true,
            bookmarks_popup_line_only: true,
            find_in_files_limit: zen_workbench::search::DEFAULT_POPUP_LIMIT,
            find_in_files_too_many: zen_workbench::search::DEFAULT_TOO_MANY,
            project_view_scratches: true,
            disable_double_modifiers: false,
            markdown_hide_floating_toolbar: false,
            run_confirm_rerun: true,
            run_temporary_limit: zen_project::run::TEMPORARY_LIMIT,
            editor_tabs: zen_workbench::tabs::EditorTabSettings::default(),
            todo: zen_workbench::todo_patterns::TodoSettings::default(),
            caffeine: zen_workbench::caffeine::CaffeineSettings::default(),
            markdown: zen_workbench::preview::MarkdownSettings::default(),
            terminal_links: zen_workbench::terminal::LinkChoices::default(),
            terminal_restore: true,
            login_shell_environment: true,
            terminal_agents: Vec::new(),
            build_reload_after: zen_workbench::build_sync::ReloadAfter::default(),
            power_save_mode: false,
            remove_breakpoint_by_drag: false,
            coroutine_debug_mode: true,
            value_tooltip: true,
            value_tooltip_delay: zen_workbench::debug::VALUE_TOOLTIP_DELAY_DEFAULT,
            recent_expressions: zen_workbench::debug::RECENT_EXPRESSIONS_DEFAULT,
            inline_values: true,
            run_to_cursor_inlay: true,
            collection_views: true,
            status_bar_widgets: BTreeMap::new(),
            git_avatars: zen_workbench::vcs::avatars::AVATARS_BY_DEFAULT,
            git_avatars_default_on: true,
            inlay_hints: InlayHintSettings::default(),
            code_vision: CodeVisionSettings::default(),
            spelling: SpellingSettings::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SpellingSettings {
    pub enabled: bool,
    pub process_code: bool,
    pub process_literals: bool,
    pub process_comments: bool,
    pub custom_dictionaries: Vec<String>,
    pub use_single_dictionary: bool,
    pub saving_dictionary: zen_workbench::spelling::DictionaryLevel,
}

impl Default for SpellingSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            process_code: true,
            process_literals: true,
            process_comments: true,
            custom_dictionaries: Vec::new(),
            use_single_dictionary: false,
            saving_dictionary: zen_workbench::spelling::DictionaryLevel::Project,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EditorFont {
    pub family: Option<String>,
    pub size: f32,
    pub line_spacing: f32,
}

impl Default for EditorFont {
    fn default() -> Self {
        Self { family: None, size: zen_workbench::font_size::DEFAULT_SIZE, line_spacing: 1.2 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsDamage {
    pub file: PathBuf,
    pub detail: String,
    pub kept: Option<PathBuf>,
}

impl SettingsDamage {
    fn set_aside(path: &Path, detail: String) -> Self {
        let free = (1..).map(|copy| match copy {
            1 => path.with_extension("json.corrupt"),
            copy => path.with_extension(format!("json.corrupt.{copy}")),
        });
        let kept = free.into_iter().find(|name| !name.exists()).filter(|name| std::fs::rename(path, name).is_ok());
        Self { file: path.to_owned(), detail, kept }
    }

    pub fn notice(&self) -> String {
        match &self.kept {
            Some(kept) => t!("ide-settings-corrupt", detail = self.detail.clone(), file = kept.display().to_string()),
            None => {
                t!("ide-settings-corrupt-stuck", detail = self.detail.clone(), file = self.file.display().to_string())
            }
        }
    }
}

impl IdeSettings {
    #[cfg(test)]
    pub fn load(path: &Path, default_port: u16) -> Self {
        Self::load_checked(path, default_port).0
    }

    pub fn load_checked(path: &Path, default_port: u16) -> (Self, Option<SettingsDamage>) {
        let read = std::fs::read(path)
            .map_err(|error| (error.kind() != std::io::ErrorKind::NotFound).then(|| error.to_string()))
            .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|error| Some(error.to_string())));
        let (mut settings, damage): (Self, _) = match read {
            Ok(settings) => (settings, None),
            Err(None) => (Self::default(), None),
            Err(Some(detail)) => (Self::default(), Some(SettingsDamage::set_aside(path, detail))),
        };
        settings.customization = crate::ide::customization::Customization::load(path);
        let mut migrated = false;
        if !settings.code_vision.authors_default_v2 {
            settings.code_vision.authors = false;
            settings.code_vision.authors_default_v2 = true;
            migrated = true;
        }
        if !settings.git_avatars_default_on {
            settings.git_avatars = true;
            settings.git_avatars_default_on = true;
            migrated = true;
        }
        if migrated && path.is_file() && damage.is_none() {
            let _ = settings.save(path);
        }
        if settings.mcp_port == 0 {
            settings.mcp_port = default_port;
        }
        (settings, damage)
    }

    pub fn project_opening(&self) -> OpenProjectIn {
        self.open_project_in.unwrap_or(if self.open_in_new_window {
            OpenProjectIn::NewWindow
        } else {
            OpenProjectIn::CurrentWindow
        })
    }

    pub fn apply_to_session(&self, settings: &mut zen_core::Settings) {
        let _ = settings.set(zen_workbench::new_project::DEFAULT_DIRECTORY, &self.default_project_directory);
        let _ = settings.set(zen_workbench::folding::SHOW_ARROWS, self.show_fold_arrows);
        let _ = settings.set(zen_workbench::folding::ARROWS, self.fold_arrows.name());
        let _ = settings.set(zen_workbench::folding::BOTTOM_ARROWS, self.fold_bottom_arrows);
        let _ = settings.set(zen_modules::folding::FILE_HEADER, self.fold_file_header);
        let _ = settings.set(zen_workbench::folding::FOLD_IMPORTS, self.fold_imports);
        let _ = settings.set(zen_modules::folding::DOC_COMMENTS, self.fold_doc_comments);
        let _ = settings.set(zen_modules::folding::METHOD_BODIES, self.fold_method_bodies);
        let _ = settings.set(zen_modules::folding::CUSTOM_REGIONS, self.fold_custom_regions);
        let _ = settings.set(zen_workbench::editing::CAMEL_HUMPS, self.camel_humps);
        let _ = settings.set(zen_workbench::editing::CAMEL_HUMPS_ON_DOUBLE_CLICK, self.camel_humps_on_double_click);
        let _ = settings.set(zen_workbench::plugins::CLOSING_TAGS, self.closing_tags);
        let _ = settings.set(GUTTER_RUN_ICONS, self.gutter_run_icons);
        let _ = settings.set(zen_workbench::language::highlights::AT_CARET, self.highlight_usages_at_caret);
        let _ = settings.set(zen_workbench::soft_wrap::SOFT_WRAP_FILES, &self.soft_wrap_files);
        self.editor_appearance.apply_to(settings);
        let _ =
            settings.set(zen_workbench::markdown::format::HIDE_FLOATING_TOOLBAR, self.markdown_hide_floating_toolbar);
        let _ = settings.set(zen_workbench::language::breadcrumbs::SHOW_BREADCRUMBS, self.breadcrumbs);
        let _ = settings.set(zen_workbench::line_numbers::SHOW_LINE_NUMBERS, self.line_numbers);
        let _ = settings.set(zen_workbench::line_numbers::LINE_NUMBERS_MODE, self.line_numbers_mode);
        let _ = settings.set(zen_workbench::line_numbers::SHOW_METHOD_SEPARATORS, self.method_separators);
        let _ = settings.set(zen_workbench::language::navigation_bar::SHOW_MEMBERS, self.navigation_bar_members);
        let _ = settings.set(zen_workbench::sticky::ENABLED, self.sticky_lines);
        let _ = settings.set(zen_workbench::build_sync::RELOAD_AFTER, self.build_reload_after);
        let _ = settings.set(zen_workbench::sticky::MAX, self.sticky_lines_max);
        let _ = settings.set(zen_workbench::clipboard_history::LIMIT, self.clipboard_limit);
        let _ = settings.set(zen_workbench::run::CONSOLE_BUFFER_OVERRIDE, self.console_buffer_override);
        let _ = settings.set(zen_workbench::run::CONSOLE_BUFFER_KB, self.console_buffer_kb);
        let _ = settings.set(zen_workbench::on_save::REMOVE_TRAILING_SPACES, self.remove_trailing_spaces);
        let _ = settings.set(zen_workbench::on_save::REMOVE_TRAILING_SPACES_ON, self.remove_trailing_spaces_on);
        let _ = settings.set(zen_workbench::on_save::KEEP_CARET_LINES, self.keep_trailing_spaces_on_caret_line);
        let _ = settings.set(zen_workbench::on_save::REMOVE_TRAILING_BLANK_LINES, self.remove_trailing_blank_lines);
        let _ = settings.set(zen_workbench::on_save::ENSURE_LINE_BREAK, self.ensure_line_break_at_end);
        let _ = settings.set(zen_workbench::font_size::EDITOR_FONT_SIZE, self.editor_font.size);
        self.actions_on_save().apply_to(settings);
        let (languages, general) = self.ide_scheme();
        zen_workbench::code_style::apply(settings, languages);
        zen_workbench::code_style::apply_general(settings, general);
        zen_workbench::code_style_options::apply(settings, &self.code_style_options);
        let _ = settings.set(zen_workbench::code_style_options::REARRANGE_ON_REFORMAT, self.rearrange_on_reformat);
        let separator = self.line_separator.map_or(zen_workbench::line_separators::SYSTEM, zen_text::LineSeparator::id);
        let _ = settings.set(zen_workbench::line_separators::NEW_FILES, separator);
        let global_encoding = self.global_encoding.clone().unwrap_or_default();
        let _ = settings.set(zen_workbench::project_settings::GLOBAL_ENCODING, global_encoding);
        let _ = settings.set(zen_workbench::editor_config::ENABLED, self.editor_config);
        let _ = settings.set(zen_workbench::groups::OPEN_IN_MAIN_WINDOW, self.open_declaration_in_main);
        let _ = settings.set(zen_workbench::file_indent::DETECT_INDENTS, self.detect_indents);
        let project = self.code_style_scheme == CodeStyleScheme::Project;
        let _ = settings.set(zen_workbench::project_settings::PROJECT_CODE_STYLE, project);
        let _ = settings.set(zen_workbench::sql_format::OPTIONS, self.sql_format);
        let stale: Vec<String> = settings
            .iter()
            .map(|(key, _)| key.to_owned())
            .filter(|key| {
                key.starts_with(DECLARED_PREFIX) && !self.declared_settings.contains_key(&key[DECLARED_PREFIX.len()..])
            })
            .collect();
        for key in stale {
            settings.reset(&key);
            settings.reset(&key[DECLARED_PREFIX.len()..]);
        }
        for (key, value) in &self.declared_settings {
            let _ = settings.set(key, value);
            let _ = settings.set(&format!("{DECLARED_PREFIX}{key}"), true);
        }
        let _ = settings.set(inlay_hints::ENABLED, self.inlay_hints.enabled);
        for kind in HintKind::ALL {
            let _ = settings.set(&kind.setting(), self.inlay_hints.is_on(kind));
        }
        let _ = settings.set(code_vision::ENABLED, self.code_vision.enabled);
        let _ = settings.set(code_vision::USAGES, self.code_vision.usages);
        let _ = settings.set(code_vision::AUTHORS, self.code_vision.authors);
        let _ = settings.set(zen_workbench::vcs::avatars::AVATARS, self.git_avatars);
        let _ = settings.set(zen_workbench::new_project::GENERATORS_NETWORK, self.generators_network);
        let _ = settings.set(zen_workbench::new_project::GENERATOR_SETTINGS, &self.generator_settings);
        let _ = settings.set(zen_workbench::search_everywhere::WAIT_FOR_ALL, self.search_wait_for_all);
        let _ = settings.set(crate::keymap::DISABLE_DOUBLE_MODIFIERS, self.disable_double_modifiers);
        let _ = settings.set(zen_workbench::spelling::ENABLED, self.spelling.enabled);
        let _ = settings.set(zen_workbench::spelling::PROCESS_CODE, self.spelling.process_code);
        let _ = settings.set(zen_workbench::spelling::PROCESS_LITERALS, self.spelling.process_literals);
        let _ = settings.set(zen_workbench::spelling::PROCESS_COMMENTS, self.spelling.process_comments);
        let _ = settings.set(zen_workbench::spelling::CUSTOM_DICTIONARIES, &self.spelling.custom_dictionaries);
        let _ = settings.set(zen_workbench::spelling::SINGLE_DICTIONARY, self.spelling.use_single_dictionary);
        let _ = settings.set(zen_workbench::spelling::SAVING_DICTIONARY, self.spelling.saving_dictionary);
        let _ = settings.set(zen_workbench::extend_selection::SELECT_WHITESPACE, self.extend_selection_whitespace);
        let _ = settings.set(zen_workbench::search_everywhere::TEXT_ON_ALL, self.search_text_on_all);
        let _ = settings.set(zen_workbench::bookmarks::POPUP_LINE_ONLY, self.bookmarks_popup_line_only);
        let _ = settings.set(zen_workbench::search::POPUP_LIMIT, self.find_in_files_limit);
        let _ = settings.set(zen_workbench::search::TOO_MANY, self.find_in_files_too_many);
        let _ = settings.set(zen_workbench::run::TEMPORARY_LIMIT, self.run_temporary_limit);
        let _ = settings.set(zen_workbench::tabs::SETTINGS, &self.editor_tabs);
        let _ = settings.set(zen_workbench::todo_patterns::SETTINGS, &self.todo);
        let _ = settings.set(zen_workbench::power_save::ENABLED, self.power_save_mode);
        let _ = settings.set(zen_workbench::debug::REMOVE_BY_DRAG, self.remove_breakpoint_by_drag);
        let _ = settings.set(zen_workbench::java::coroutines::DEBUG_MODE, self.coroutine_debug_mode);
        let _ = settings.set(zen_workbench::terminal::RESTORE, self.terminal_restore);
        let _ = settings.set(zen_workbench::login_env::SETTING, self.login_shell_environment);
        zen_env::set_enabled(self.login_shell_environment);
        let _ = settings.set(zen_workbench::terminal::agents::AGENTS, &self.terminal_agents);
        let _ = settings.set(zen_workbench::debug::VALUE_TOOLTIP, self.value_tooltip);
        let _ = settings.set(zen_workbench::debug::VALUE_TOOLTIP_DELAY, self.value_tooltip_delay);
        let _ = settings.set(zen_workbench::debug::RECENT_EXPRESSIONS, self.recent_expressions);
        let _ = settings.set(zen_workbench::debug::INLINE_VALUES, self.inline_values);
        let _ = settings.set(zen_workbench::debug::RUN_TO_CURSOR_INLAY, self.run_to_cursor_inlay);
        let _ = settings.set(zen_workbench::debug::views::COLLECTION_VIEWS, self.collection_views);
    }

    pub fn save(&self, path: &Path) -> Result<(), zen_vfs::VfsError> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        self.customization.save(path)?;
        let mut value = serde_json::to_value(self).unwrap_or_default();
        if let Some(fields) = value.as_object_mut() {
            fields.remove("customization");
        }
        zen_vfs::write_atomic(path, &serde_json::to_vec_pretty(&value).unwrap_or_default())
    }

    pub fn ui_font_size(&self) -> f32 {
        self.ui_font_size.unwrap_or(DEFAULT_UI_FONT_SIZE)
    }

    pub fn ui_zoom(&self) -> f32 {
        self.font_zoom() * self.ide_zoom_factor()
    }

    pub fn font_zoom(&self) -> f32 {
        self.ui_font_size().clamp(*UI_FONT_SIZES.start(), *UI_FONT_SIZES.end()) / DEFAULT_UI_FONT_SIZE
    }

    pub fn ide_zoom_factor(&self) -> f32 {
        f32::from(self.ide_zoom.clamp(*IDE_ZOOMS.start(), *IDE_ZOOMS.end())) / 100.0
    }

    pub fn actions_on_save(&self) -> zen_workbench::actions_on_save::ActionsOnSave {
        zen_workbench::actions_on_save::ActionsOnSave {
            reformat: self.reformat_on_save,
            reformat_files: self.reformat_on_save_files.clone(),
            reformat_scope: self.reformat_on_save_scope,
            optimize_imports: self.optimize_imports_on_save,
            optimize_files: self.optimize_imports_on_save_files.clone(),
        }
    }

    pub fn set_actions_on_save(&mut self, actions: zen_workbench::actions_on_save::ActionsOnSave) {
        self.reformat_on_save = actions.reformat;
        self.reformat_on_save_files = actions.reformat_files;
        self.reformat_on_save_scope = actions.reformat_scope;
        self.optimize_imports_on_save = actions.optimize_imports;
        self.optimize_imports_on_save_files = actions.optimize_files;
    }

    pub fn code_style_for(&self, language: &str) -> CodeStyle {
        let (languages, _) = self.ide_scheme();
        if let Some(style) = languages.get(language) {
            return *style;
        }
        if CodeStyle::BUILT_IN.iter().any(|(id, _)| *id == language) {
            return CodeStyle::default_for(language);
        }
        languages.get(DEFAULT_LANGUAGE).copied().unwrap_or_else(|| CodeStyle::default_for(language))
    }

    pub fn selected_copy(&self) -> Option<&str> {
        let copy = self.code_style_copy.as_deref().filter(|_| self.code_style_scheme == CodeStyleScheme::Default)?;
        self.code_style_schemes.contains_key(copy).then_some(copy)
    }

    pub fn ide_scheme(&self) -> (&BTreeMap<String, CodeStyle>, &CodeStyleGeneral) {
        match self.selected_copy().and_then(|copy| self.code_style_schemes.get(copy)) {
            Some(copy) => (&copy.languages, &copy.general),
            None => (&self.code_style, &self.code_style_general),
        }
    }

    pub fn ide_scheme_mut(&mut self) -> (&mut BTreeMap<String, CodeStyle>, &mut CodeStyleGeneral) {
        let copy = self.selected_copy().map(str::to_owned);
        match copy.and_then(|copy| self.code_style_schemes.get_mut(&copy)) {
            Some(copy) => (&mut copy.languages, &mut copy.general),
            None => (&mut self.code_style, &mut self.code_style_general),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_generators_settings_are_saved_and_reach_the_session() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("settings.json");
        let mut settings = IdeSettings::load(&file, 1);
        assert!(settings.generators_network, "on by default");
        settings.generators_network = false;
        settings.generator_settings.insert("spring-boot.initializr.serviceUrl".to_owned(), "https://own".to_owned());
        settings.save(&file).unwrap();
        let settings = IdeSettings::load(&file, 1);
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(zen_workbench::new_project::GENERATORS_NETWORK), Some(false));
        let saved: BTreeMap<String, String> = session.get(zen_workbench::new_project::GENERATOR_SETTINGS).unwrap();
        assert_eq!(saved["spring-boot.initializr.serviceUrl"], "https://own");
    }

    #[test]
    fn the_sql_options_are_saved_and_reach_the_session() {
        let mut settings = IdeSettings::default();
        assert_eq!(serde_json::to_value(&settings).unwrap()["sqlFormat"]["keywordCase"], "asIs");
        settings.sql_format.keyword_case = zen_db::format::KeywordCase::Upper;
        settings.sql_format.commas = zen_db::format::Commas::Leading;
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(serde_json::from_value::<IdeSettings>(json).unwrap(), settings);
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        let applied = session.get::<zen_db::format::FormatOptions>(zen_workbench::sql_format::OPTIONS);
        assert_eq!(applied, Some(settings.sql_format));
    }

    #[test]
    fn the_code_style_scheme_is_saved_by_its_id_and_reaches_the_session() {
        let mut settings = IdeSettings { code_style_scheme: CodeStyleScheme::Default, ..IdeSettings::default() };
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(json["codeStyleScheme"], CodeStyleScheme::Default.id());
        assert_eq!(serde_json::from_value::<IdeSettings>(json).unwrap(), settings);
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(zen_workbench::project_settings::PROJECT_CODE_STYLE), Some(false));
        settings.code_style_scheme = CodeStyleScheme::Project;
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(zen_workbench::project_settings::PROJECT_CODE_STYLE), Some(true));
        assert_eq!(CodeStyleScheme::ALL.map(CodeStyleScheme::id), ["default", "project"]);
    }

    #[test]
    fn code_style_options_and_rearrange_on_reformat_reach_the_session_and_go_when_removed() {
        let mut settings = IdeSettings::default();
        settings
            .code_style_options
            .insert("rust".to_owned(), BTreeMap::from([("max_width".to_owned(), serde_json::json!(80))]));
        settings.rearrange_on_reformat = true;
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(json["codeStyleOptions"]["rust"]["max_width"], 80);
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<u32>("editor.codeStyle.rust.max_width"), Some(80));
        assert_eq!(session.get::<bool>(zen_workbench::code_style_options::REARRANGE_ON_REFORMAT), Some(true));
        settings.code_style_options.clear();
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<u32>("editor.codeStyle.rust.max_width"), None);
    }

    #[test]
    fn new_fields_have_defaults_and_older_files_load() {
        let older = r#"{"mcpEnabled": true, "mcpPort": 7000, "foldImports": false, "theme": "monokai"}"#;
        let settings: IdeSettings = serde_json::from_str(older).unwrap();
        assert!(settings.mcp_enabled && !settings.fold_imports);
        assert_eq!(settings.ui_font_size, None);
        assert_eq!(settings.ui_font_size(), 13.0);
        assert_eq!(settings.editor_font, EditorFont { family: None, size: 13.0, line_spacing: 1.2 });
        assert!(settings.code_style.is_empty());
        assert_eq!(settings.code_style_scheme, CodeStyleScheme::Project);
        assert!(settings.inlay_hints.enabled && settings.inlay_hints.parameter_names);
        assert!(!settings.inlay_hints.variable_types);
        assert_eq!(
            settings,
            IdeSettings {
                mcp_enabled: true,
                mcp_port: 7000,
                fold_imports: false,
                theme: Some("monokai".to_owned()),
                git_avatars_default_on: false,
                ..IdeSettings::default()
            }
        );
    }

    #[test]
    fn the_wheel_changes_no_font_size_by_default_and_the_zoom_indicator_shows() {
        let settings: IdeSettings = serde_json::from_str("{}").unwrap();
        assert!(!settings.wheel_font_size && settings.zoom_indicator);
        assert_eq!(settings.wheel_font_size_in, WheelFontSize::ActiveEditor);
        let changed = IdeSettings {
            wheel_font_size: true,
            wheel_font_size_in: WheelFontSize::AllEditors,
            zoom_indicator: false,
            ..IdeSettings::default()
        };
        let json = serde_json::to_value(&changed).unwrap();
        assert_eq!(
            (json["wheelFontSize"].clone(), json["wheelFontSizeIn"].clone()),
            (true.into(), "allEditors".into())
        );
        assert_eq!(json["zoomIndicator"], false);
        assert_eq!(serde_json::from_value::<IdeSettings>(json).unwrap(), changed);
    }

    #[test]
    fn the_console_cycle_buffer_reaches_the_session_only_when_overridden() {
        let mut settings = IdeSettings { console_buffer_kb: 16, ..IdeSettings::default() };
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        assert_eq!(zen_workbench::run::console_limit(&session), zen_run::DEFAULT_BUFFER_KB * 1024);
        settings.console_buffer_override = true;
        settings.apply_to_session(&mut session);
        assert_eq!(zen_workbench::run::console_limit(&session), 16 * 1024);
    }

    #[test]
    fn the_editor_font_size_reaches_the_session() {
        let mut settings = IdeSettings::default();
        settings.editor_font.size = 17.0;
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        assert_eq!(zen_workbench::font_size::settings_size(&session), 17.0);
    }

    #[test]
    fn new_fields_round_trip_in_camel_case() {
        let mut settings = IdeSettings { ui_font_size: Some(15.0), ..IdeSettings::default() };
        settings.editor_font = EditorFont { family: Some("Fira Code".to_owned()), size: 14.0, line_spacing: 1.5 };
        settings.code_style.insert("go".to_owned(), CodeStyle { use_tabs: false, tab_size: 8, indent: 2 });
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(json["uiFontSize"], 15.0);
        assert_eq!(json["editorFont"]["family"], "Fira Code");
        assert_eq!(json["editorFont"]["lineSpacing"], 1.5);
        assert_eq!(json["codeStyle"]["go"]["useTabs"], false);
        assert_eq!(json["codeStyle"]["go"]["tabSize"], 8);
        assert_eq!(serde_json::from_value::<IdeSettings>(json).unwrap(), settings);
        let partial: IdeSettings = serde_json::from_str(r#"{"editorFont": {"size": 16}}"#).unwrap();
        assert_eq!(partial.editor_font, EditorFont { family: None, size: 16.0, line_spacing: 1.2 });
    }

    #[test]
    fn the_ui_zoom_follows_the_ui_font_size() {
        let zoom = |size| IdeSettings { ui_font_size: size, ..IdeSettings::default() }.ui_zoom();
        assert_eq!(zoom(None), 1.0);
        assert_eq!(zoom(Some(13.0)), 1.0);
        assert_eq!(zoom(Some(26.0)), 2.0);
        assert_eq!(zoom(Some(6.5)), 8.0 / 13.0, "a size below the range takes the smallest");
        assert_eq!(zoom(Some(64.0)), 32.0 / 13.0, "a size above the range takes the largest");
    }

    #[test]
    fn the_ide_zoom_scales_on_top_of_the_ui_font_size() {
        let settings = |size, zoom| IdeSettings { ui_font_size: size, ide_zoom: zoom, ..IdeSettings::default() };
        assert_eq!(IdeSettings::default().ide_zoom, 100);
        assert_eq!(settings(None, 150).ui_zoom(), 1.5);
        assert_eq!(settings(Some(26.0), 150).ui_zoom(), 3.0);
        assert_eq!(settings(Some(26.0), 150).font_zoom(), 2.0, "the font's part does not change with the zoom");
        assert_eq!(settings(None, 10).ide_zoom_factor(), 0.5, "a zoom below the range takes the smallest");
        assert_eq!(settings(None, 900).ide_zoom_factor(), 3.0, "a zoom above the range takes the largest");
        let older: IdeSettings = serde_json::from_str(r#"{"uiFontSize": 15}"#).unwrap();
        assert_eq!(older.ide_zoom, 100, "a file from before the zoom is at 100%");
        assert_eq!(serde_json::to_value(settings(None, 125)).unwrap()["ideZoom"], 125);
    }

    #[test]
    fn customizations_are_saved_beside_the_settings_not_in_them() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let mut settings = IdeSettings::default();
        settings.customization.insert("mainToolbar", None, "editor.code.reformat");
        settings.save(&file).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert!(json.get("customization").is_none());
        assert!(dir.path().join("customization.json").is_file());
        assert_eq!(IdeSettings::load(&file, 1).customization, settings.customization);
    }

    #[test]
    fn the_settings_override_the_built_in_code_style() {
        let mut settings = IdeSettings::default();
        assert_eq!(settings.code_style_for("go"), CodeStyle::default_for("go"));
        assert_eq!(settings.code_style_for("rust"), CodeStyle::default_for(DEFAULT_LANGUAGE));
        let custom = CodeStyle { use_tabs: true, tab_size: 3, indent: 3 };
        settings.code_style.insert(DEFAULT_LANGUAGE.to_owned(), custom);
        assert_eq!(settings.code_style_for("rust"), custom, "the user's default covers other languages");
        assert_eq!(
            settings.code_style_for("json"),
            CodeStyle::default_for("json"),
            "a language of the table keeps its own"
        );
        let go = CodeStyle { use_tabs: false, tab_size: 2, indent: 2 };
        settings.code_style.insert("go".to_owned(), go);
        assert_eq!(settings.code_style_for("go"), go);
    }

    #[test]
    fn code_vision_is_on_by_default_and_its_switches_reach_the_session() {
        let older: IdeSettings = serde_json::from_str(r#"{"mcpEnabled": true}"#).unwrap();
        assert_eq!(older.code_vision, CodeVisionSettings::default());
        assert!(older.code_vision.any());
        assert!(older.code_vision.usages && !older.code_vision.authors, "the authors are off by default");
        let mut settings = IdeSettings::default();
        settings.code_vision.authors = true;
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        assert!(code_vision::Switches::from_settings(&session).authors);
        settings.code_vision.authors = false;
        settings.apply_to_session(&mut session);
        let switches = code_vision::Switches::from_settings(&session);
        assert_eq!(switches, code_vision::Switches { usages: true, authors: false });
        settings.code_vision.enabled = false;
        settings.apply_to_session(&mut session);
        assert!(!code_vision::Switches::from_settings(&session).any());
        assert!(!settings.code_vision.any());
        assert_eq!(serde_json::to_value(&settings).unwrap()["codeVision"]["authors"], false);
    }

    #[test]
    fn camel_humps_are_off_by_default_and_reach_the_session() {
        let mut settings: IdeSettings = serde_json::from_str("{}").unwrap();
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        assert!(!zen_workbench::editing::camel_humps(&session));
        assert!(!zen_workbench::editing::camel_humps_on_double_click(&session));
        settings.camel_humps = true;
        settings.apply_to_session(&mut session);
        assert!(zen_workbench::editing::camel_humps_on_double_click(&session));
        settings.camel_humps_on_double_click = false;
        settings.apply_to_session(&mut session);
        assert!(zen_workbench::editing::camel_humps(&session));
        assert!(!zen_workbench::editing::camel_humps_on_double_click(&session));
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!((json["camelHumps"].clone(), json["camelHumpsOnDoubleClick"].clone()), (true.into(), false.into()));
    }

    #[test]
    fn whitespace_with_extend_selection_is_off_by_default_and_reaches_the_session() {
        let key = zen_workbench::extend_selection::SELECT_WHITESPACE;
        let mut settings: IdeSettings = serde_json::from_str("{}").unwrap();
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(key), Some(false));
        settings.extend_selection_whitespace = true;
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(key), Some(true));
        assert_eq!(serde_json::to_value(&settings).unwrap()["extendSelectionWhitespace"], true);
    }

    #[test]
    fn the_bookmarks_popup_shows_only_line_bookmarks_by_default_and_the_choice_reaches_the_session() {
        let key = zen_workbench::bookmarks::POPUP_LINE_ONLY;
        let mut settings: IdeSettings = serde_json::from_str("{}").unwrap();
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(key), Some(true));
        settings.bookmarks_popup_line_only = false;
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(key), Some(false));
        assert_eq!(serde_json::to_value(&settings).unwrap()["bookmarksPopupLineOnly"], false);
    }

    #[test]
    fn removing_a_breakpoint_by_a_click_is_the_default_and_dragging_reaches_the_session() {
        let key = zen_workbench::debug::REMOVE_BY_DRAG;
        let mut settings: IdeSettings = serde_json::from_str("{}").unwrap();
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(key), Some(false));
        settings.remove_breakpoint_by_drag = true;
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(key), Some(true));
        assert_eq!(serde_json::to_value(&settings).unwrap()["removeBreakpointByDrag"], true);
    }

    #[test]
    fn coroutine_debug_mode_is_on_by_default_and_reaches_the_session() {
        let key = zen_workbench::java::coroutines::DEBUG_MODE;
        let mut settings: IdeSettings = serde_json::from_str("{}").unwrap();
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(key), Some(true));
        settings.coroutine_debug_mode = false;
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(key), Some(false));
        assert_eq!(serde_json::to_value(&settings).unwrap()["coroutineDebugMode"], false);
    }

    #[test]
    fn the_login_shell_environment_is_on_by_default_and_reaches_the_session() {
        let key = zen_workbench::login_env::SETTING;
        let mut settings: IdeSettings = serde_json::from_str("{}").unwrap();
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(key), Some(true));
        settings.login_shell_environment = false;
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(key), Some(false));
        assert!(!zen_env::enabled(), "turning it off takes effect at once");
        assert_eq!(serde_json::to_value(&settings).unwrap()["loginShellEnvironment"], false);
        settings.login_shell_environment = true;
        settings.apply_to_session(&mut session);
        assert!(zen_env::enabled());
    }

    #[test]
    fn restoring_terminal_tabs_is_on_by_default_and_reaches_the_session() {
        let key = zen_workbench::terminal::RESTORE;
        let mut settings: IdeSettings = serde_json::from_str("{}").unwrap();
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(key), Some(true));
        settings.terminal_restore = false;
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(key), Some(false));
        assert_eq!(serde_json::to_value(&settings).unwrap()["terminalRestore"], false);
    }

    #[test]
    fn value_tooltips_and_recent_expressions_have_their_defaults_and_reach_the_session() {
        let mut settings: IdeSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(
            (settings.value_tooltip, settings.value_tooltip_delay, settings.recent_expressions),
            (true, 700, 50)
        );
        settings.value_tooltip = false;
        settings.value_tooltip_delay = 300;
        settings.recent_expressions = 10;
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(zen_workbench::debug::VALUE_TOOLTIP), Some(false));
        assert_eq!(session.get::<u64>(zen_workbench::debug::VALUE_TOOLTIP_DELAY), Some(300));
        assert_eq!(session.get::<usize>(zen_workbench::debug::RECENT_EXPRESSIONS), Some(10));
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(
            (&json["valueTooltip"], &json["valueTooltipDelay"], &json["recentExpressions"]),
            (&serde_json::json!(false), &serde_json::json!(300), &serde_json::json!(10))
        );
    }

    #[test]
    fn inline_values_and_the_run_to_cursor_inlay_are_on_by_default_and_reach_the_session() {
        let mut settings: IdeSettings = serde_json::from_str("{}").unwrap();
        assert!(settings.inline_values && settings.run_to_cursor_inlay && settings.collection_views);
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(zen_workbench::debug::INLINE_VALUES), Some(true));
        assert_eq!(session.get::<bool>(zen_workbench::debug::views::COLLECTION_VIEWS), Some(true));
        settings.collection_views = false;
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(zen_workbench::debug::views::COLLECTION_VIEWS), Some(false));
        settings.inline_values = false;
        settings.run_to_cursor_inlay = false;
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(zen_workbench::debug::INLINE_VALUES), Some(false));
        assert_eq!(session.get::<bool>(zen_workbench::debug::RUN_TO_CURSOR_INLAY), Some(false));
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(
            (&json["inlineValues"], &json["runToCursorInlay"]),
            (&serde_json::json!(false), &serde_json::json!(false))
        );
    }

    #[test]
    fn waiting_for_all_search_contributors_is_off_by_default_and_reaches_the_session() {
        let key = zen_workbench::search_everywhere::WAIT_FOR_ALL;
        let mut settings: IdeSettings = serde_json::from_str("{}").unwrap();
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(key), Some(false));
        settings.search_wait_for_all = true;
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(key), Some(true));
        assert_eq!(serde_json::to_value(&settings).unwrap()["searchWaitForAll"], true);
    }

    #[test]
    fn double_modifier_shortcuts_are_on_by_default_and_their_switch_reaches_the_session() {
        let key = crate::keymap::DISABLE_DOUBLE_MODIFIERS;
        let mut settings: IdeSettings = serde_json::from_str("{}").unwrap();
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(key), Some(false));
        settings.disable_double_modifiers = true;
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(key), Some(true));
        assert_eq!(serde_json::to_value(&settings).unwrap()["disableDoubleModifiers"], true);
    }

    #[test]
    fn text_on_all_is_on_by_default_and_reaches_the_session() {
        let key = zen_workbench::search_everywhere::TEXT_ON_ALL;
        let mut settings: IdeSettings = serde_json::from_str("{}").unwrap();
        assert!(settings.search_text_on_all);
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(key), Some(true));
        settings.search_text_on_all = false;
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<bool>(key), Some(false));
        assert_eq!(serde_json::to_value(&settings).unwrap()["searchTextOnAll"], false);
    }

    #[test]
    fn the_find_in_files_limits_are_100_and_1000_by_default_and_reach_the_session() {
        let mut settings: IdeSettings = serde_json::from_str("{}").unwrap();
        assert_eq!((settings.find_in_files_limit, settings.find_in_files_too_many), (100, 1000));
        assert_eq!((settings.run_confirm_rerun, settings.run_temporary_limit), (true, 5));
        settings.find_in_files_limit = 250;
        let mut session = zen_core::Settings::default();
        settings.apply_to_session(&mut session);
        assert_eq!(session.get::<usize>(zen_workbench::search::POPUP_LIMIT), Some(250));
        assert_eq!(session.get::<usize>(zen_workbench::search::TOO_MANY), Some(1000));
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(
            (&json["findInFilesLimit"], &json["findInFilesTooMany"]),
            (&serde_json::json!(250), &serde_json::json!(1000))
        );
    }

    #[test]
    fn older_files_have_the_code_authors_turned_off_once_and_a_choice_after_that_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let saved = |file: &Path| serde_json::from_slice::<serde_json::Value>(&std::fs::read(file).unwrap()).unwrap();
        std::fs::write(&file, r#"{"mcpPort": 0, "codeVision": {"enabled": true, "usages": true, "authors": true}}"#)
            .unwrap();
        let migrated = IdeSettings::load(&file, 1);
        assert!(!migrated.code_vision.authors, "turned off once");
        assert!(migrated.code_vision.authors_default_v2);
        assert_eq!(saved(&file)["codeVision"]["authors"], false);
        assert_eq!(saved(&file)["codeVision"]["authorsDefaultV2"], true, "the marker is written");
        assert_eq!(saved(&file)["mcpPort"], 0, "the default port stays unwritten");
        let mut chosen = migrated;
        chosen.code_vision.authors = true;
        chosen.save(&file).unwrap();
        assert!(IdeSettings::load(&file, 1).code_vision.authors);
        std::fs::write(&file, r#"{"codeVision": {"authors": true, "authorsDefaultV2": true}}"#).unwrap();
        assert!(IdeSettings::load(&file, 1).code_vision.authors, "a file with the marker keeps its choice");
        let fresh = IdeSettings::load(&dir.path().join("missing.json"), 1);
        assert!(!fresh.code_vision.authors && fresh.code_vision.authors_default_v2);
        assert!(!dir.path().join("missing.json").exists(), "nothing written for a missing file");
    }

    #[test]
    fn the_authors_pictures_are_on_by_default_and_turned_on_once_in_older_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let saved = |file: &Path| serde_json::from_slice::<serde_json::Value>(&std::fs::read(file).unwrap()).unwrap();
        assert!(IdeSettings::default().git_avatars, "on in fresh settings");
        std::fs::write(&file, r#"{"gitAvatars": false, "codeVision": {"authorsDefaultV2": true}}"#).unwrap();
        let migrated = IdeSettings::load(&file, 1);
        assert!(migrated.git_avatars && migrated.git_avatars_default_on, "turned on once");
        assert_eq!(saved(&file)["gitAvatars"], true);
        assert_eq!(saved(&file)["gitAvatarsDefaultOn"], true, "the marker is written");
        let mut chosen = migrated;
        chosen.git_avatars = false;
        chosen.save(&file).unwrap();
        assert!(!IdeSettings::load(&file, 1).git_avatars, "a choice after the switch is kept");
    }

    #[test]
    fn a_damaged_settings_file_is_kept_aside_and_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        std::fs::write(&file, r#"{"ligatures": true,"#).unwrap();
        let (settings, damage) = IdeSettings::load_checked(&file, 7);
        let damage = damage.unwrap();
        let kept = dir.path().join("settings.json.corrupt");
        assert_eq!(damage.kept.as_ref(), Some(&kept));
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), r#"{"ligatures": true,"#);
        assert!(!file.exists(), "the migration writes nothing over it");
        assert_eq!(settings.mcp_port, 7, "defaults are used");
    }

    #[test]
    fn a_setting_of_the_wrong_type_counts_as_damage() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        std::fs::write(&file, r#"{"ligatures": "yes"}"#).unwrap();
        assert!(IdeSettings::load_checked(&file, 1).1.is_some());
        assert!(dir.path().join("settings.json.corrupt").exists());
    }

    #[test]
    fn a_second_damaged_file_does_not_replace_the_first_copy() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        std::fs::write(dir.path().join("settings.json.corrupt"), "first").unwrap();
        std::fs::write(&file, "second").unwrap();
        let kept = IdeSettings::load_checked(&file, 1).1.unwrap().kept.unwrap();
        assert_eq!(kept, dir.path().join("settings.json.corrupt.2"));
        assert_eq!(std::fs::read_to_string(dir.path().join("settings.json.corrupt")).unwrap(), "first");
        assert_eq!(std::fs::read_to_string(kept).unwrap(), "second");
    }

    #[test]
    fn a_missing_settings_file_is_not_damage() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(IdeSettings::load_checked(&dir.path().join("settings.json"), 1).1, None);
    }
}
