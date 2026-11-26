use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::textmate::{self, FontStyle, Rule, Selector};
use crate::format::id_from_name;
use crate::{
    EditorKey as E, FormatError, ImportError, ImportReport, Key, MAX_DEPTH, ResolvedTheme, Rgba, Style, SyntaxKey as S,
    TerminalKey as T, Theme, ThemeImporter, UiKey as U, Underline, base_id, jsonc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Ui(U),
    Editor(E),
    Terminal(T),
}

impl Target {
    pub fn keeps_alpha(self) -> bool {
        matches!(
            self,
            Target::Editor(
                E::Selection
                    | E::CurrentLine
                    | E::SearchMatch
                    | E::SymbolHighlight
                    | E::SymbolHighlightWrite
                    | E::BracketMatchedBackground
                    | E::FoldMarker
            ) | Target::Ui(U::ListHover | U::ListSelection | U::PopupHover | U::PopupShadow | U::PopupCodeBackground)
        )
    }

    pub fn id(self) -> String {
        match self {
            Target::Ui(key) => format!("{}.{}", U::SECTION, key.id()),
            Target::Editor(key) => format!("{}.{}", E::SECTION, key.id()),
            Target::Terminal(key) => format!("{}.{}", T::SECTION, key.id()),
        }
    }
}

