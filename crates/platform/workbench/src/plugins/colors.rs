use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_syntax::{ColorSpan, Feature};

use super::PluginFeatures;
use crate::Workbench;

/// The parameters of `editor.colors.list`.
#[derive(Deserialize, JsonSchema)]
pub struct ListParams {
    /// The file: open in the editor, or read from disk.
    pub path: PathBuf,
}

/// A colour literal of a file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ColorLiteral {
    /// The first character of the literal.
    pub start: usize,
    /// The character after its last.
    pub end: usize,
    /// The zero-based line it starts on.
    pub line: usize,
    /// The colour, `#rrggbbaa`.
    pub color: String,
    /// The literal as written, e.g. `rgb(0 0 255)`.
    pub text: String,
}

/// What `editor.colors.list` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ColorList {
    /// The version of the open document they belong to; 0 for a file that is not open.
    pub version: u64,
    /// The literals, in the order of the text.
    pub colors: Vec<ColorLiteral>,
}

/// The parameters of `editor.colors.set`.
#[derive(Deserialize, JsonSchema)]
pub struct SetParams {
    /// The file, open in the editor.
    pub path: PathBuf,
    /// A character offset inside the literal, or at its start.
    pub offset: usize,
    /// The new colour: `#rgb`, `#rrggbb` or `#rrggbbaa`.
    pub color: String,
}

/// What `editor.colors.set` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct SetResult {
    /// Whether a literal was there and changed.
    pub applied: bool,
    /// The literal written, in the notation of the old one.
    pub text: Option<String>,
}

pub(crate) fn spans(session: &mut Session, path: &Path, text: &str, version: Option<u64>) -> Arc<Vec<ColorSpan>> {
    let Some(plugin) = super::plugin_for(session, path, Feature::Colors) else { return Arc::default() };
    let Some(version) = version else { return Arc::new(plugin.colors(text)) };
    if let Some((seen, spans)) = session.state::<PluginFeatures>().and_then(|features| features.colors.get(path))
        && *seen == version
    {
        return Arc::clone(spans);
    }
    let spans = Arc::new(plugin.colors(text));
    let open = super::open_paths(session);
    let features = session.state_mut::<PluginFeatures>();
    features.colors.retain(|known, _| open.contains(known));
    features.colors.insert(path.to_owned(), (version, Arc::clone(&spans)));
    spans
}

pub(crate) fn literals(text: &str, spans: &[ColorSpan]) -> Vec<ColorLiteral> {
    let mut literals = Vec::with_capacity(spans.len());
    let (mut byte, mut chars, mut line) = (0, 0, 0);
    let mut advance = |to: usize| {
        let part = &text[byte..to];
        chars += part.chars().count();
        line += part.bytes().filter(|b| *b == b'\n').count();
        byte = to;
        (chars, line)
    };
    let mut previous_end = 0;
    for span in spans {
        if span.range.end > text.len() || span.range.start < previous_end {
            continue;
        }
        previous_end = span.range.end;
        let (start, at_line) = advance(span.range.start);
        let (end, _) = advance(span.range.end);
        let [r, g, b, a] = span.rgba;
        literals.push(ColorLiteral {
            start,
            end,
            line: at_line,
            color: format!("#{r:02x}{g:02x}{b:02x}{a:02x}"),
            text: text[span.range.clone()].to_owned(),
        });
    }
    literals
}

pub fn parse_hex(color: &str) -> Option<[u8; 4]> {
    let hex = color.strip_prefix('#')?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |at: usize| u8::from_str_radix(&hex[at..at + 1], 16).ok().map(|d| d * 17);
    let pair = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
    match hex.len() {
        3 => Some([digit(0)?, digit(1)?, digit(2)?, 255]),
        4 => Some([digit(0)?, digit(1)?, digit(2)?, digit(3)?]),
        6 => Some([pair(0)?, pair(2)?, pair(4)?, 255]),
        8 => Some([pair(0)?, pair(2)?, pair(4)?, pair(6)?]),
        _ => None,
    }
}

fn alpha(a: u8) -> String {
    let text = format!("{:.2}", f64::from(a) / 255.0);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.is_empty() { "0".to_owned() } else { text.to_owned() }
}

