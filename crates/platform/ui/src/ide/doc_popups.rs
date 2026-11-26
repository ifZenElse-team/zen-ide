use std::path::PathBuf;
use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, CursorIcon, FontId, Key, Modifiers, Rect, Sense, Stroke, Ui, pos2, vec2};
use pulldown_cmark::{CodeBlockKind, Event as Markdown, Parser, Tag, TagEnd};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_syntax::Highlighter;
use zen_workbench::Workbench;
use zen_workbench::language::docs::Popups;

use super::IdePalette;

const DOC_WIDTH: f32 = 520.0;
const DOC_HEIGHT: f32 = 400.0;
const BODY: f32 = 13.0;
const CODE_SIZE: f32 = 12.5;
const INDENT: f32 = 14.0;
const MARKER: f32 = 16.0;

#[derive(Default)]
pub(super) struct DocPopups {
    documentation: Option<ShownDocumentation>,
    parameter_info: Option<Value>,
}

struct ShownDocumentation {
    state: Value,
    markdown: Option<String>,
    blocks: Vec<Block>,
    selection: Option<(PathBuf, u64)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Block {
    Heading(u8, Vec<Inline>),
    Paragraph(Vec<Inline>),
    Item { depth: usize, marker: Option<String>, spans: Vec<Inline> },
    Code { language: String, text: String },
    Rule,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Inline {
    pub(super) text: String,
    pub(super) strong: bool,
    pub(super) emphasis: bool,
    pub(super) code: bool,
    pub(super) link: Option<String>,
}

pub(super) fn parse(markdown: &str) -> Vec<Block> {
    let mut reader = Reader::default();
    for event in Parser::new(markdown) {
        reader.event(event);
    }
    reader.flush();
    reader.blocks
}

#[derive(Default)]
struct Reader {
    blocks: Vec<Block>,
    spans: Vec<Inline>,
    strong: usize,
    emphasis: usize,
    link: Option<String>,
    lists: Vec<Option<u64>>,
    marker: Option<String>,
    heading: Option<u8>,
    code: Option<(String, String)>,
}

impl Reader {
    fn event(&mut self, event: Markdown<'_>) {
        match event {
            Markdown::Start(tag) => self.start(tag),
            Markdown::End(tag) => self.end(tag),
            Markdown::Text(text) => match &mut self.code {
                Some((_, code)) => code.push_str(&text),
                None => self.push(&text, false),
            },
            Markdown::Code(text) => self.push(&text, true),
            Markdown::SoftBreak => self.push(" ", false),
            Markdown::HardBreak => self.push("\n", false),
            Markdown::Rule => {
                self.flush();
                self.blocks.push(Block::Rule);
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.flush();
                self.heading = Some(level as u8);
            }
            Tag::List(first) => {
                self.flush();
                self.lists.push(first);
            }
            Tag::Item => {
                self.flush();
                self.marker = Some(match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let marker = format!("{number}.");
                        *number += 1;
                        marker
                    }
                    _ => "•".to_owned(),
                });
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or_default().to_owned(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((language, String::new()));
            }
            Tag::Emphasis => self.emphasis += 1,
            Tag::Strong => self.strong += 1,
            Tag::Link { dest_url, .. } => self.link = Some(dest_url.to_string()),
            Tag::Paragraph | Tag::BlockQuote(_) => self.flush(),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(level) => {
                self.heading = None;
                let spans = std::mem::take(&mut self.spans);
                if !spans.is_empty() {
                    self.blocks.push(Block::Heading(level as u8, spans));
                }
            }
            TagEnd::Paragraph | TagEnd::Item | TagEnd::BlockQuote(..) => self.flush(),
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
            }
            TagEnd::CodeBlock => {
                if let Some((language, text)) = self.code.take() {
                    self.blocks.push(Block::Code { language, text: text.trim_end_matches('\n').to_owned() });
                }
            }
            TagEnd::Emphasis => self.emphasis = self.emphasis.saturating_sub(1),
            TagEnd::Strong => self.strong = self.strong.saturating_sub(1),
            TagEnd::Link => self.link = None,
            _ => {}
        }
    }

    fn push(&mut self, text: &str, code: bool) {
        let span = Inline {
            text: text.to_owned(),
            strong: self.strong > 0,
            emphasis: self.emphasis > 0,
            code,
            link: self.link.clone(),
        };
        match self.spans.last_mut() {
            Some(last)
                if (last.strong, last.emphasis, last.code, &last.link)
                    == (span.strong, span.emphasis, span.code, &span.link) =>
            {
                last.text.push_str(text);
            }
            _ => self.spans.push(span),
        }
    }

