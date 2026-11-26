use std::path::PathBuf;

use mermaid_svg::Theme;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;
use zen_image::{Picture, SvgFonts, render_svg_with_fonts};

use super::{BlockKind, render};

pub const MAX_SOURCE: usize = 256 * 1024;

pub const LANGUAGE: &str = "mermaid";

const MAX_DEPTH: usize = 100;

const MAX_INDENT: usize = 400;

const RENDER_STACK: usize = 64 << 20;

pub fn is_mermaid(language: &str) -> bool {
    language.eq_ignore_ascii_case(LANGUAGE)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Palette {
    pub background: [u8; 3],
    pub text: [u8; 3],
    pub muted: [u8; 3],
    pub node: [u8; 3],
    pub border: [u8; 3],
    pub panel: [u8; 3],
}

#[derive(Clone, Debug)]
pub struct Diagram {
    pub width: f32,
    pub height: f32,
    pub picture: Picture,
}

fn hex([red, green, blue]: [u8; 3]) -> String {
    format!("#{red:02x}{green:02x}{blue:02x}")
}

fn is_dark([red, green, blue]: [u8; 3]) -> bool {
    u32::from(red) + u32::from(green) + u32::from(blue) < 3 * 128
}

fn theme(palette: &Palette, family: &str) -> Theme {
    let base = if is_dark(palette.background) { Theme::dark() } else { Theme::default_theme() };
    let (text, muted) = (hex(palette.text), hex(palette.muted));
    let (node, border) = (hex(palette.node), hex(palette.border));
    let (background, panel) = (hex(palette.background), hex(palette.panel));
    Theme {
        fg: text.clone().into(),
        fg_muted: muted.clone().into(),
        bg: background.clone().into(),
        actor_fill: node.clone().into(),
        actor_stroke: border.clone().into(),
        lifeline: muted.clone().into(),
        arrow_stroke: text.clone().into(),
        note_fill: panel.clone().into(),
        note_stroke: border.clone().into(),
        activation_fill: panel.clone().into(),
        activation_stroke: border.clone().into(),
        frame_label_fill: panel.clone().into(),
        flow_node_fill: node.into(),
        flow_node_stroke: border.clone().into(),
        flow_edge_stroke: text.into(),
        flow_label_bg: background.into(),
        flow_cluster_fill: panel.into(),
        flow_cluster_stroke: border.into(),
        responsive: false,
        ..base
    }
    .with_font(family.to_owned())
}

const BLOCK_WORDS: [&str; 10] = ["loop", "alt", "opt", "par", "critical", "break", "rect", "box", "subgraph", "block"];

fn too_deep(source: &str) -> bool {
    let mut depth = 0usize;
    for line in source.lines() {
        let word = line.split_whitespace().next().unwrap_or_default();
        let indent = line.len() - line.trim_start().len();
        depth += usize::from(BLOCK_WORDS.contains(&word)) + line.matches('{').count();
        depth = depth.saturating_sub(usize::from(word == "end") + line.matches('}').count());
        if depth > MAX_DEPTH || indent > MAX_INDENT {
            return true;
        }
    }
    false
}

pub fn svg(source: &str, palette: &Palette, family: &str) -> Result<String, String> {
    if source.len() > MAX_SOURCE {
        return Err(t!("workbench-markdown-mermaid-too-large", size = MAX_SOURCE / 1024));
    }
    if too_deep(source) {
        return Err(t!("workbench-markdown-mermaid-too-deep", depth = MAX_DEPTH));
    }
    let theme = theme(palette, family);
    // WHY: an overflowing stack ends the whole IDE, and catch_unwind does not see it.
    let rendered = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(RENDER_STACK)
            .spawn_scoped(scope, || mermaid_svg::render_with(source, &theme))
            .map(|thread| thread.join().map_err(|_| t!("workbench-markdown-mermaid-crashed")))
            .unwrap_or_else(|_| Err(t!("workbench-markdown-mermaid-crashed")))
    })?;
    rendered.map_err(|error| error.to_string())
}