pub const COLORS: &[(&str, Target)] = &[
    ("editor.background", Target::Editor(E::Background)),
    ("editor.foreground", Target::Editor(E::Foreground)),
    ("editorCursor.foreground", Target::Editor(E::Caret)),
    ("editor.selectionBackground", Target::Editor(E::Selection)),
    ("editor.lineHighlightBackground", Target::Editor(E::CurrentLine)),
    ("editorLineNumber.foreground", Target::Editor(E::LineNumber)),
    ("editor.findMatchBackground", Target::Editor(E::SearchMatch)),
    ("editorOverviewRuler.findMatchForeground", Target::Editor(E::SearchMark)),
    ("editor.findMatchHighlightBackground", Target::Editor(E::SearchMark)),
    ("editor.foldBackground", Target::Editor(E::FoldMarker)),
    ("editor.wordHighlightBackground", Target::Editor(E::SymbolHighlight)),
    ("editor.wordHighlightStrongBackground", Target::Editor(E::SymbolHighlightWrite)),
    ("editorBracketMatch.background", Target::Editor(E::BracketMatchedBackground)),
    ("editorStickyScroll.background", Target::Editor(E::StickyBackground)),
    ("editorStickyScroll.border", Target::Editor(E::StickyBorder)),
    ("editorInlayHint.background", Target::Editor(E::InlayHintBackground)),
    ("editorInlayHint.foreground", Target::Editor(E::InlayHintForeground)),
    ("editorGutter.addedBackground", Target::Editor(E::GutterAdded)),
    ("editorGutter.modifiedBackground", Target::Editor(E::GutterModified)),
    ("editorGutter.deletedBackground", Target::Editor(E::GutterDeleted)),
    ("diffEditor.insertedLineBackground", Target::Editor(E::DiffInserted)),
    ("diffEditor.removedLineBackground", Target::Editor(E::DiffDeleted)),
    ("diffEditor.insertedTextBackground", Target::Editor(E::DiffInsertedWord)),
    ("diffEditor.removedTextBackground", Target::Editor(E::DiffDeletedWord)),
    ("diffEditor.diagonalFill", Target::Editor(E::DiffFiller)),
    ("editorWhitespace.foreground", Target::Editor(E::Whitespace)),
    ("editorIndentGuide.background1", Target::Editor(E::IndentGuide)),
    ("editorIndentGuide.background", Target::Editor(E::IndentGuide)),
    ("editorIndentGuide.activeBackground1", Target::Editor(E::IndentGuideSelected)),
    ("editorIndentGuide.activeBackground", Target::Editor(E::IndentGuideSelected)),
    ("editorRuler.foreground", Target::Editor(E::RightMargin)),
    ("editorError.foreground", Target::Editor(E::DiagnosticError)),
    ("editorWarning.foreground", Target::Editor(E::DiagnosticWarning)),
    ("editorInfo.foreground", Target::Editor(E::DiagnosticInfo)),
    ("editorHint.foreground", Target::Editor(E::DiagnosticHint)),
    ("sideBar.background", Target::Ui(U::WindowBackground)),
    ("titleBar.activeBackground", Target::Ui(U::WindowBackground)),
    ("statusBar.background", Target::Ui(U::WindowBackground)),
    ("panel.background", Target::Ui(U::IslandBackground)),
    ("editorGroupHeader.tabsBackground", Target::Ui(U::IslandBackground)),
    ("foreground", Target::Ui(U::TextPrimary)),
    ("descriptionForeground", Target::Ui(U::TextSecondary)),
    ("errorForeground", Target::Ui(U::TextError)),
    ("editorWarning.foreground", Target::Ui(U::TextWarning)),
    ("textLink.foreground", Target::Ui(U::Link)),
    ("button.background", Target::Ui(U::Accent)),
    ("button.foreground", Target::Ui(U::AccentForeground)),
    ("list.hoverBackground", Target::Ui(U::ListHover)),
    ("list.activeSelectionBackground", Target::Ui(U::ListSelection)),
    ("list.inactiveSelectionBackground", Target::Ui(U::ListSelection)),
    ("editorWidget.background", Target::Ui(U::PopupBackground)),
    ("menu.background", Target::Ui(U::PopupBackground)),
    ("editorWidget.border", Target::Ui(U::PopupBorder)),
    ("menu.border", Target::Ui(U::PopupBorder)),
    ("menu.selectionBackground", Target::Ui(U::PopupHover)),
    ("quickInputList.focusBackground", Target::Ui(U::PopupHover)),
    ("widget.shadow", Target::Ui(U::PopupShadow)),
    ("textCodeBlock.background", Target::Ui(U::PopupCodeBackground)),
    ("input.background", Target::Ui(U::TextFieldBackground)),
    ("input.border", Target::Ui(U::TextFieldBorder)),
    ("button.border", Target::Ui(U::ButtonSecondaryBorder)),
    ("tab.border", Target::Ui(U::TabBorder)),
    ("tab.activeBackground", Target::Ui(U::TabActiveBackground)),
    ("tab.activeBorderTop", Target::Ui(U::TabActiveBorder)),
    ("tab.activeBorder", Target::Ui(U::TabActiveBorder)),
    ("gitDecoration.modifiedResourceForeground", Target::Ui(U::TabModified)),
    ("gitDecoration.ignoredResourceForeground", Target::Ui(U::TreeExcludedText)),
    ("gitDecoration.modifiedResourceForeground", Target::Ui(U::VcsModified)),
    ("gitDecoration.addedResourceForeground", Target::Ui(U::VcsAdded)),
    ("gitDecoration.deletedResourceForeground", Target::Ui(U::VcsDeleted)),
    ("gitDecoration.untrackedResourceForeground", Target::Ui(U::VcsUnversioned)),
    ("gitDecoration.ignoredResourceForeground", Target::Ui(U::VcsIgnored)),
    ("gitDecoration.conflictingResourceForeground", Target::Ui(U::VcsConflict)),
    ("scmGraph.foreground1", Target::Ui(U::VcsGraph0)),
    ("scmGraph.foreground2", Target::Ui(U::VcsGraph1)),
    ("scmGraph.foreground3", Target::Ui(U::VcsGraph2)),
    ("scmGraph.foreground4", Target::Ui(U::VcsGraph3)),
    ("scmGraph.foreground5", Target::Ui(U::VcsGraph4)),
    ("banner.background", Target::Ui(U::BannerBackground)),
    ("terminal.background", Target::Terminal(T::Background)),
    ("terminal.foreground", Target::Terminal(T::Foreground)),
    ("terminal.selectionBackground", Target::Terminal(T::Selection)),
    ("terminalCursor.foreground", Target::Terminal(T::Cursor)),
    ("terminal.ansiBlack", Target::Terminal(T::Ansi0)),
    ("terminal.ansiRed", Target::Terminal(T::Ansi1)),
    ("terminal.ansiGreen", Target::Terminal(T::Ansi2)),
    ("terminal.ansiYellow", Target::Terminal(T::Ansi3)),
    ("terminal.ansiBlue", Target::Terminal(T::Ansi4)),
    ("terminal.ansiMagenta", Target::Terminal(T::Ansi5)),
    ("terminal.ansiCyan", Target::Terminal(T::Ansi6)),
    ("terminal.ansiWhite", Target::Terminal(T::Ansi7)),
    ("terminal.ansiBrightBlack", Target::Terminal(T::Ansi8)),
    ("terminal.ansiBrightRed", Target::Terminal(T::Ansi9)),
    ("terminal.ansiBrightGreen", Target::Terminal(T::Ansi10)),
    ("terminal.ansiBrightYellow", Target::Terminal(T::Ansi11)),
    ("terminal.ansiBrightBlue", Target::Terminal(T::Ansi12)),
    ("terminal.ansiBrightMagenta", Target::Terminal(T::Ansi13)),
    ("terminal.ansiBrightCyan", Target::Terminal(T::Ansi14)),
    ("terminal.ansiBrightWhite", Target::Terminal(T::Ansi15)),
];

