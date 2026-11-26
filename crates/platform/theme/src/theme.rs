use std::collections::BTreeMap;
use std::fmt;

use crate::zen_dark::{ZEN_DARK_EDITOR, ZEN_DARK_ID, ZEN_DARK_SYNTAX, ZEN_DARK_TERMINAL, ZEN_DARK_UI, builtin};
use crate::zen_light::{ZEN_LIGHT_EDITOR, ZEN_LIGHT_ID, ZEN_LIGHT_SYNTAX, ZEN_LIGHT_TERMINAL, ZEN_LIGHT_UI};
use crate::{EditorKey, Key, Rgba, Style, SyntaxKey, TerminalKey, UiKey};

pub const MAX_DEPTH: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub dark: bool,
    pub parent: Option<String>,
    pub ui: BTreeMap<UiKey, Rgba>,
    pub editor: BTreeMap<EditorKey, Rgba>,
    pub syntax: BTreeMap<SyntaxKey, Style>,
    pub terminal: BTreeMap<TerminalKey, Rgba>,
    pub unknown_keys: Vec<String>,
}

impl Theme {
    pub fn new(id: impl Into<String>, name: impl Into<String>, dark: bool) -> Theme {
        Theme {
            id: id.into(),
            name: name.into(),
            dark,
            parent: None,
            ui: BTreeMap::new(),
            editor: BTreeMap::new(),
            syntax: BTreeMap::new(),
            terminal: BTreeMap::new(),
            unknown_keys: Vec::new(),
        }
    }