pub fn draw(source: &str, palette: &Palette, fonts: &SvgFonts, scale: f32) -> Result<Diagram, String> {
    let svg = svg(source, palette, fonts.family())?;
    let picture = render_svg_with_fonts(svg.as_bytes(), scale, fonts).map_err(|error| error.to_string())?;
    let (width, height) = picture.natural_size();
    Ok(Diagram { width, height, picture })
}

/// The parameters of `markdown.mermaid.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// An open file or a file on disk; the active one when absent.
    pub path: Option<PathBuf>,
    /// Also answers each diagram's SVG.
    pub svg: bool,
}

/// One Mermaid block of a file.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagramInfo {
    /// The line of its fence, from 1.
    pub line: u32,
    /// Whether the preview can draw it; when not, the preview shows its source with `error`.
    pub drawn: bool,
    /// Why it is not drawn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The diagram as SVG, with `svg` in the parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub svg: Option<String>,
}

/// What `markdown.mermaid.list` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagramList {
    /// The file.
    pub path: PathBuf,
    /// Its Mermaid blocks in reading order.
    pub diagrams: Vec<DiagramInfo>,
}

pub struct ListDiagrams;

impl Command for ListDiagrams {
    type Params = ListParams;
    type Output = DiagramList;
    const ID: &'static str = "markdown.mermaid.list";
    const TITLE: &'static str = "List Mermaid Diagrams";
    const DESCRIPTION: &'static str = "The ```mermaid blocks of a Markdown file (the active one by default): each one's line, \
         whether the preview can draw it, why not (the preview then shows its source with that note) and, with svg, the \
         diagram as SVG. Nothing is fetched: diagrams are laid out in the IDE.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<DiagramList, CommandError> {
        let (path, text) = super::commands::document_text(session, params.path.as_deref())?;
        let diagrams = render(&text)
            .blocks
            .iter()
            .filter_map(|block| match &block.kind {
                BlockKind::Code { language, text } if is_mermaid(language) => Some((block.line, text.clone())),
                _ => None,
            })
            .map(|(line, source)| {
                let drawn = svg(&source, &Palette::default(), "sans-serif");
                let error = drawn.as_ref().err().cloned();
                DiagramInfo { line, drawn: drawn.is_ok(), error, svg: drawn.ok().filter(|_| params.svg) }
            })
            .collect();
        Ok(DiagramList { path, diagrams })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FONT: &[u8] = include_bytes!("../../../../../assets/fonts/Inter-Regular.ttf");

    const PALETTE: Palette = Palette {
        background: [30, 31, 34],
        text: [223, 225, 229],
        muted: [150, 150, 160],
        node: [43, 45, 48],
        border: [86, 140, 255],
        panel: [50, 52, 56],
    };

    const DIAGRAMS: [(&str, &str); 4] = [
        ("flowchart", "flowchart TD\n  A[Start] --> B{Ready?}\n  B -->|Yes| C[Ship]\n"),
        ("sequence", "sequenceDiagram\n  participant A as Alice\n  A->>Bob: Hello\n  Bob-->>A: Hi\n"),
        ("class", "classDiagram\n  class Animal {\n    +String name\n  }\n  Animal <|-- Dog\n"),
        ("state", "stateDiagram-v2\n  [*] --> Idle\n  Idle --> Running : start\n  Running --> [*]\n"),
    ];

    fn inked(picture: &Picture, colour: [u8; 3]) -> usize {
        picture
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] > 200)
            .filter(|pixel| (0..3).all(|at| pixel[at].abs_diff(colour[at]) < 12))
            .count()
    }

    #[test]
    fn the_four_first_kinds_draw_in_the_themes_colours_with_their_text() {
        let fonts = SvgFonts::new(&[FONT], "Inter");
        for (name, source) in DIAGRAMS {
            let diagram = draw(source, &PALETTE, &fonts, 2.0).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert!(diagram.width > 50.0 && diagram.height > 50.0, "{name}: {}x{}", diagram.width, diagram.height);
            assert_eq!(diagram.picture.width, (diagram.width * 2.0).ceil() as usize, "{name}: drawn at the density");
            assert!(inked(&diagram.picture, PALETTE.text) > 50, "{name}: the text is drawn in the theme's colour");
        }
    }

    #[test]
    fn a_diagram_it_cannot_read_says_why() {
        let error = svg("gantt-of-nothing\n  what is this", &PALETTE, "Inter").unwrap_err();
        assert!(!error.is_empty());
    }

    #[test]
    fn odd_sources_never_take_the_ide_down() {
        for source in
            ["", "\n\n", "flowchart", "flowchart TD\n  A[", "sequenceDiagram\n  ->>", "classDiagram\n  {", "%%{init"]
        {
            let _ = svg(source, &PALETTE, "Inter");
        }
    }

    #[test]
    fn a_huge_diagram_is_refused_before_it_is_read() {
        zen_i18n::register(crate::LOCALES);
        let huge = "flowchart TD\n".repeat(MAX_SOURCE / 8);
        assert!(svg(&huge, &PALETTE, "Inter").unwrap_err().contains("KiB"));
    }

    #[test]
    fn deep_nesting_is_refused_before_it_can_overflow_the_stack() {
        zen_i18n::register(crate::LOCALES);
        let deep = |open: &str, close: &str, head: &str, count: usize| {
            format!("{head}\n{}{}", open.repeat(count), close.repeat(count))
        };
        for source in [
            deep("loop x\n", "end\n", "sequenceDiagram", 15_000),
            deep("subgraph s\n", "end\n", "flowchart TD", 15_000),
            deep("state s {\n", "}\n", "stateDiagram-v2", 15_000),
            deep("Boundary(b,\"b\") {\n", "}\n", "C4Context", 10_000),
        ] {
            assert!(source.len() <= MAX_SOURCE, "{}", source.len());
            assert!(svg(&source, &PALETTE, "Inter").unwrap_err().contains("deep"));
        }
        let indented: String = (0..600).map(|depth| format!("{}n\n", " ".repeat(depth + 1))).collect();
        assert!(svg(&format!("mindmap\n{indented}"), &PALETTE, "Inter").is_err());
    }

    #[test]
    fn nesting_at_the_limit_draws_on_a_small_stack() {
        let flow: String = "subgraph s\n".repeat(MAX_DEPTH);
        let source = format!("flowchart TD\n{flow}A-->B\n{}", "end\n".repeat(MAX_DEPTH));
        let fonts = SvgFonts::new(&[FONT], "Inter");
        let small = std::thread::Builder::new().stack_size(2 << 20);
        let drawn = small.spawn(move || draw(&source, &PALETTE, &fonts, 1.0).map(|_| ())).unwrap().join().unwrap();
        assert_eq!(drawn, Ok(()));
    }

    #[test]
    fn a_files_diagrams_are_listed_for_agents() {
        let mut registry = zen_core::Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.md");
        registry.dispatch(&mut session, "file.project.open", serde_json::json!({ "path": dir.path() })).unwrap();
        std::fs::write(&file, "# A\n\n```mermaid\nflowchart LR\n  A --> B\n```\n\n```mermaid\nnonsense\n```\n")
            .unwrap();
        let answer = registry
            .dispatch(&mut session, "markdown.mermaid.list", serde_json::json!({ "path": file, "svg": true }))
            .unwrap();
        let diagrams = answer["diagrams"].as_array().unwrap();
        assert_eq!(diagrams.len(), 2, "{answer}");
        assert_eq!(diagrams[0]["line"], 3);
        assert_eq!(diagrams[0]["drawn"], true);
        assert!(diagrams[0]["svg"].as_str().unwrap().contains("<svg"));
        assert_eq!(diagrams[1]["drawn"], false);
        assert!(diagrams[1]["error"].is_string(), "{answer}");
    }
}