pub const DERIVED: &[(&str, E, u8)] = &[
    ("editorGutter.modifiedBackground", E::DiffModified, 0x40),
    ("editorGutter.modifiedBackground", E::DiffModifiedWord, 0x80),
    ("editorGutter.addedBackground", E::DiffInserted, 0x40),
    ("editorGutter.addedBackground", E::DiffInsertedWord, 0x80),
    ("editorGutter.deletedBackground", E::DiffDeleted, 0x40),
    ("editorGutter.deletedBackground", E::DiffDeletedWord, 0x80),
];

pub const GLOBAL_SETTINGS: &[(&str, &str)] = &[
    ("background", "editor.background"),
    ("foreground", "editor.foreground"),
    ("caret", "editorCursor.foreground"),
    ("selection", "editor.selectionBackground"),
    ("lineHighlight", "editor.lineHighlightBackground"),
];

pub const SCOPES: &[(S, &[&[&str]])] = &[
    (S::Keyword, &[&["keyword"], &["keyword.control"], &["storage.type"], &["storage.modifier"]]),
    (S::String, &[&["string"]]),
    (S::Number, &[&["constant.numeric"]]),
    (S::Comment, &[&["comment"]]),
    (S::DocComment, &[&["comment.block.documentation"]]),
    (S::Macro, &[&["entity.name.function.macro"]]),
    (S::Type, &[&["entity.name.type"], &["entity.name.class"], &["support.type"]]),
    (S::Function, &[&["entity.name.function"], &["support.function"]]),
    (S::Lifetime, &[&["storage.modifier.lifetime"]]),
    (S::Constant, &[&["variable.other.constant"], &["constant.language"]]),
    (S::Attribute, &[&["meta.attribute"], &["entity.other.attribute-name"]]),
    (S::Punctuation, &[&["punctuation"], &["keyword.operator"]]),
    (S::Heading, &[&["markup.heading"]]),
    (S::Strong, &[&["markup.bold"]]),
    (S::Emphasis, &[&["markup.italic"]]),
    (S::Link, &[&["markup.underline.link"]]),
    (S::Code, &[&["markup.inline.raw"]]),
    (S::SemanticPackage, &[&["entity.name.namespace"]]),
    (S::SemanticType, &[&["entity.name.type"], &["entity.name.class"]]),
    (S::SemanticBuiltinType, &[&["support.type.primitive"], &["support.type"]]),
    (S::SemanticCall, &[&["meta.function-call", "entity.name.function"], &["meta.function-call", "support.function"]]),
    (S::Property, &[&["variable.other.property"], &["variable.other.object.property"], &["support.variable.property"]]),
    (S::GlobalVariable, &[&["support.variable"], &["variable.other.global"]]),
];

pub const SEMANTIC: &[(S, &[&str])] = &[
    (S::SemanticPackage, &["namespace"]),
    (S::SemanticType, &["type", "class", "struct", "interface", "enum", "typeParameter"]),
    (S::SemanticBuiltinType, &["type.defaultLibrary", "class.defaultLibrary", "builtinType"]),
    (S::SemanticCall, &["function", "method"]),
    (S::Property, &["property"]),
    (S::GlobalVariable, &["variable.defaultLibrary"]),
    (S::Function, &["function.declaration", "method.declaration", "function", "method"]),
    (S::Macro, &["macro"]),
    (S::Keyword, &["keyword"]),
    (S::String, &["string"]),
    (S::Number, &["number"]),
    (S::Comment, &["comment"]),
    (S::Constant, &["enumMember", "variable.readonly"]),
    (S::Attribute, &["decorator"]),
    (S::Punctuation, &["operator"]),
    (S::Lifetime, &["lifetime"]),
];

pub const DEFAULT_NAME: &str = "VS Code theme";

type Resolve<'a> = dyn Fn(&str) -> Option<String> + 'a;
type SharedResolve = dyn Fn(&str) -> Option<String> + Send + Sync;

pub struct VsCodeImporter {
    path: String,
    resolve: Box<SharedResolve>,
}

