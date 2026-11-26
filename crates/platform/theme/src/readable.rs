use std::fmt;

use crate::{EditorKey as E, Key, ResolvedTheme, Rgba, SyntaxKey as S, TerminalKey as T, Theme, UiKey as U};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    Ui(U),
    Editor(E),
    Syntax(S),
    Terminal(T),
}

impl Slot {
    pub fn color(self, theme: &ResolvedTheme) -> Rgba {
        let (color, under) = match self {
            Slot::Ui(key) => (theme.ui(key), theme.ui(U::IslandBackground)),
            Slot::Editor(key) => (theme.editor(key), theme.editor(E::Background)),
            Slot::Syntax(key) => (theme.syntax(key).color, theme.editor(E::Background)),
            Slot::Terminal(key) => (theme.terminal(key), theme.terminal(T::Background)),
        };
        let ground = if under.a() == 0xFF { under } else { under.over(Rgba::rgb(0xFF, 0xFF, 0xFF)) };
        if color.a() == 0xFF { color } else { color.over(ground) }
    }

    fn set(self, theme: &mut Theme, resolved: &ResolvedTheme, color: Rgba) {
        match self {
            Slot::Ui(key) => {
                theme.ui.insert(key, color);
            }
            Slot::Editor(key) => {
                theme.editor.insert(key, color);
            }
            Slot::Syntax(key) => {
                let mut style = theme.syntax.get(&key).copied().unwrap_or_else(|| resolved.syntax(key));
                style.color = color;
                theme.syntax.insert(key, style);
            }
            Slot::Terminal(key) => {
                theme.terminal.insert(key, color);
            }
        }
    }
}

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Slot::Ui(key) => write!(f, "{}.{}", U::SECTION, key.id()),
            Slot::Editor(key) => write!(f, "{}.{}", E::SECTION, key.id()),
            Slot::Syntax(key) => write!(f, "{}.{}", S::SECTION, key.id()),
            Slot::Terminal(key) => write!(f, "{}.{}", T::SECTION, key.id()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pair {
    pub text: Slot,
    pub background: Slot,
    pub minimum: f32,
}

const fn pair(text: Slot, background: Slot, minimum: f32) -> Pair {
    Pair { text, background, minimum }
}

const BODY: f32 = 4.5;
const LABEL: f32 = 3.0;
const FAINT: f32 = 2.0;

pub const PAIRS: &[Pair] = &[
    pair(Slot::Ui(U::TextPrimary), Slot::Ui(U::WindowBackground), BODY),
    pair(Slot::Ui(U::TextPrimary), Slot::Ui(U::IslandBackground), BODY),
    pair(Slot::Ui(U::TextPrimary), Slot::Ui(U::PopupBackground), BODY),
    pair(Slot::Ui(U::TextPrimary), Slot::Ui(U::TextFieldBackground), BODY),
    pair(Slot::Ui(U::TextPrimary), Slot::Ui(U::ListHover), BODY),
    pair(Slot::Ui(U::TextPrimary), Slot::Ui(U::ListSelection), BODY),
    pair(Slot::Ui(U::TextPrimary), Slot::Ui(U::PopupHover), BODY),
    pair(Slot::Ui(U::TextPrimary), Slot::Ui(U::TabActiveBackground), BODY),
    pair(Slot::Ui(U::TextPrimary), Slot::Ui(U::BannerBackground), BODY),
    pair(Slot::Ui(U::TextPrimary), Slot::Ui(U::SearchHighlight), BODY),
    pair(Slot::Ui(U::TextSecondary), Slot::Ui(U::WindowBackground), LABEL),
    pair(Slot::Ui(U::TextSecondary), Slot::Ui(U::IslandBackground), LABEL),
    pair(Slot::Ui(U::TextSecondary), Slot::Ui(U::PopupBackground), LABEL),
    pair(Slot::Ui(U::TextError), Slot::Ui(U::IslandBackground), LABEL),
    pair(Slot::Ui(U::TextError), Slot::Ui(U::PopupBackground), LABEL),
    pair(Slot::Ui(U::TextWarning), Slot::Ui(U::PopupBackground), LABEL),
    pair(Slot::Ui(U::Link), Slot::Ui(U::IslandBackground), LABEL),
    pair(Slot::Ui(U::Link), Slot::Ui(U::PopupBackground), LABEL),
    pair(Slot::Ui(U::AccentForeground), Slot::Ui(U::Accent), LABEL),
    pair(Slot::Ui(U::StripeActiveIcon), Slot::Ui(U::Accent), LABEL),
    pair(Slot::Ui(U::TabModified), Slot::Ui(U::IslandBackground), LABEL),
    pair(Slot::Ui(U::TreeExcludedText), Slot::Ui(U::TreeExcludedBackground), LABEL),
    pair(Slot::Ui(U::VcsModified), Slot::Ui(U::IslandBackground), LABEL),
    pair(Slot::Ui(U::VcsAdded), Slot::Ui(U::IslandBackground), LABEL),
    pair(Slot::Ui(U::VcsDeleted), Slot::Ui(U::IslandBackground), LABEL),
    pair(Slot::Ui(U::VcsUnversioned), Slot::Ui(U::IslandBackground), LABEL),
    pair(Slot::Ui(U::VcsIgnored), Slot::Ui(U::IslandBackground), LABEL),
    pair(Slot::Ui(U::VcsConflict), Slot::Ui(U::IslandBackground), LABEL),
    pair(Slot::Ui(U::VcsIncoming), Slot::Ui(U::PopupBackground), LABEL),
    pair(Slot::Ui(U::VcsOutgoing), Slot::Ui(U::PopupBackground), LABEL),
    pair(Slot::Ui(U::ProjectForeground), Slot::Ui(U::Project0), LABEL),
    pair(Slot::Ui(U::ProjectForeground), Slot::Ui(U::Project1), LABEL),
    pair(Slot::Ui(U::ProjectForeground), Slot::Ui(U::Project2), LABEL),
    pair(Slot::Ui(U::ProjectForeground), Slot::Ui(U::Project3), LABEL),
    pair(Slot::Ui(U::ProjectForeground), Slot::Ui(U::Project4), LABEL),
    pair(Slot::Ui(U::ProjectForeground), Slot::Ui(U::Project5), LABEL),
    pair(Slot::Editor(E::Foreground), Slot::Editor(E::Background), BODY),
    pair(Slot::Editor(E::Foreground), Slot::Editor(E::CurrentLine), BODY),
    pair(Slot::Editor(E::Foreground), Slot::Editor(E::Selection), LABEL),
    pair(Slot::Editor(E::Foreground), Slot::Editor(E::SearchMatch), BODY),
    pair(Slot::Editor(E::Foreground), Slot::Editor(E::DiffInserted), BODY),
    pair(Slot::Editor(E::Foreground), Slot::Editor(E::DiffDeleted), BODY),
    pair(Slot::Editor(E::Foreground), Slot::Editor(E::DiffModified), BODY),
    pair(Slot::Editor(E::Foreground), Slot::Editor(E::DiffInsertedWord), LABEL),
    pair(Slot::Editor(E::Foreground), Slot::Editor(E::DiffDeletedWord), LABEL),
    pair(Slot::Editor(E::Foreground), Slot::Editor(E::DiffModifiedWord), LABEL),
    pair(Slot::Editor(E::LineNumber), Slot::Editor(E::Background), FAINT),
    pair(Slot::Editor(E::Unused), Slot::Editor(E::Background), FAINT),
    pair(Slot::Editor(E::FoldText), Slot::Editor(E::FoldMarker), LABEL),
    pair(Slot::Editor(E::InlayHintForeground), Slot::Editor(E::InlayHintBackground), LABEL),
    pair(Slot::Editor(E::BracketMatchedForeground), Slot::Editor(E::BracketMatchedBackground), LABEL),
    pair(Slot::Editor(E::BracketUnmatchedForeground), Slot::Editor(E::BracketUnmatchedBackground), LABEL),
    pair(Slot::Editor(E::AnnotationRecent), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::Default), Slot::Editor(E::Background), BODY),
    pair(Slot::Syntax(S::Keyword), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::String), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::Number), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::Comment), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::DocComment), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::DocName), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::Macro), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::Type), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::Function), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::Lifetime), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::Constant), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::Attribute), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::Punctuation), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::Heading), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::Strong), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::Emphasis), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::Link), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::Code), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::SemanticPackage), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::SemanticType), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::SemanticBuiltinType), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::SemanticCall), Slot::Editor(E::Background), LABEL),
    pair(Slot::Syntax(S::NavigationLink), Slot::Editor(E::Background), LABEL),
    pair(Slot::Terminal(T::Foreground), Slot::Terminal(T::Background), BODY),
    pair(Slot::Terminal(T::Ansi1), Slot::Terminal(T::Background), LABEL),
    pair(Slot::Terminal(T::Ansi2), Slot::Terminal(T::Background), LABEL),
    pair(Slot::Terminal(T::Ansi3), Slot::Terminal(T::Background), LABEL),
    pair(Slot::Terminal(T::Ansi4), Slot::Terminal(T::Background), LABEL),
    pair(Slot::Terminal(T::Ansi5), Slot::Terminal(T::Background), LABEL),
    pair(Slot::Terminal(T::Ansi6), Slot::Terminal(T::Background), LABEL),
    pair(Slot::Terminal(T::Ansi8), Slot::Terminal(T::Background), LABEL),
    pair(Slot::Terminal(T::Ansi9), Slot::Terminal(T::Background), LABEL),
    pair(Slot::Terminal(T::Ansi10), Slot::Terminal(T::Background), LABEL),
    pair(Slot::Terminal(T::Ansi11), Slot::Terminal(T::Background), LABEL),
    pair(Slot::Terminal(T::Ansi12), Slot::Terminal(T::Background), LABEL),
    pair(Slot::Terminal(T::Ansi13), Slot::Terminal(T::Background), LABEL),
    pair(Slot::Terminal(T::Ansi14), Slot::Terminal(T::Background), LABEL),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Unreadable {
    pub pair: Pair,
    pub contrast: f32,
}