fn to_hsl([r, g, b]: [u8; 3]) -> (u32, u32, u32) {
    let (r, g, b) = (f64::from(r) / 255.0, f64::from(g) / 255.0, f64::from(b) / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let lightness = (max + min) / 2.0;
    let delta = max - min;
    if delta == 0.0 {
        return (0, 0, (lightness * 100.0).round() as u32);
    }
    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs());
    let hue = if max == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    ((hue.round() as u32) % 360, (saturation * 100.0).round() as u32, (lightness * 100.0).round() as u32)
}

pub fn format_like(old: &str, [r, g, b, a]: [u8; 4]) -> String {
    let lower = old.trim().to_ascii_lowercase();
    let opaque = a == 255;
    let modern = !old.contains(',');
    if lower.starts_with("rgb") {
        return match (modern, opaque) {
            (true, true) => format!("rgb({r} {g} {b})"),
            (true, false) => format!("rgb({r} {g} {b} / {})", alpha(a)),
            (false, true) => format!("rgb({r}, {g}, {b})"),
            (false, false) => format!("rgba({r}, {g}, {b}, {})", alpha(a)),
        };
    }
    if lower.starts_with("hsl") {
        let (h, s, l) = to_hsl([r, g, b]);
        return match (modern, opaque) {
            (true, true) => format!("hsl({h} {s}% {l}%)"),
            (true, false) => format!("hsl({h} {s}% {l}% / {})", alpha(a)),
            (false, true) => format!("hsl({h}, {s}%, {l}%)"),
            (false, false) => format!("hsla({h}, {s}%, {l}%, {})", alpha(a)),
        };
    }
    let hex = if opaque { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") };
    if old.starts_with('#') && old.bytes().any(|b| b.is_ascii_uppercase()) { hex.to_ascii_uppercase() } else { hex }
}

fn document_text(session: &Session, path: &Path) -> Option<(String, u64)> {
    let document = session.state::<Workbench>()?.documents().iter().find(|document| document.path() == path)?;
    document.image().is_none().then(|| (document.text().to_owned(), document.version()))
}

pub struct ListColors;

impl Command for ListColors {
    type Params = ListParams;
    type Output = ColorList;
    const ID: &'static str = "editor.colors.list";
    const TITLE: &'static str = "List Colors";
    const DESCRIPTION: &'static str = "Lists the colour literals of a file found by its plug-in (hexadecimal, rgb(), hsl(), \
         named colours), with character offsets, lines and the colour as #rrggbbaa; the gutter shows them as swatches.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<ColorList, CommandError> {
        if super::plugin_for(session, &params.path, Feature::Colors).is_none() {
            return Ok(ColorList::default());
        }
        let (text, version) = match document_text(session, &params.path) {
            Some((text, version)) => (text, Some(version)),
            None => (zen_vfs::read_text(&params.path).map_err(|e| CommandError::new(e.to_string()))?, None),
        };
        let spans = spans(session, &params.path, &text, version);
        Ok(ColorList { version: version.unwrap_or(0), colors: literals(&text, &spans) })
    }
}

pub struct SetColor;

impl Command for SetColor {
    type Params = SetParams;
    type Output = SetResult;
    const ID: &'static str = "editor.colors.set";
    const TITLE: &'static str = "Set Color";
    const DESCRIPTION: &'static str = "Replaces the colour literal at a character offset of an open file with a new colour \
         (#rrggbb or #rrggbbaa), written in the literal's own notation, as one undo step.";

    fn run(&self, session: &mut Session, params: SetParams) -> Result<SetResult, CommandError> {
        let Some(rgba) = parse_hex(params.color.trim()) else {
            return Err(CommandError::new(t!("workbench-error-bad-color", color = params.color.as_str())));
        };
        let (text, version) =
            document_text(session, &params.path).ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let spans = spans(session, &params.path, &text, Some(version));
        let found = literals(&text, &spans)
            .into_iter()
            .find(|literal| literal.start <= params.offset && params.offset < literal.end.max(literal.start + 1));
        let Some(literal) = found else { return Ok(SetResult { applied: false, text: None }) };
        let written = format_like(&literal.text, rgba);
        if written == literal.text {
            return Ok(SetResult { applied: false, text: Some(written) });
        }
        let document = session
            .state_mut::<Workbench>()
            .documents
            .iter_mut()
            .find(|document| document.path() == params.path)
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let applied = document.replace_ranges(vec![(literal.start..literal.end, written.clone())]);
        Ok(SetResult { applied, text: Some(written) })
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListColors)?;
    registry.register(SetColor)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::plugins::testing::session;