impl VsCodeImporter {
    pub fn new() -> VsCodeImporter {
        VsCodeImporter { path: String::new(), resolve: Box::new(|_| None) }
    }

    pub fn with_includes(
        path: impl Into<String>,
        resolve: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
    ) -> VsCodeImporter {
        VsCodeImporter { path: path.into(), resolve: Box::new(resolve) }
    }
}

impl Default for VsCodeImporter {
    fn default() -> VsCodeImporter {
        VsCodeImporter::new()
    }
}

impl ThemeImporter for VsCodeImporter {
    fn format_name(&self) -> &'static str {
        "VS Code colour theme"
    }

    fn detects(&self, text: &str) -> bool {
        let Ok(Value::Object(root)) = serde_json::from_str::<Value>(&jsonc::strip(text)) else { return false };
        let schema = root.get("$schema").and_then(Value::as_str).is_some_and(|schema| schema.contains("color-theme"));
        let fields = ["colors", "tokenColors", "semanticTokenColors", "include"];
        let foreign = ["formatVersion", "ui", "editorScheme", "dark"];
        !foreign.iter().any(|field| root.contains_key(*field))
            && (schema || fields.iter().any(|field| root.contains_key(*field)))
    }

    fn import(&self, text: &str) -> Result<ImportReport, ImportError> {
        import(text, &self.path, &*self.resolve)
    }

    fn import_at(
        &self,
        path: &str,
        text: &str,
        read: &dyn Fn(&str) -> Option<String>,
    ) -> Result<ImportReport, ImportError> {
        import(text, path, read)
    }
}

pub fn import(text: &str, path: &str, resolve: &dyn Fn(&str) -> Option<String>) -> Result<ImportReport, ImportError> {
    let mut chain = vec![normalize(path)];
    let source = load(text, &chain[0].clone(), resolve, &mut chain)?;
    Ok(build(source))
}

#[derive(Default)]
struct Source {
    name: Option<String>,
    kind: Option<String>,
    colors: BTreeMap<String, Value>,
    token_colors: Vec<Value>,
    semantic_highlighting: Option<bool>,
    semantic: BTreeMap<String, Value>,
    unmapped: Vec<String>,
}

fn load(text: &str, path: &str, resolve: &Resolve<'_>, chain: &mut Vec<String>) -> Result<Source, ImportError> {
    let place = if path.is_empty() { String::new() } else { format!("{path}: ") };
    let root = match serde_json::from_str::<Value>(&jsonc::strip(text)) {
        Ok(Value::Object(root)) => root,
        Ok(_) => return Err(ImportError(format!("{place}a VS Code colour theme is a JSON object"))),
        Err(error) => return Err(ImportError(format!("{place}{}", FormatError::from(error)))),
    };
    let mut source = Source::default();
    if let Some(include) = root.get("include") {
        match include.as_str() {
            Some(include) => {
                let target = join(path, include);
                if chain.contains(&target) {
                    source.unmapped.push(format!("include {include} (a cycle)"));
                } else if chain.len() > MAX_DEPTH {
                    source.unmapped.push(format!("include {include} (too deep)"));
                } else if let Some(text) = resolve(&target) {
                    chain.push(target.clone());
                    source = load(&text, &target, resolve, chain)?;
                    chain.pop();
                } else {
                    source.unmapped.push(format!("include {include} (not found)"));
                }
            }
            None => source.unmapped.push("include".to_owned()),
        }
    }
    let mut fields: Vec<(String, Value)> = root.into_iter().collect();
    fields.sort_by(|left, right| left.0.cmp(&right.0));
    for (field, value) in fields {
        match (field.as_str(), value) {
            ("$schema" | "include", _) => {}
            ("name", Value::String(name)) => source.name = Some(name),
            ("type", Value::String(kind)) => source.kind = Some(kind),
            ("colors", Value::Object(colors)) => source.colors.extend(colors),
            ("tokenColors", Value::Array(rules)) => source.token_colors.extend(rules),
            ("semanticHighlighting", Value::Bool(on)) => source.semantic_highlighting = Some(on),
            ("semanticTokenColors", Value::Object(rules)) => source.semantic.extend(rules),
            (_, _) => source.unmapped.push(field),
        }
    }
    Ok(source)
}

fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    let joined = parts.join("/");
    if path.starts_with(['/', '\\']) { format!("/{joined}") } else { joined }
}