    fn flush(&mut self) {
        if self.spans.is_empty() || self.heading.is_some() {
            return;
        }
        let spans = std::mem::take(&mut self.spans);
        if self.lists.is_empty() {
            self.blocks.push(Block::Paragraph(spans));
        } else {
            self.blocks.push(Block::Item { depth: self.lists.len() - 1, marker: self.marker.take(), spans });
        }
    }
}

pub(super) fn parameter_parts(signature: &Value) -> Vec<(String, bool)> {
    let label: Vec<char> = signature["label"].as_str().unwrap_or_default().chars().collect();
    let active = signature["activeParameter"].as_u64().map(|index| index as usize);
    signature["parameters"]
        .as_array()
        .map(|list| {
            list.iter()
                .enumerate()
                .map(|(index, range)| {
                    let start = range[0].as_u64().unwrap_or(0) as usize;
                    let end = (range[1].as_u64().unwrap_or(0) as usize).min(label.len());
                    let text: String = label.get(start.min(end)..end).unwrap_or_default().iter().collect();
                    (text, active == Some(index))
                })
                .collect()
        })
        .unwrap_or_default()
}

impl DocPopups {
    pub(super) fn keys(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        if self.documentation.is_none() && self.parameter_info.is_none() {
            return;
        }
        if !ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
            return;
        }
        if self.documentation.take().is_some() {
            let _ = registry.dispatch(session, "editor.documentation.hide", json!({}));
        } else if self.parameter_info.take().is_some() {
            let _ = registry.dispatch(session, "editor.parameterInfo.hide", json!({}));
        }
    }

    pub(super) fn follow(&mut self, ui: &Ui, registry: &Registry, session: &mut Session) {
        let (documentation, parameter_info) = session
            .state::<Popups>()
            .map_or((false, false), |popups| (popups.documentation_open(), popups.parameter_info_open()));
        let selection = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .map(|document| (document.path().to_owned(), document.selection_version()));
        let mut pending = false;
        if !documentation {
            self.documentation = None;
        } else if self.documentation.as_ref().is_some_and(|shown| shown.selection != selection) {
            self.documentation = None;
            let _ = registry.dispatch(session, "editor.documentation.hide", json!({}));
        } else {
            let state = registry.dispatch(session, "editor.documentation.state", json!({})).unwrap_or_default();
            let markdown = state["markdown"].as_str().map(str::to_owned);
            pending |= state["pending"] == true;
            match &mut self.documentation {
                Some(shown) if shown.markdown == markdown => shown.state = state,
                _ if state["open"] == true => {
                    let blocks = markdown.as_deref().map(parse).unwrap_or_default();
                    let selection = self.documentation.take().map_or(selection, |shown| shown.selection);
                    self.documentation = Some(ShownDocumentation { state, markdown, blocks, selection });
                }
                _ => self.documentation = None,
            }
        }
        self.parameter_info = if parameter_info {
            registry
                .dispatch(session, "editor.parameterInfo.state", json!({}))
                .ok()
                .filter(|state| state["open"] == true)
                .inspect(|state| pending |= state["pending"] == true)
        } else {
            None
        };
        if pending {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    pub(super) fn show(
        &self,
        ui: &Ui,
        palette: &IdePalette,
        caret: Option<Rect>,
        area: Rect,
        highlighter: Option<&Arc<dyn Highlighter>>,
    ) {
        let Some(caret) = caret else { return };
        let mut covered = 0.0;
        if let Some(state) = &self.parameter_info {
            covered = self.show_parameter_info(ui, palette, caret, state);
        }
        let Some(shown) = &self.documentation else { return };
        let below = area.bottom() - caret.bottom();
        let above_room = caret.top() - area.top() - covered;
        let above = below < 160.0 && above_room > below;
        let (pivot, at, room) = if above {
            (Align2::LEFT_BOTTOM, caret.left_top() + vec2(-8.0, -4.0 - covered), above_room)
        } else {
            (Align2::LEFT_TOP, caret.left_bottom() + vec2(-8.0, 4.0), below)
        };
        let width = DOC_WIDTH.min(area.width() - 24.0).max(200.0);
        egui::Area::new(ui.scope_id().with("quick-documentation"))
            .order(egui::Order::Foreground)
            .pivot(pivot)
            .fixed_pos(at)
            .show(ui.ctx(), |ui| {
                popup_frame(palette).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
                    ui.set_max_width(width);
                    egui::ScrollArea::vertical().max_height((room - 16.0).clamp(80.0, DOC_HEIGHT)).show(ui, |ui| {
                        let state = &shown.state;
                        let message = if let Some(error) = state["error"].as_str() {
                            Some(error.to_owned())
                        } else if shown.blocks.is_empty() && state["pending"] == true {
                            Some(t!("ide-doc-loading"))
                        } else if shown.blocks.is_empty() {
                            Some(t!("ide-doc-none"))
                        } else {
                            None
                        };
                        match message {
                            Some(message) => {
                                ui.label(
                                    egui::RichText::new(message)
                                        .font(crate::theme::regular(BODY))
                                        .color(palette.text_secondary),
                                );
                            }
                            None => render(ui, &shown.blocks, palette, width, highlighter),
                        }
                    });
                });
            });
    }

    fn show_parameter_info(&self, ui: &Ui, palette: &IdePalette, caret: Rect, state: &Value) -> f32 {
        let signatures = state["signatures"].as_array().cloned().unwrap_or_default();
        if signatures.is_empty() {
            return 0.0;
        }
        let shown = egui::Area::new(ui.scope_id().with("parameter-info"))
            .order(egui::Order::Foreground)
            .pivot(Align2::LEFT_BOTTOM)
            .fixed_pos(caret.left_top() + vec2(-8.0, -2.0))
            .interactable(false)
            .show(ui.ctx(), |ui| {
                popup_frame(palette).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for signature in &signatures {
                        let mut job = LayoutJob::default();
                        let parts = parameter_parts(signature);
                        let dim = TextFormat::simple(crate::theme::regular(BODY), palette.text_secondary);
                        if parts.is_empty() {
                            job.append(&t!("ide-params-none"), 0.0, dim.clone());
                        }
                        for (index, (text, active)) in parts.iter().enumerate() {
                            if index > 0 {
                                job.append(", ", 0.0, dim.clone());
                            }
                            let format = if *active {
                                TextFormat::simple(crate::theme::semibold(BODY), palette.text)
                            } else {
                                dim.clone()
                            };
                            job.append(text, 0.0, format);
                        }
                        ui.add(egui::Label::new(job).extend());
                    }
                });
            });
        shown.response.rect.height() + 2.0
    }
}