impl fmt::Display for Unreadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} on {}: contrast {:.2}, needs {:.1}",
            self.pair.text, self.pair.background, self.contrast, self.pair.minimum
        )
    }
}

pub fn unreadable(theme: &ResolvedTheme) -> Vec<Unreadable> {
    PAIRS
        .iter()
        .filter_map(|pair| {
            let contrast = pair.text.color(theme).contrast(pair.background.color(theme));
            (contrast < pair.minimum).then_some(Unreadable { pair: *pair, contrast })
        })
        .collect()
}

const SURFACES: [Slot; 6] = [
    Slot::Ui(U::WindowBackground),
    Slot::Ui(U::IslandBackground),
    Slot::Ui(U::PopupBackground),
    Slot::Ui(U::TextFieldBackground),
    Slot::Editor(E::Background),
    Slot::Terminal(T::Background),
];

pub fn make_readable(theme: &mut Theme, find: impl Fn(&str) -> Option<Theme>) -> Vec<String> {
    let mut changed: Vec<Slot> = Vec::new();
    for _ in 0..4 {
        let resolved = theme.resolve(&find).theme;
        let mut fixed = false;
        for found in unreadable(&resolved) {
            let Pair { text, background, minimum } = found.pair;
            let (fore, back) = (text.color(&resolved), background.color(&resolved));
            let (slot, start, other) =
                if SURFACES.contains(&background) { (text, fore, back) } else { (background, back, fore) };
            let target = if other.is_light() { Rgba::rgb(0, 0, 0) } else { Rgba::rgb(0xFF, 0xFF, 0xFF) };
            let mut best = start;
            for step in 1..=20 {
                #[allow(clippy::cast_precision_loss, reason = "a step of at most 20")]
                let candidate = start.towards(target, step as f32 / 20.0);
                best = candidate;
                if candidate.contrast(other) >= minimum {
                    break;
                }
            }
            if best != start {
                slot.set(theme, &resolved, best);
                fixed = true;
                if !changed.contains(&slot) {
                    changed.push(slot);
                }
            }
        }
        if !fixed {
            break;
        }
    }
    changed.sort();
    changed.iter().map(ToString::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_is_the_wcag_ratio() {
        let (black, white) = (Rgba::rgb(0, 0, 0), Rgba::rgb(0xFF, 0xFF, 0xFF));
        assert!((black.contrast(white) - 21.0).abs() < 0.01);
        assert!((white.contrast(white) - 1.0).abs() < 0.001);
        assert!((Rgba::rgb(0x76, 0x76, 0x76).contrast(white) - 4.54).abs() < 0.02);
        assert!(white.is_light() && !black.is_light());
    }

    #[test]
    fn both_built_in_themes_are_readable() {
        for theme in [ResolvedTheme::zen_dark(), ResolvedTheme::zen_light()] {
            let problems: Vec<String> = unreadable(&theme).iter().map(ToString::to_string).collect();
            assert!(problems.is_empty(), "{}: {problems:#?}", theme.id);
        }
    }

    #[test]
    fn unreadable_text_is_darkened_on_light_and_lightened_on_dark() {
        let mut theme = Theme::new("pale", "Pale", false);
        theme.ui.insert(U::TextSecondary, Rgba::rgb(0xD0, 0xD0, 0xD0));
        assert_eq!(make_readable(&mut theme, |_| None), ["ui.text.secondary"]);
        let resolved = theme.resolve(|_| None).theme;
        assert!(unreadable(&resolved).is_empty(), "{:?}", unreadable(&resolved));
        let secondary = resolved.ui(U::TextSecondary);
        assert!(secondary.r() < 0xD0 && secondary.r() == secondary.g(), "darker, still grey: {secondary}");

        let mut theme = Theme::new("mixed", "Mixed", false);
        theme.terminal.insert(T::Background, Rgba::rgb(0x20, 0x22, 0x26));
        let changed = make_readable(&mut theme, |_| None);
        assert!(changed.contains(&"terminal.foreground".to_owned()), "{changed:?}");
        let resolved = theme.resolve(|_| None).theme;
        assert!(unreadable(&resolved).is_empty(), "{:?}", unreadable(&resolved));
        assert!(resolved.terminal(T::Foreground).is_light());
    }

    #[test]
    fn on_a_selection_or_a_diff_line_the_background_gives_way() {
        let mut theme = Theme::new("ink", "Ink", false);
        theme.editor.insert(E::Selection, Rgba::rgb(0x01, 0x23, 0x45));
        theme.editor.insert(E::DiffInserted, Rgba::new(0x00, 0x40, 0x00, 0xC0));
        let changed = make_readable(&mut theme, |_| None);
        assert_eq!(changed, ["editor.selection", "editor.diff.inserted"]);
        let resolved = theme.resolve(|_| None).theme;
        assert!(unreadable(&resolved).is_empty(), "{:?}", unreadable(&resolved));
        assert_eq!(resolved.editor(E::Foreground), ResolvedTheme::zen_light().editor(E::Foreground));
        assert_eq!(resolved.editor(E::DiffInserted).a(), 0xFF, "a mended background is opaque");
    }

    #[test]
    fn a_readable_theme_is_left_alone() {
        let mut theme = crate::zen_light();
        assert!(make_readable(&mut theme, |_| None).is_empty());
        assert_eq!(theme, crate::zen_light());
    }

    #[test]
    fn a_code_style_keeps_its_flags() {
        let mut theme = Theme::new("faded", "Faded", false);
        theme.syntax.insert(S::Constant, crate::Style::color(Rgba::rgb(0xEE, 0xDD, 0xEE)).italic());
        assert_eq!(make_readable(&mut theme, |_| None), ["syntax.constant"]);
        let style = theme.syntax[&S::Constant];
        assert!(style.italic);
        assert!(style.color.contrast(Rgba::rgb(0xFF, 0xFF, 0xFF)) >= LABEL);
    }
}