fn join(path: &str, include: &str) -> String {
    if include.starts_with(['/', '\\']) {
        return normalize(include);
    }
    match path.rfind('/') {
        Some(slash) => normalize(&format!("{}/{include}", &path[..slash])),
        None => normalize(include),
    }
}

fn parse_color(value: &Value) -> Option<Rgba> {
    let text = value.as_str()?;
    let digits = text.strip_prefix('#')?;
    if digits.len() == 4 && digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        let nibble = |index: usize| u8::from_str_radix(&digits[index..=index], 16).ok().map(|value| value * 0x11);
        return Some(Rgba::new(nibble(0)?, nibble(1)?, nibble(2)?, nibble(3)?));
    }
    text.parse().ok()
}

fn over(color: Rgba, background: Rgba) -> Rgba {
    color.over(background)
}

fn mul_alpha(one: u8, two: u8) -> u8 {
    u8::try_from((u32::from(one) * u32::from(two) + 127) / 255).unwrap_or(u8::MAX)
}

fn is_light(color: Rgba) -> bool {
    u32::from(color.r()) * 299 + u32::from(color.g()) * 587 + u32::from(color.b()) * 114 > 128_000
}

fn set(theme: &mut Theme, target: Target, color: Rgba) {
    match target {
        Target::Ui(key) => theme.ui.entry(key).or_insert(color),
        Target::Editor(key) => theme.editor.entry(key).or_insert(color),
        Target::Terminal(key) => theme.terminal.entry(key).or_insert(color),
    };
}

struct Tokens {
    rules: Vec<Rule>,
    global: BTreeMap<String, Rgba>,
}

fn read_tokens(values: &[Value], mapped: &mut usize, unmapped: &mut Vec<String>) -> Tokens {
    let mut tokens = Tokens { rules: Vec::new(), global: BTreeMap::new() };
    for value in values {
        let Some(settings) = value.get("settings").and_then(Value::as_object) else {
            unmapped.push(format!("tokenColors: {}", describe(value)));
            continue;
        };
        let selectors = scopes(value.get("scope"));
        if selectors.is_empty() {
            for (name, color) in settings {
                match (GLOBAL_SETTINGS.iter().any(|(setting, _)| setting == name), parse_color(color)) {
                    (true, Some(color)) => {
                        *mapped += 1;
                        tokens.global.insert(name.clone(), color);
                    }
                    _ => unmapped.push(format!("tokenColors.settings.{name}")),
                }
            }
            continue;
        }
        let foreground = settings.get("foreground").and_then(parse_color);
        let background = settings.get("background").and_then(parse_color);
        let font_style = settings.get("fontStyle").and_then(Value::as_str).map(FontStyle::parse);
        for selector in selectors {
            match Selector::parse(&selector).filter(probed) {
                Some(parsed) => {
                    *mapped += 1;
                    tokens.rules.push(Rule { selector: parsed, foreground, background, font_style });
                }
                None => unmapped.push(format!("tokenColors: {selector}")),
            }
        }
    }
    tokens
}

fn describe(value: &Value) -> String {
    value.get("name").and_then(Value::as_str).map_or_else(|| value.to_string(), str::to_owned)
}