pub(super) fn popup_frame(palette: &IdePalette) -> egui::Frame {
    palette.popup_frame().corner_radius(8)
}

pub(super) fn render(
    ui: &mut Ui,
    blocks: &[Block],
    palette: &IdePalette,
    width: f32,
    highlighter: Option<&Arc<dyn Highlighter>>,
) {
    ui.spacing_mut().item_spacing.y = 0.0;
    for (index, block) in blocks.iter().enumerate() {
        if index > 0 {
            ui.add_space(6.0);
        }
        match block {
            Block::Heading(level, spans) => {
                let size = match level {
                    1 => 15.0,
                    2 => 14.0,
                    _ => 13.5,
                };
                text_block(ui, spans, palette, size, true, width, 0.0);
            }
            Block::Paragraph(spans) => text_block(ui, spans, palette, BODY, false, width, 0.0),
            Block::Item { depth, marker, spans } => {
                let indent = *depth as f32 * INDENT;
                let top = ui.cursor().top();
                text_block(ui, spans, palette, BODY, false, width - indent - MARKER, indent + MARKER);
                if let Some(marker) = marker {
                    let left = ui.min_rect().left() + indent;
                    ui.painter().text(
                        pos2(left, top),
                        Align2::LEFT_TOP,
                        marker,
                        crate::theme::regular(BODY),
                        palette.text_secondary,
                    );
                }
            }
            Block::Code { language, text } => {
                let job = code_job(text, language, palette, highlighter, width);
                let galley = ui.painter().layout_job(job);
                let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
                ui.painter().galley(rect.min, galley, palette.code);
            }
            Block::Rule => {
                let (rect, _) = ui.allocate_exact_size(vec2(width, 7.0), Sense::hover());
                ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, palette.popup_border));
            }
        }
    }
}