    pub fn resolve(&self, find: impl Fn(&str) -> Option<Theme>) -> Resolution {
        let mut warnings = Vec::new();
        let mut chain = vec![self.clone()];
        let mut next = self.parent.clone();
        while let Some(id) = next.take() {
            if chain.iter().any(|theme| theme.id == id) {
                let mut ids: Vec<String> = chain.iter().map(|theme| theme.id.clone()).collect();
                ids.push(id);
                warnings.push(Warning::Cycle(ids));
                break;
            }
            if chain.len() > MAX_DEPTH {
                warnings.push(Warning::TooDeep { theme: self.id.clone() });
                break;
            }
            match find(&id).or_else(|| builtin(&id)) {
                Some(parent) => {
                    next = parent.parent.clone();
                    chain.push(parent);
                }
                None => {
                    let child = chain.last().map(|theme| theme.id.clone()).unwrap_or_default();
                    warnings.push(Warning::MissingParent { theme: child, parent: id });
                }
            }
        }

        let farthest_dark = chain.last().is_none_or(|theme| theme.dark);
        let mut resolved = ResolvedTheme::base(farthest_dark);
        resolved.id.clone_from(&self.id);
        resolved.name.clone_from(&self.name);
        resolved.dark = self.dark;
        for theme in chain.iter().rev() {
            for (key, color) in &theme.ui {
                resolved.ui[key.index()] = *color;
            }
            for (key, color) in &theme.editor {
                resolved.editor[key.index()] = *color;
            }
            for (key, style) in &theme.syntax {
                resolved.syntax[key.index()] = *style;
            }
            for (key, color) in &theme.terminal {
                resolved.terminal[key.index()] = *color;
            }
        }
        for theme in &chain {
            warnings.extend(
                theme.unknown_keys.iter().map(|key| Warning::UnknownKey { theme: theme.id.clone(), key: key.clone() }),
            );
        }
        Resolution { theme: resolved, warnings }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Warning {
    UnknownKey { theme: String, key: String },
    MissingParent { theme: String, parent: String },
    Cycle(Vec<String>),
    TooDeep { theme: String },
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Warning::UnknownKey { theme, key } => write!(f, "theme `{theme}`: unknown key `{key}` is ignored"),
            Warning::MissingParent { theme, parent } => {
                write!(f, "theme `{theme}`: parent `{parent}` is not found; its colours come from the built-in theme")
            }
            Warning::Cycle(ids) => write!(f, "themes inherit from each other: {}", ids.join(" -> ")),
            Warning::TooDeep { theme } => {
                write!(f, "theme `{theme}` has more than {MAX_DEPTH} parents; the farther ones are ignored")
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub theme: ResolvedTheme,
    pub warnings: Vec<Warning>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedTheme {
    pub id: String,
    pub name: String,
    pub dark: bool,
    ui: [Rgba; UiKey::COUNT],
    editor: [Rgba; EditorKey::COUNT],
    syntax: [Style; SyntaxKey::COUNT],
    terminal: [Rgba; TerminalKey::COUNT],
}

impl ResolvedTheme {
    pub fn zen_dark() -> ResolvedTheme {
        ResolvedTheme {
            id: ZEN_DARK_ID.to_owned(),
            name: "Zen Dark".to_owned(),
            dark: true,
            ui: ZEN_DARK_UI.map(|(_, color)| color),
            editor: ZEN_DARK_EDITOR.map(|(_, color)| color),
            syntax: ZEN_DARK_SYNTAX.map(|(_, style)| style),
            terminal: ZEN_DARK_TERMINAL.map(|(_, color)| color),
        }
    }

    pub fn zen_light() -> ResolvedTheme {
        ResolvedTheme {
            id: ZEN_LIGHT_ID.to_owned(),
            name: "Zen Light".to_owned(),
            dark: false,
            ui: ZEN_LIGHT_UI.map(|(_, color)| color),
            editor: ZEN_LIGHT_EDITOR.map(|(_, color)| color),
            syntax: ZEN_LIGHT_SYNTAX.map(|(_, style)| style),
            terminal: ZEN_LIGHT_TERMINAL.map(|(_, color)| color),
        }
    }

    pub fn base(dark: bool) -> ResolvedTheme {
        if dark { ResolvedTheme::zen_dark() } else { ResolvedTheme::zen_light() }
    }

    pub fn ui(&self, key: UiKey) -> Rgba {
        self.ui[key.index()]
    }

    pub fn editor(&self, key: EditorKey) -> Rgba {
        self.editor[key.index()]
    }

    pub fn syntax(&self, key: SyntaxKey) -> Style {
        self.syntax[key.index()]
    }

    pub fn terminal(&self, key: TerminalKey) -> Rgba {
        self.terminal[key.index()]
    }

    pub fn ansi(&self) -> [Rgba; 16] {
        TerminalKey::ANSI.map(|key| self.terminal(key))
    }

    pub fn project(&self, hash: usize) -> Rgba {
        self.ui(UiKey::PROJECTS[hash % UiKey::PROJECTS.len()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zen_dark;

    const RED: Rgba = Rgba::rgb(0xFF, 0, 0);
    const GREEN: Rgba = Rgba::rgb(0, 0xFF, 0);
    const BLUE: Rgba = Rgba::rgb(0, 0, 0xFF);

    fn child(id: &str, parent: Option<&str>) -> Theme {
        Theme { parent: parent.map(str::to_owned), ..Theme::new(id, id, true) }
    }

    fn library(themes: Vec<Theme>) -> impl Fn(&str) -> Option<Theme> {
        move |id| themes.iter().find(|theme| theme.id == id).cloned()
    }

    #[test]
    fn an_empty_theme_is_zen_dark() {
        let resolution = Theme::new("blank", "Blank", true).resolve(|_| None);
        assert!(resolution.warnings.is_empty());
        let zen = ResolvedTheme::zen_dark();
        assert_eq!(resolution.theme, ResolvedTheme { id: "blank".into(), name: "Blank".into(), ..zen });
    }

    #[test]
    fn zen_dark_resolves_to_itself() {
        let resolution = zen_dark().resolve(|_| None);
        assert_eq!(resolution.theme, ResolvedTheme::zen_dark());
        assert!(resolution.warnings.is_empty());
    }

    #[test]
    fn an_empty_light_theme_is_zen_light() {
        let resolution = Theme::new("blank", "Blank", false).resolve(|_| None);
        assert!(resolution.warnings.is_empty());
        let light = ResolvedTheme::zen_light();
        assert_eq!(resolution.theme, ResolvedTheme { id: "blank".into(), name: "Blank".into(), ..light });
        assert_eq!(crate::zen_light().resolve(|_| None).theme, ResolvedTheme::zen_light());
    }

    #[test]
    fn the_farthest_theme_of_the_chain_chooses_the_base() {
        let parent = Theme::new("day", "Day", false);
        let mut theme = child("dusk", Some("day"));
        theme.ui.insert(UiKey::Accent, RED);
        let resolved = theme.resolve(library(vec![parent])).theme;
        assert_eq!(resolved.ui(UiKey::IslandBackground), ResolvedTheme::zen_light().ui(UiKey::IslandBackground));
        assert_eq!(resolved.ui(UiKey::Accent), RED);
        assert!(resolved.dark, "the theme says what it is; only the base follows the chain");

        let mut orphan = Theme::new("orphan", "Orphan", false);
        orphan.parent = Some("gone".into());
        let resolution = orphan.resolve(|_| None);
        assert_eq!(resolution.theme.ui(UiKey::TextPrimary), ResolvedTheme::zen_light().ui(UiKey::TextPrimary));
        assert!(resolution.warnings[0].to_string().ends_with("its colours come from the built-in theme"));
    }

    #[test]
    fn the_nearest_theme_wins_along_the_chain() {
        let mut grandparent = child("grandparent", None);
        grandparent.ui.insert(UiKey::Accent, RED);
        grandparent.ui.insert(UiKey::Link, RED);
        grandparent.terminal.insert(TerminalKey::Ansi1, RED);
        let mut parent = child("parent", Some("grandparent"));
        parent.ui.insert(UiKey::Link, GREEN);
        parent.syntax.insert(SyntaxKey::Keyword, Style::color(GREEN).italic());
        let mut theme = child("theme", Some("parent"));
        theme.ui.insert(UiKey::Accent, BLUE);
        theme.dark = false;

        let resolution = theme.resolve(library(vec![parent, grandparent]));
        assert!(resolution.warnings.is_empty(), "{:?}", resolution.warnings);
        let resolved = resolution.theme;
        assert_eq!(resolved.ui(UiKey::Accent), BLUE);
        assert_eq!(resolved.ui(UiKey::Link), GREEN);
        assert_eq!(resolved.syntax(SyntaxKey::Keyword), Style::color(GREEN).italic());
        assert_eq!(resolved.terminal(TerminalKey::Ansi1), RED);
        assert_eq!(resolved.ansi()[1], RED);
        assert_eq!(resolved.ui(UiKey::WindowBackground), Rgba::rgb(0x25, 0x28, 0x2C));
        assert!(!resolved.dark);
        assert_eq!(resolved.id, "theme");
    }

    #[test]
    fn built_in_parents_are_found() {
        let mut theme = child("mine", Some(ZEN_DARK_ID));
        theme.editor.insert(EditorKey::Caret, RED);
        let resolution = theme.resolve(|_| None);
        assert!(resolution.warnings.is_empty());
        assert_eq!(resolution.theme.editor(EditorKey::Caret), RED);
    }

    #[test]
    fn a_cycle_is_reported_and_broken() {
        let mut a = child("a", Some("b"));
        a.ui.insert(UiKey::Accent, RED);
        let mut b = child("b", Some("a"));
        b.ui.insert(UiKey::Link, GREEN);
        let resolution = a.resolve(library(vec![a.clone(), b]));
        assert_eq!(resolution.warnings, [Warning::Cycle(vec!["a".into(), "b".into(), "a".into()])]);
        assert_eq!(resolution.theme.ui(UiKey::Accent), RED);
        assert_eq!(resolution.theme.ui(UiKey::Link), GREEN);
    }

    #[test]
    fn a_theme_that_is_its_own_parent_is_a_cycle() {
        let theme = child("self", Some("self"));
        let resolution = theme.resolve(library(vec![theme.clone()]));
        assert_eq!(resolution.warnings, [Warning::Cycle(vec!["self".into(), "self".into()])]);
    }

    #[test]
    fn at_most_eight_parents_are_followed() {
        let chain = |length: usize| -> Vec<Theme> {
            let mut themes: Vec<Theme> = (0..length)
                .map(|index| {
                    let parent = (index + 1 < length).then(|| format!("t{}", index + 1));
                    child(&format!("t{index}"), parent.as_deref())
                })
                .collect();
            if let Some(farthest) = themes.last_mut() {
                farthest.ui.insert(UiKey::Accent, RED);
            }
            themes
        };

        let themes = chain(MAX_DEPTH + 1);
        let resolution = themes[0].resolve(library(themes.clone()));
        assert!(resolution.warnings.is_empty(), "eight parents are allowed: {:?}", resolution.warnings);
        assert_eq!(resolution.theme.ui(UiKey::Accent), RED);

        let themes = chain(MAX_DEPTH + 2);
        let resolution = themes[0].resolve(library(themes.clone()));
        assert_eq!(resolution.warnings, [Warning::TooDeep { theme: "t0".into() }]);
        assert_eq!(resolution.theme.ui(UiKey::Accent), ResolvedTheme::zen_dark().ui(UiKey::Accent));
    }

    #[test]
    fn a_missing_parent_falls_back_to_zen_dark() {
        let mut theme = child("orphan", Some("gone"));
        theme.ui.insert(UiKey::Accent, RED);
        let resolution = theme.resolve(|_| None);
        assert_eq!(resolution.warnings, [Warning::MissingParent { theme: "orphan".into(), parent: "gone".into() }]);
        assert_eq!(resolution.theme.ui(UiKey::Accent), RED);
        assert_eq!(resolution.theme.ui(UiKey::Link), ResolvedTheme::zen_dark().ui(UiKey::Link));
    }

    #[test]
    fn unknown_keys_of_every_theme_in_the_chain_are_warnings() {
        let mut parent = child("parent", None);
        parent.unknown_keys.push("editor.gutter.glow".into());
        let mut theme = child("theme", Some("parent"));
        theme.unknown_keys.push("ui.no.such.key".into());
        let resolution = theme.resolve(library(vec![parent]));
        assert_eq!(
            resolution.warnings,
            [
                Warning::UnknownKey { theme: "theme".into(), key: "ui.no.such.key".into() },
                Warning::UnknownKey { theme: "parent".into(), key: "editor.gutter.glow".into() },
            ]
        );
        assert_eq!(resolution.warnings[0].to_string(), "theme `theme`: unknown key `ui.no.such.key` is ignored");
    }

    #[test]
    fn project_colours_wrap_around() {
        let zen = ResolvedTheme::zen_dark();
        assert_eq!(zen.project(0), zen.ui(UiKey::Project0));
        assert_eq!(zen.project(7), zen.ui(UiKey::Project1));
    }
}