fn scopes(scope: Option<&Value>) -> Vec<String> {
    let texts: Vec<&str> = match scope {
        Some(Value::String(text)) => vec![text],
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    texts
        .iter()
        .flat_map(|text| text.split(','))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .collect()
}

fn probed(selector: &Selector) -> bool {
    SCOPES.iter().flat_map(|(_, stacks)| stacks.iter()).any(|stack| selector.score(stack).is_some())
}

fn style(color: Rgba, background: Option<Rgba>, font: FontStyle) -> Style {
    Style {
        color,
        background,
        bold: font.bold,
        italic: font.italic,
        underline: font.underline.then_some(Underline::Line),
        strikethrough: font.strikethrough,
    }
}

fn semantic_style(value: &Value, fallback: Rgba, editor: Rgba) -> Option<Style> {
    if value.is_string() {
        return parse_color(value).map(|color| Style::color(over(color, editor)));
    }
    let object: &Map<String, Value> = value.as_object()?;
    let color = object.get("foreground").and_then(parse_color).map_or(fallback, |color| over(color, editor));
    let mut font = object.get("fontStyle").and_then(Value::as_str).map(FontStyle::parse).unwrap_or_default();
    let flag = |name: &str| object.get(name).and_then(Value::as_bool);
    font.bold = flag("bold").unwrap_or(font.bold);
    font.italic = flag("italic").unwrap_or(font.italic);
    font.underline = flag("underline").unwrap_or(font.underline);
    font.strikethrough = flag("strikethrough").unwrap_or(font.strikethrough);
    Some(style(color, None, font))
}

fn build(source: Source) -> ImportReport {
    let mut unmapped = source.unmapped;
    let mut mapped = 0;

    let mut colors: BTreeMap<&str, Rgba> = BTreeMap::new();
    for (name, value) in &source.colors {
        let known = COLORS.iter().find(|(key, _)| key == name).map(|(key, _)| *key);
        match (known, parse_color(value)) {
            (Some(key), Some(color)) => {
                mapped += 1;
                colors.insert(key, color);
            }
            _ if value.is_null() => {}
            _ => unmapped.push(name.clone()),
        }
    }
    let tokens = read_tokens(&source.token_colors, &mut mapped, &mut unmapped);
    for (setting, key) in GLOBAL_SETTINGS {
        if let Some(color) = tokens.global.get(*setting) {
            colors.entry(*key).or_insert(*color);
        }
    }

    let dark = match source.kind.as_deref() {
        Some("light" | "hcLight" | "hc-light" | "vs") => false,
        Some("dark" | "hc" | "hcDark" | "hc-black" | "vs-dark") => true,
        _ => !colors.get("editor.background").is_some_and(|color| is_light(over(*color, Rgba::rgb(0, 0, 0)))),
    };
    let zen = ResolvedTheme::base(dark);
    let base = if dark { zen.editor(E::Background) } else { Rgba::rgb(0xFF, 0xFF, 0xFF) };
    let editor = colors.get("editor.background").map_or(zen.editor(E::Background), |color| over(*color, base));
    let terminal = colors.get("terminal.background").map_or(editor, |color| over(*color, editor));

    let name = source.name.unwrap_or_else(|| DEFAULT_NAME.to_owned());
    let mut theme = Theme::new(id_from_name(&name), name, dark);
    theme.parent = Some(base_id(dark).to_owned());
    for (key, target) in COLORS {
        let Some(color) = colors.get(key).copied() else { continue };
        let color = match target {
            _ if target.keeps_alpha() => color,
            Target::Editor(E::Background) => editor,
            Target::Terminal(T::Background) => terminal,
            Target::Terminal(_) => over(color, terminal),
            _ => over(color, editor),
        };
        set(&mut theme, *target, color);
    }
    for (key, target, alpha) in DERIVED {
        let Some(color) = colors.get(key).copied() else { continue };
        let tint = Rgba::new(color.r(), color.g(), color.b(), mul_alpha(color.a(), *alpha));
        set(&mut theme, Target::Editor(*target), over(tint, editor));
    }

    let foreground = theme.editor.get(&E::Foreground).copied();
    if let Some(color) = foreground {
        theme.syntax.insert(S::Default, Style::color(color));
    }
    for (key, stacks) in SCOPES {
        let Some(found) = stacks.iter().find_map(|stack| textmate::resolve(&tokens.rules, stack)) else { continue };
        let color = found.foreground.map(|color| over(color, editor)).or(foreground).unwrap_or(zen.syntax(*key).color);
        theme.syntax.insert(*key, style(color, found.background, found.font_style.unwrap_or_default()));
    }

    if source.semantic_highlighting == Some(false) {
        unmapped.extend(source.semantic.keys().map(|selector| format!("semanticTokenColors: {selector}")));
    } else {
        for (key, selectors) in SEMANTIC {
            let fallback =
                theme.syntax.get(key).map_or(foreground.unwrap_or(zen.syntax(*key).color), |style| style.color);
            let found = selectors.iter().find_map(|selector| source.semantic.get(*selector));
            if let Some(style) = found.and_then(|value| semantic_style(value, fallback, editor)) {
                theme.syntax.insert(*key, style);
            }
        }
        for (selector, value) in &source.semantic {
            let known = SEMANTIC.iter().any(|(_, selectors)| selectors.contains(&selector.as_str()));
            if known && (value.is_string() || value.is_object()) && semantic_style(value, editor, editor).is_some() {
                mapped += 1;
            } else {
                unmapped.push(format!("semanticTokenColors: {selector}"));
            }
        }
    }

    let mut seen = Vec::new();
    unmapped.retain(|entry| {
        let new = !seen.contains(entry);
        seen.push(entry.clone());
        new
    });
    ImportReport { theme, mapped, unmapped }
}