fn text_block(ui: &mut Ui, spans: &[Inline], palette: &IdePalette, size: f32, strong: bool, width: f32, left: f32) {
    let mut job = LayoutJob::default();
    job.wrap.max_width = width.max(40.0);
    let mut links: Vec<(usize, usize, &str)> = Vec::new();
    let mut chars = 0;
    for span in spans {
        let font = if span.code {
            FontId::monospace(size - 0.5)
        } else if span.strong || strong {
            crate::theme::semibold(size)
        } else {
            crate::theme::regular(size)
        };
        let color = if span.link.is_some() { palette.link } else { palette.text };
        let mut format = TextFormat::simple(font, color);
        format.italics = span.emphasis;
        if span.code {
            format.background = palette.popup_code;
        }
        if let Some(link) = &span.link {
            format.underline = Stroke::new(1.0, palette.link);
            links.push((chars, chars + span.text.chars().count(), link));
        }
        chars += span.text.chars().count();
        job.append(&span.text, 0.0, format);
    }
    let galley = ui.painter().layout_job(job);
    let (rect, response) = ui.allocate_exact_size(vec2(left, 0.0) + galley.size(), Sense::click());
    crate::accessibility::name(&response, egui::Role::Label, galley.text());
    let origin = rect.min + vec2(left, 0.0);
    let link_at = |pos: egui::Pos2| {
        let index = galley.cursor_from_pos(pos - origin).index.0;
        links.iter().find(|(start, end, _)| (*start..*end).contains(&index)).map(|(_, _, link)| *link)
    };
    if let Some(pos) = response.hover_pos()
        && link_at(pos).is_some()
    {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && let Some(link) = link_at(pos)
        && (link.starts_with("https://") || link.starts_with("http://"))
    {
        ui.ctx().open_url(egui::OpenUrl::new_tab(link));
    }
    ui.painter().galley(origin, galley, palette.text);
}

pub(super) fn code_job(
    text: &str,
    language: &str,
    palette: &IdePalette,
    highlighter: Option<&Arc<dyn Highlighter>>,
    width: f32,
) -> LayoutJob {
    code_job_sized(text, language, palette, highlighter, width, CODE_SIZE)
}

pub(super) fn code_job_sized(
    text: &str,
    language: &str,
    palette: &IdePalette,
    highlighter: Option<&Arc<dyn Highlighter>>,
    width: f32,
    size: f32,
) -> LayoutJob {
    let font = FontId::monospace(size);
    let mut job = LayoutJob::default();
    job.wrap.max_width = width.max(40.0);
    let plain = TextFormat::simple(font.clone(), palette.code);
    let highlighter =
        highlighter.filter(|highlighter| language.is_empty() || highlighter.handles(&format!("fence.{language}")));
    let Some(highlighter) = highlighter else {
        job.append(text, 0.0, plain);
        return job;
    };
    let mut at = 0;
    for span in highlighter.highlight(text) {
        let (start, end) = (span.range.start.max(at), span.range.end.min(text.len()));
        if start >= end || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            continue;
        }
        if start > at {
            job.append(&text[at..start], 0.0, plain.clone());
        }
        job.append(&text[start..end], 0.0, palette.syntax(span.kind).format(font.clone()));
        at = end;
    }
    if at < text.len() {
        job.append(&text[at..], 0.0, plain);
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Inline {
        Inline { text: text.to_owned(), ..Inline::default() }
    }

    #[test]
    fn markdown_becomes_blocks() {
        let markdown = "# Title\n\nSome *em*, **strong**, `code` and [a link](https://go.dev).\nNext line.\n\n\
                        - one\n- two\n  1. three\n  2. four\n\n```go\nfunc f()\n```\n\n---\n\n## After\n\n    indented";
        let blocks = parse(markdown);
        assert_eq!(
            blocks,
            vec![
                Block::Heading(1, vec![plain("Title")]),
                Block::Paragraph(vec![
                    plain("Some "),
                    Inline { emphasis: true, ..plain("em") },
                    plain(", "),
                    Inline { strong: true, ..plain("strong") },
                    plain(", "),
                    Inline { code: true, ..plain("code") },
                    plain(" and "),
                    Inline { link: Some("https://go.dev".to_owned()), ..plain("a link") },
                    plain(". Next line."),
                ]),
                Block::Item { depth: 0, marker: Some("•".to_owned()), spans: vec![plain("one")] },
                Block::Item { depth: 0, marker: Some("•".to_owned()), spans: vec![plain("two")] },
                Block::Item { depth: 1, marker: Some("1.".to_owned()), spans: vec![plain("three")] },
                Block::Item { depth: 1, marker: Some("2.".to_owned()), spans: vec![plain("four")] },
                Block::Code { language: "go".to_owned(), text: "func f()".to_owned() },
                Block::Rule,
                Block::Heading(2, vec![plain("After")]),
                Block::Code { language: String::new(), text: "indented".to_owned() },
            ]
        );
    }

    #[test]
    fn loose_items_keep_their_paragraphs_and_breaks() {
        let blocks = parse("1. first\n\n   more\n2. second  \n   line\n\nplain\\*text");
        assert_eq!(
            blocks,
            vec![
                Block::Item { depth: 0, marker: Some("1.".to_owned()), spans: vec![plain("first")] },
                Block::Item { depth: 0, marker: None, spans: vec![plain("more")] },
                Block::Item { depth: 0, marker: Some("2.".to_owned()), spans: vec![plain("second\nline")] },
                Block::Paragraph(vec![plain("plain*text")]),
            ]
        );
        assert!(parse("").is_empty());
    }

    #[test]
    fn parameters_are_cut_from_the_label() {
        let signature = json!({
            "label": "func(формат string, a ...any)",
            "parameters": [[5, 18], [20, 28]],
            "activeParameter": 1
        });
        assert_eq!(
            parameter_parts(&signature),
            vec![("формат string".to_owned(), false), ("a ...any".to_owned(), true)]
        );
        assert!(parameter_parts(&json!({ "label": "main()", "parameters": [] })).is_empty());
    }
}