    #[test]
    fn formats_in_the_old_notation() {
        assert_eq!(format_like("red", [0, 128, 255, 255]), "#0080ff");
        assert_eq!(format_like("#FFF", [0, 128, 255, 255]), "#0080FF");
        assert_eq!(format_like("#fff", [0, 128, 255, 128]), "#0080ff80");
        assert_eq!(format_like("rgb(1, 2, 3)", [10, 20, 30, 255]), "rgb(10, 20, 30)");
        assert_eq!(format_like("rgb(1, 2, 3)", [10, 20, 30, 64]), "rgba(10, 20, 30, 0.25)");
        assert_eq!(format_like("RGBA(1 2 3 / 50%)", [10, 20, 30, 0]), "rgb(10 20 30 / 0)");
        assert_eq!(format_like("hsl(0, 0%, 0%)", [255, 0, 0, 255]), "hsl(0, 100%, 50%)");
        assert_eq!(format_like("hsl(0 0% 0%)", [0, 0, 255, 255]), "hsl(240 100% 50%)");
        assert_eq!(format_like("hsla(0, 0%, 0%, 1)", [128, 128, 128, 255]), "hsl(0, 0%, 50%)");
        assert_eq!(format_like("hsl(0 0% 0%)", [0, 255, 0, 51]), "hsl(120 100% 50% / 0.2)");
        assert_eq!(parse_hex("#0f08"), Some([0, 255, 0, 136]));
        assert_eq!(parse_hex("#00ff0080"), Some([0, 255, 0, 128]));
        assert_eq!(parse_hex("00ff00"), None);
        assert_eq!(parse_hex("#00ff0"), None);
    }

    #[test]
    fn lists_and_sets_colours() {
        let (registry, mut session, folder, plugin) = session(&[("page.web", "é #f00\nx rgb(0 0 255)\n")]);
        let path = folder.path().join("page.web");
        let listed = registry.dispatch(&mut session, "editor.colors.list", json!({ "path": path })).unwrap();
        assert_eq!(listed["version"], 0, "read from disk");
        assert_eq!(
            listed["colors"],
            json!([
                { "start": 2, "end": 6, "line": 0, "color": "#ff0000ff", "text": "#f00" },
                { "start": 9, "end": 21, "line": 1, "color": "#0000ffff", "text": "rgb(0 0 255)" },
            ])
        );
        assert!(
            registry
                .dispatch(&mut session, "editor.colors.set", json!({ "path": path, "offset": 2, "color": "#000" }))
                .is_err()
        );
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        let calls = *plugin.colors.lock().unwrap();
        registry.dispatch(&mut session, "editor.colors.list", json!({ "path": path })).unwrap();
        registry.dispatch(&mut session, "editor.colors.list", json!({ "path": path })).unwrap();
        assert_eq!(*plugin.colors.lock().unwrap(), calls + 1, "reused while the version is the same");
        let set = |session: &mut Session, offset: usize, color: &str| {
            registry.dispatch(session, "editor.colors.set", json!({ "path": path, "offset": offset, "color": color }))
        };
        assert_eq!(
            set(&mut session, 12, "#00ff0080").unwrap(),
            json!({ "applied": true, "text": "rgb(0 255 0 / 0.5)" })
        );
        assert_eq!(set(&mut session, 2, "#0f0").unwrap(), json!({ "applied": true, "text": "#00ff00" }));
        assert_eq!(set(&mut session, 0, "#0f0").unwrap(), json!({ "applied": false, "text": null }));
        assert!(set(&mut session, 2, "green").is_err());
        let text = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        assert_eq!(text, "é #00ff00\nx rgb(0 255 0 / 0.5)\n");
        let listed = registry.dispatch(&mut session, "editor.colors.list", json!({ "path": path })).unwrap();
        assert_eq!(listed["colors"][1]["color"], "#00ff0080");
        assert_ne!(listed["version"], 0);
        registry.dispatch(&mut session, "editor.edit.undo", json!({})).unwrap();
        let text = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        assert_eq!(text, "é #f00\nx rgb(0 255 0 / 0.5)\n", "one undo step per colour written");
    }
}
