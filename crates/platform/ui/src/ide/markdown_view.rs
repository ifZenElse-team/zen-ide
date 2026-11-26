use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Color32, CursorIcon, FontId, Galley, Rect, Sense, Stroke, Ui, pos2, vec2};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;
use zen_workbench::markdown::selection::{self, Fragment, Part, PreviewSelection, Selection, Spot};
use zen_workbench::markdown::{Alert, Align, Block, BlockKind, Container, Inline, Rendered};

use super::markdown_math::{self, Mark, Maths, Shown};
use super::markdown_mermaid::{Mermaids, Shown as MermaidShown};
use super::{IdePalette, IdeView, customization};
use crate::theme;

const COLUMN: f32 = 860.0;
const PADDING: f32 = 28.0;
const BODY: f32 = zen_workbench::preview::DEFAULT_MARKDOWN_FONT_SIZE;
const CODE: f32 = 12.5;
const INDENT: f32 = 24.0;
const IMAGE_LIMIT: u64 = 10 * 1024 * 1024;

#[derive(Default)]
pub(super) struct MarkdownViews {
    scroll: HashMap<PathBuf, Scroll>,
    images: HashMap<String, Picture>,
    anchors: HashMap<PathBuf, String>,
    copied: Option<(PathBuf, usize)>,
    texts: HashMap<PathBuf, Texts>,
    selection: Option<Held>,
    published: Option<(PathBuf, Selection)>,
    maths: Maths,
    mermaids: Mermaids,
}

struct Texts {
    rendered: Arc<Rendered>,
    fragments: Vec<Fragment>,
}

struct Held {
    path: PathBuf,
    rendered: Arc<Rendered>,
    selection: Selection,
}

#[derive(Default)]
struct Frame {
    asked: Vec<Asked>,
    drawn: Vec<Drawn>,
    gesture: Option<Gesture>,
    menu: Option<&'static str>,
}

struct Drawn {
    part: Part,
    galley: Arc<Galley>,
    origin: egui::Pos2,
    highlight: egui::layers::ShapeIdx,
    text: Option<String>,
}

enum Gesture {
    Word(egui::Pos2),
    Block(usize),
}

pub(super) const SELECT_ALL: &str = "view.markdownPreview.selectAll";
pub(super) const COPY: &str = "view.markdownPreview.copy";
pub(super) const COPY_MARKDOWN: &str = "view.markdownPreview.copyMarkdown";

#[derive(Default)]
struct Scroll {
    tops: Vec<f32>,
    offset: f32,
    editor_seen: Option<usize>,
    expected: Option<usize>,
    editor_wanted: Option<usize>,
    asked: Option<f32>,
}

impl MarkdownViews {
    pub(super) fn take_editor_line(&mut self, path: &Path) -> Option<usize> {
        self.scroll.get_mut(path).and_then(|scroll| scroll.editor_wanted.take())
    }

    #[cfg(test)]
    pub(super) fn offset(&self, path: &Path) -> Option<f32> {
        self.scroll.get(path).map(|scroll| scroll.offset)
    }

    #[cfg(test)]
    pub(super) fn math_loading(&self) -> bool {
        self.maths.loading() || self.mermaids.loading()
    }

    fn held(&self, path: &Path, rendered: &Arc<Rendered>) -> Option<Selection> {
        self.selection
            .as_ref()
            .filter(|held| held.path == path && Arc::ptr_eq(&held.rendered, rendered))
            .map(|held| held.selection)
    }

    fn hold(&mut self, path: &Path, rendered: &Arc<Rendered>, selection: Option<Selection>) {
        self.selection =
            selection.map(|selection| Held { path: path.to_owned(), rendered: Arc::clone(rendered), selection });
    }

    pub(super) fn select_all(&mut self, path: &Path) -> bool {
        let Some(texts) = self.texts.get(path) else { return false };
        let all = selection::select_all(&texts.fragments);
        let rendered = Arc::clone(&texts.rendered);
        self.hold(path, &rendered, all);
        true
    }

    pub(super) fn selected(&self, path: &Path, source: Option<&str>) -> Option<String> {
        let texts = self.texts.get(path)?;
        let held = self.held(path, &texts.rendered).filter(|held| !held.is_empty())?;
        Some(match source {
            Some(source) => selection::selected_source(source, &texts.rendered, &held),
            None => selection::selected_text(&texts.rendered, &texts.fragments, &held),
        })
    }
}

pub(super) fn follows_editor(now: Option<usize>, seen: Option<usize>, expected: Option<usize>) -> bool {
    now.is_some() && now != seen && now != expected
}

enum Picture {
    Loading(Arc<OnceLock<Result<Arc<zen_image::Picture>, String>>>),
    Ready(Arc<zen_image::Picture>, Option<egui::TextureHandle>),
    Failed,
}

enum Asked {
    Link(String),
    Caret(u32),
    Toggle(u32),
    Allow(String),
    Copy(usize, String),
}

fn line_at(rendered: &Rendered, tops: &[f32], offset: f32) -> Option<u32> {
    let index = tops.partition_point(|&top| top <= offset).checked_sub(1)?;
    let block = rendered.blocks.get(index)?;
    let top = tops[index];
    let next_top = tops.get(index + 1).copied().unwrap_or(top + 1.0);
    let next_line = rendered.blocks.get(index + 1).map_or(block.end_line + 1, |next| next.line);
    let fraction = ((offset - top) / (next_top - top).max(1.0)).clamp(0.0, 1.0);
    let line = block.line + (next_line.saturating_sub(block.line) as f32 * fraction) as u32;
    Some(line.min(block.end_line.max(block.line)).max(block.line))
}

fn offset_of_line(rendered: &Rendered, tops: &[f32], line: u32) -> Option<f32> {
    let index = zen_workbench::markdown::block_at_line(rendered, line)?;
    let block = rendered.blocks.get(index)?;
    let top = *tops.get(index)?;
    let next_top = tops.get(index + 1).copied().unwrap_or(top);
    let next_line = rendered.blocks.get(index + 1).map_or(block.end_line + 1, |next| next.line);
    let span = next_line.saturating_sub(block.line).max(1) as f32;
    let fraction = (line.saturating_sub(block.line) as f32 / span).clamp(0.0, 1.0);
    Some(top + (next_top - top) * fraction)
}

fn fence_extension(language: &str) -> String {
    let language = language.to_ascii_lowercase();
    let known = match language.as_str() {
        "rust" => "rs",
        "golang" => "go",
        "javascript" | "node" => "js",
        "typescript" => "ts",
        "python" | "py3" => "py",
        "bash" | "shell" | "zsh" | "console" | "shellscript" => "sh",
        "yml" => "yaml",
        "markdown" => "md",
        "dockerfile" | "docker" => "dockerfile",
        "jsonc" | "json5" => "json",
        "htm" | "xhtml" => "html",
        "c++" => "cpp",
        "csharp" | "c#" => "cs",
        "kotlin" => "kt",
        other => other,
    };
    known.to_owned()
}

impl IdeView {
    pub(super) fn markdown_pane(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        path: &Path,
        rendered: &Arc<Rendered>,
    ) {
        let p = self.palette;
        let area = ui.available_rect_before_wrap();
        let editor_top = session.state::<zen_workbench::sticky::ShownTop>().and_then(|shown| shown.get(path));
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
        let sync_scroll = self.overview.settings.markdown.sync_scroll;
        let mut scroll_area = egui::ScrollArea::vertical().id_salt(("markdown", path)).auto_shrink([false, false]);
        {
            let views = &mut self.markdown_views;
            let scroll = views.scroll.entry(path.to_owned()).or_default();
            let anchor = views.anchors.remove(path);
            let mut asked = None;
            if let Some(anchor) = anchor
                && let Some(index) = zen_workbench::markdown::block_of_anchor(rendered, &anchor)
                && let Some(top) = scroll.tops.get(index)
            {
                asked = Some(*top);
            } else if sync_scroll
                && follows_editor(editor_top, scroll.editor_seen, scroll.expected)
                && let Some(top) = editor_top
                && let Some(offset) = offset_of_line(rendered, &scroll.tops, top as u32 + 1)
            {
                asked = Some(offset);
                scroll.expected = None;
            }
            if editor_top.is_some() && editor_top == scroll.expected {
                scroll.expected = None;
            }
            scroll.editor_seen = editor_top;
            if let Some(offset) = asked {
                scroll_area = scroll_area.vertical_scroll_offset(offset);
                scroll.asked = Some(offset);
            }
        }
        let mut frame = Frame::default();
        let settings = self.overview.settings.markdown;
        let body = settings.preview_font_size;
        let mut tops = Vec::with_capacity(rendered.blocks.len());
        let base = path.parent().map(Path::to_owned).unwrap_or_default();
        let copied = self.markdown_views.copied.clone().filter(|(at, _)| at == path).map(|(_, index)| index);
        let focus = ui.make_persistent_id(("markdown-preview", path)).with("surface");
        let selected = self.markdown_views.held(path, rendered).is_some_and(|held| !held.is_empty());
        let output = scroll_area
            .scroll_source(egui::scroll_area::ScrollSource {
                drag: egui::scroll_area::DragScroll::Never,
                ..egui::scroll_area::ScrollSource::ALL
            })
            .show(ui, |ui| {
                let surface = ui.interact(ui.clip_rect(), focus, Sense::drag());
                let width = (ui.available_width() - 2.0 * PADDING).clamp(120.0, COLUMN);
                let left = ui.max_rect().left()
                    + ((ui.available_width() - width) / 2.0).max(PADDING.min(ui.available_width() / 4.0));
                let content_top = ui.cursor().top();
                ui.add_space(PADDING * 0.6);
                for (index, block) in rendered.blocks.iter().enumerate() {
                    tops.push(ui.cursor().top() - content_top);
                    let context = Context {
                        p: &p,
                        left,
                        width,
                        base: &base,
                        root: root.as_deref(),
                        copied: copied == Some(index),
                        selected,
                        index,
                        body,
                    };
                    self.draw_block(ui, session, &context, (index, block), &mut frame);
                }
                ui.add_space(PADDING * 2.0);
                self.select_in_preview(ui, path, rendered, &mut frame, &surface, focus);
            });
        let offset = output.state.clamped_offset().y;
        let views = &mut self.markdown_views;
        let scroll = views.scroll.entry(path.to_owned()).or_default();
        scroll.tops = tops;
        let ours = scroll.asked.take().is_some_and(|wanted| (wanted - offset).abs() < 1.0);
        if sync_scroll
            && (offset - scroll.offset).abs() > 0.5
            && !ours
            && ui.rect_contains_pointer(area)
            && let Some(line) = line_at(rendered, &scroll.tops, offset)
        {
            let top = line.saturating_sub(1) as usize;
            scroll.editor_wanted = Some(top);
            scroll.expected = Some(top);
        }
        scroll.offset = offset;
        if self.markdown_views.images.values().any(|image| matches!(image, Picture::Loading(_))) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        self.markdown_views.maths.start(ui.ctx());
        self.markdown_views.mermaids.start(ui.ctx());
        self.publish_selection(session, path);
        if let Some(action) = frame.menu {
            self.run_action(ui, registry, session, action, serde_json::json!({}));
        }
        for action in frame.asked {
            match action {
                Asked::Caret(line) => {
                    self.dispatch(ui, registry, session, "editor.caret.goTo", serde_json::json!({ "line": line }));
                }
                Asked::Toggle(line) => {
                    self.dispatch(
                        ui,
                        registry,
                        session,
                        "markdown.task.toggle",
                        serde_json::json!({ "line": line, "path": path }),
                    );
                }
                Asked::Allow(host) => {
                    self.dispatch(ui, registry, session, "markdown.images.allow", serde_json::json!({ "host": host }));
                }
                Asked::Copy(index, text) => {
                    ui.ctx().copy_text(text);
                    self.markdown_views.copied = Some((path.to_owned(), index));
                }
                Asked::Link(link) => self.follow_markdown_link(ui, registry, session, path, &link),
            }
        }
    }

    fn follow_markdown_link(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, path: &Path, link: &str) {
        if link.starts_with("http://") || link.starts_with("https://") || link.starts_with("mailto:") {
            ui.ctx().open_url(egui::OpenUrl::new_tab(link));
            return;
        }
        let (file, anchor) = link.split_once('#').map_or((link, None), |(file, anchor)| (file, Some(anchor)));
        if file.is_empty() {
            if let Some(anchor) = anchor {
                self.markdown_views.anchors.insert(path.to_owned(), anchor.to_owned());
            }
            return;
        }
        let target = path.parent().unwrap_or(Path::new("")).join(file);
        if let Some(anchor) = anchor {
            self.markdown_views.anchors.insert(target.clone(), anchor.to_owned());
        }
        self.dispatch(ui, registry, session, "file.document.open", serde_json::json!({ "path": target }));
    }

    fn draw_block(
        &mut self,
        ui: &mut Ui,
        session: &Session,
        context: &Context<'_>,
        (index, block): (usize, &Block),
        frame: &mut Frame,
    ) {
        let p = context.p;
        let indent = block
            .nesting
            .iter()
            .map(|container| match container {
                Container::Quote { .. } => 18.0,
                Container::Item { .. } => INDENT,
                Container::Footnote { .. } => INDENT,
            })
            .sum::<f32>();
        let left = context.left + indent;
        let width = (context.width - indent).max(60.0);
        let gap = match &block.kind {
            BlockKind::Heading { level: 1 | 2, .. } => 22.0,
            BlockKind::Heading { .. } => 16.0,
            _ if block.nesting.iter().any(|c| matches!(c, Container::Item { marker: None, .. })) => 6.0,
            _ if block.nesting.iter().any(|c| matches!(c, Container::Item { .. })) => 4.0,
            _ => 12.0,
        };
        ui.add_space(gap);
        let asked_before = frame.asked.len();
        let top = ui.cursor().top();
        let alert = block.nesting.iter().find_map(|container| match container {
            Container::Quote { alert: Some(alert), first } => Some((*alert, *first)),
            _ => None,
        });
        if let Some((alert, true)) = alert {
            let colour = alert_colour(alert, p);
            let (row, _) = ui.allocate_exact_size(vec2(context.width, 22.0), Sense::hover());
            let at = pos2(left, row.center().y);
            draw_alert_icon(
                ui.painter(),
                Rect::from_center_size(at + vec2(7.0, 0.0), egui::Vec2::splat(14.0)),
                alert,
                colour,
            );
            ui.painter().text(
                at + vec2(20.0, 0.0),
                Align2::LEFT_CENTER,
                alert_title(alert),
                theme::semibold(context.body),
                colour,
            );
        }
        let content_top = ui.cursor().top();
        let response = match &block.kind {
            BlockKind::Heading { level, inlines, .. } => {
                let size = context.scaled(match level {
                    1 => 28.0,
                    2 => 22.0,
                    3 => 18.0,
                    4 => 16.0,
                    5 => 14.5,
                    _ => 13.5,
                });
                let colour = if *level >= 6 { p.text_secondary } else { p.text };
                let galley = inline_galley(ui, &self.markdown_views.maths, inlines, p, width, size, true, colour);
                let response = self.paint_text(ui, galley, p, (left, context.width - indent), frame, Part::of(index));
                if *level <= 2 {
                    let y = ui.cursor().top() + 6.0;
                    ui.painter().hline(left..=left + width, y, Stroke::new(1.0, p.tab_border));
                    ui.add_space(8.0);
                }
                response
            }
            BlockKind::Paragraph { inlines } => self.paragraph(ui, session, context, inlines, left, width, frame),
            BlockKind::Code { language, text } => {
                self.code_block(ui, context, index, language, text, left, width, frame)
            }
            BlockKind::Math { text } => self.display_math(ui, context, index, text, (left, width), frame),
            BlockKind::Table { align, head, rows } => {
                let place = (left, width, context.body);
                table(ui, &self.markdown_views.maths, p, place, align, head, rows, frame, index)
            }
            BlockKind::Rule => {
                let (rect, response) = ui.allocate_exact_size(vec2(context.width, 12.0), Sense::click());
                ui.painter().hline(left..=left + width, rect.center().y, Stroke::new(2.0, p.tab_border));
                response
            }
            BlockKind::Footnotes => {
                let (rect, response) = ui.allocate_exact_size(vec2(context.width, 26.0), Sense::click());
                ui.painter().hline(left..=left + width, rect.top() + 6.0, Stroke::new(1.0, p.tab_border));
                ui.painter().text(
                    pos2(left, rect.top() + 16.0),
                    Align2::LEFT_CENTER,
                    t!("ide-markdown-footnotes"),
                    theme::semibold(12.0),
                    p.text_secondary,
                );
                response
            }
        };
        let bottom = ui.cursor().top();
        let mut x = context.left;
        for container in &block.nesting {
            match container {
                Container::Quote { alert, .. } => {
                    let colour = alert.map_or(p.tab_border, |alert| alert_colour(alert, p));
                    ui.painter().vline(x + 2.0, top - 4.0..=bottom + 2.0, Stroke::new(3.0, colour));
                    x += 18.0;
                }
                Container::Item { marker, task } => {
                    let first_line = content_top + context.body * 0.72;
                    if let Some(task) = task {
                        let rect = Rect::from_center_size(
                            pos2(x + INDENT / 2.0 + 2.0, first_line + 1.0),
                            egui::Vec2::splat(14.0),
                        );
                        let box_response =
                            ui.interact(rect, ui.scope_id().with(("task", block.line, task.line)), Sense::click());
                        if task.checked {
                            ui.painter().rect_filled(rect, 3, p.accent);
                            let tick = [
                                rect.left_center() + vec2(3.0, 0.0),
                                rect.center_bottom() + vec2(-1.0, -4.0),
                                rect.right_top() + vec2(-3.0, 4.0),
                            ];
                            ui.painter().line(tick.to_vec(), Stroke::new(1.8, p.accent_text));
                        } else {
                            ui.painter().rect_stroke(
                                rect,
                                3,
                                Stroke::new(1.2, p.text_secondary),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if box_response.hovered() {
                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        }
                        crate::accessibility::name(&box_response, egui::Role::CheckBox, &t!("ide-markdown-task"));
                        if box_response.clicked() {
                            frame.asked.push(Asked::Toggle(task.line));
                        }
                    } else if let Some(marker) = marker {
                        let bullet = marker.chars().all(|c| !c.is_ascii_digit());
                        let font =
                            if bullet { theme::regular(context.body + 1.0) } else { theme::regular(context.body) };
                        ui.painter().text(
                            pos2(x + INDENT - 6.0, first_line),
                            Align2::RIGHT_CENTER,
                            marker,
                            font,
                            p.text_secondary,
                        );
                    }
                    x += INDENT;
                }
                Container::Footnote { number, first } => {
                    if *first {
                        ui.painter().text(
                            pos2(x + INDENT - 6.0, content_top + context.body * 0.72),
                            Align2::RIGHT_CENTER,
                            format!("{number}."),
                            theme::regular(12.5),
                            p.text_secondary,
                        );
                    }
                    x += INDENT;
                }
            }
        }
        if response.clicked() && frame.asked.len() == asked_before {
            frame.asked.push(Asked::Caret(block.line));
        }
        if response.triple_clicked() {
            frame.gesture = Some(Gesture::Block(index));
        } else if response.double_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            frame.gesture = Some(Gesture::Word(pos));
        }
        response.context_menu(|ui| {
            customization::popup(
                ui,
                customization::MARKDOWN_POPUP,
                |_| true,
                |ui, slot| {
                    let (action, keys, enabled) = match slot {
                        "ide-markdown-copy" => {
                            (COPY, crate::keymap::shortcut(COPY).unwrap_or_else(|| "⌘C".to_owned()), context.selected)
                        }
                        "ide-markdown-copy-markdown" => (
                            COPY_MARKDOWN,
                            crate::keymap::shortcut(COPY_MARKDOWN).unwrap_or_default(),
                            context.selected,
                        ),
                        _ => (
                            SELECT_ALL,
                            crate::keymap::shortcut("editor.selection.selectAll").unwrap_or_default(),
                            true,
                        ),
                    };
                    if ui.add_enabled(enabled, egui::Button::new(t!(slot)).shortcut_text(keys)).clicked() {
                        frame.menu = Some(action);
                        ui.close();
                    }
                },
            );
        });
    }

    #[allow(clippy::too_many_arguments, reason = "the text, where it goes and what it reports")]
    fn paint_text(
        &self,
        ui: &mut Ui,
        laid: Laid,
        p: &IdePalette,
        (left, width): (f32, f32),
        frame: &mut Frame,
        part: Part,
    ) -> egui::Response {
        let Laid { galley, links, marks, errors, copy } = laid;
        let (rect, response) =
            ui.allocate_exact_size(vec2(width.max(galley.size().x), galley.size().y), Sense::click());
        crate::accessibility::name(&response, egui::Role::Label, copy.as_deref().unwrap_or(galley.text()));
        let origin = pos2(left, rect.top());
        let link_at = |pos: egui::Pos2| {
            let index = galley.cursor_from_pos(pos - origin).index.0;
            links.iter().find(|(start, end, _)| (*start..*end).contains(&index)).map(|(_, _, link)| link.clone())
        };
        if let Some(pos) = response.hover_pos()
            && link_at(pos).is_some()
        {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
            && let Some(link) = link_at(pos)
        {
            frame.asked.push(Asked::Link(link));
        }
        let chars: Vec<char> = galley.text().chars().collect();
        for (start, end, link) in &links {
            let area = range_area(&galley, origin, *start, *end);
            if area.width() < 1.0 {
                continue;
            }
            let node = ui.interact(area, ui.scope_id().with(("link", start, link)), Sense::click());
            let name: String = chars.get(*start..(*end).min(chars.len())).unwrap_or_default().iter().collect();
            crate::accessibility::name(&node, egui::Role::Link, name.trim());
            if node.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if node.clicked() {
                frame.asked.push(Asked::Link(link.clone()));
            }
        }
        for (start, end, error) in &errors {
            let area = range_area(&galley, origin, *start, *end);
            if area.width() >= 1.0 {
                let note = ui.interact(area, ui.scope_id().with(("math-error", start)), Sense::hover());
                note.on_hover_text(t!("ide-markdown-math-error", error = error.as_str()));
            }
        }
        frame.drawn_as(ui, part, &galley, origin, copy);
        markdown_math::paint(ui, &galley, origin, &marks);
        ui.painter().galley(origin, galley, p.text);
        response
    }

    #[allow(clippy::too_many_arguments, reason = "the block, where it goes and what it reports")]
    fn paragraph(
        &mut self,
        ui: &mut Ui,
        session: &Session,
        context: &Context<'_>,
        inlines: &[Inline],
        left: f32,
        width: f32,
        frame: &mut Frame,
    ) -> egui::Response {
        let p = context.p;
        let text: Vec<Inline> = inlines.iter().filter(|inline| inline.image.is_none()).cloned().collect();
        let images: Vec<&Inline> = inlines.iter().filter(|inline| inline.image.is_some()).collect();
        let has_text = text.iter().any(|inline| !inline.text.trim().is_empty());
        let mut response = None;
        if has_text || images.is_empty() {
            let galley = inline_galley(ui, &self.markdown_views.maths, &text, p, width, context.body, false, p.text);
            response = Some(self.paint_text(ui, galley, p, (left, context.width), frame, Part::of(context.index)));
        }
        if !images.is_empty() {
            let mut x = left;
            let mut row_top = ui.cursor().top() + if has_text { 6.0 } else { 0.0 };
            let mut row_height: f32 = 0.0;
            let mut placed = Vec::with_capacity(images.len());
            for image in images {
                let size = self.image_size(ui, session, context, image, width);
                if x > left && x + size.x > left + width {
                    row_top += row_height + 6.0;
                    row_height = 0.0;
                    x = left;
                }
                placed.push((image, Rect::from_min_size(pos2(x, row_top), size)));
                x += size.x + 6.0;
                row_height = row_height.max(size.y);
            }
            let bottom = row_top + row_height;
            let (_, rest) =
                ui.allocate_exact_size(vec2(context.width, (bottom - ui.cursor().top()).max(0.0)), Sense::click());
            for (image, rect) in placed {
                self.draw_image(ui, context, image, rect, frame);
            }
            response = Some(match response {
                Some(response) => response.union(rest),
                None => rest,
            });
        }
        response.unwrap_or_else(|| ui.allocate_exact_size(vec2(0.0, 0.0), Sense::hover()).1)
    }

    fn image_key(context: &Context<'_>, src: &str) -> String {
        if src.starts_with("http://") || src.starts_with("https://") {
            return src.to_owned();
        }
        let local = src.strip_prefix("file://").unwrap_or(src);
        let local = local.split(['?', '#']).next().unwrap_or(local);
        let path = Path::new(local);
        let path = if path.is_absolute() { path.to_owned() } else { context.base.join(path) };
        path.display().to_string()
    }

    fn picture(
        &mut self,
        session: &Session,
        context: &Context<'_>,
        src: &str,
    ) -> Result<Option<Arc<zen_image::Picture>>, Blocked> {
        let key = Self::image_key(context, src);
        let remote = key.starts_with("http://") || key.starts_with("https://");
        if remote {
            let host = zen_workbench::markdown::host_of(&key).unwrap_or_default();
            let allowed = context.root.is_some_and(|root| {
                session
                    .state::<zen_workbench::markdown::RemoteImages>()
                    .is_some_and(|images| images.allows(root, &host))
            });
            if !allowed {
                return Err(Blocked::Remote(host));
            }
        }
        let entry = self.markdown_views.images.entry(key.clone()).or_insert_with(|| {
            let slot: Arc<OnceLock<Result<Arc<zen_image::Picture>, String>>> = Arc::default();
            let job = {
                let (slot, key) = (Arc::clone(&slot), key.clone());
                move || {
                    let result = load_image(&key).map(Arc::new);
                    let _ = slot.set(result);
                }
            };
            #[cfg(not(target_arch = "wasm32"))]
            std::thread::spawn(job);
            #[cfg(target_arch = "wasm32")]
            job();
            Picture::Loading(slot)
        });
        if let Picture::Loading(slot) = entry
            && let Some(result) = slot.get()
        {
            *entry = match result {
                Ok(picture) => Picture::Ready(Arc::clone(picture), None),
                Err(_) => Picture::Failed,
            };
        }
        match entry {
            Picture::Loading(_) => Ok(None),
            Picture::Ready(picture, _) => Ok(Some(Arc::clone(picture))),
            Picture::Failed => Err(Blocked::Failed),
        }
    }

    fn image_size(
        &mut self,
        ui: &Ui,
        session: &Session,
        context: &Context<'_>,
        image: &Inline,
        width: f32,
    ) -> egui::Vec2 {
        let src = image.image.as_deref().unwrap_or_default();
        match self.picture(session, context, src) {
            Ok(Some(picture)) => {
                let (w, h) = picture.natural_size();
                let scale = (width / w.max(1.0)).min(1.0);
                vec2(w * scale, h * scale)
            }
            Ok(None) => vec2(120.0_f32.min(width), 60.0),
            Err(_) => {
                let galley = ui.painter().layout_no_wrap(
                    placeholder_text(image, &Blocked::Failed),
                    theme::regular(12.5),
                    context.p.text_secondary,
                );
                vec2((galley.size().x + 40.0).clamp(180.0, width.max(180.0)), 64.0)
            }
        }
    }

    fn draw_image(&mut self, ui: &mut Ui, context: &Context<'_>, image: &Inline, rect: Rect, frame: &mut Frame) {
        let p = context.p;
        let key = Self::image_key(context, image.image.as_deref().unwrap_or_default());
        let entry = self.markdown_views.images.get_mut(&key);
        match entry {
            Some(Picture::Ready(picture, texture)) => {
                let texture = texture.get_or_insert_with(|| {
                    let max_side = ui.ctx().input(|input| input.max_texture_side);
                    let shown = picture.shrunk_to(max_side);
                    let pixels = egui::ColorImage::from_rgba_unmultiplied([shown.width, shown.height], &shown.rgba);
                    ui.ctx().load_texture(format!("markdown-image:{key}"), pixels, egui::TextureOptions::LINEAR)
                });
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                ui.painter().image(texture.id(), rect, uv, Color32::WHITE);
                if let Some(link) = &image.link {
                    let response = ui.interact(rect, ui.scope_id().with(("image-link", &key)), Sense::click());
                    if response.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    if response.clicked() {
                        frame.asked.push(Asked::Link(link.clone()));
                    }
                }
            }
            Some(Picture::Loading(_)) => {
                ui.painter().rect_stroke(rect, 4, Stroke::new(1.0, p.tab_border), egui::StrokeKind::Inside);
                ui.put(
                    Rect::from_center_size(rect.center(), egui::Vec2::splat(16.0)),
                    egui::Spinner::new().size(16.0).color(p.text_secondary),
                );
            }
            other => {
                let blocked = match other {
                    Some(Picture::Failed) => Blocked::Failed,
                    _ => Blocked::Remote(zen_workbench::markdown::host_of(&key).unwrap_or_default()),
                };
                ui.painter().rect_stroke(rect, 4, Stroke::new(1.0, p.tab_border), egui::StrokeKind::Inside);
                ui.painter().text(
                    rect.left_top() + vec2(12.0, 18.0),
                    Align2::LEFT_CENTER,
                    placeholder_text(image, &blocked),
                    theme::regular(12.5),
                    p.text_secondary,
                );
                if let Blocked::Remote(host) = blocked {
                    let label = t!("ide-markdown-load-images", host = host.as_str());
                    let galley = ui.painter().layout_no_wrap(label, theme::regular(12.5), p.link);
                    let at = rect.left_top() + vec2(12.0, 36.0);
                    let link = Rect::from_min_size(at, galley.size());
                    let response = ui.interact(link, ui.scope_id().with(("allow-images", &key)), Sense::click());
                    if response.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    crate::accessibility::name(&response, egui::Role::Link, galley.text());
                    ui.painter().galley(at, galley, p.link);
                    if response.clicked() {
                        frame.asked.push(Asked::Allow(host));
                    }
                }
            }
        }
    }

    fn display_math(
        &self,
        ui: &mut Ui,
        context: &Context<'_>,
        index: usize,
        tex: &str,
        (left, width): (f32, f32),
        frame: &mut Frame,
    ) -> egui::Response {
        let p = context.p;
        let size = context.body * markdown_math::DISPLAY_SCALE;
        let shown = self.markdown_views.maths.get(ui.ctx(), tex, true, size, p.text);
        let Shown::Ready(metrics, texture) = shown else {
            let galley = ui.painter().layout(
                tex.to_owned(),
                FontId::new(13.5, theme::family::mono_italic()),
                p.constant,
                width - 24.0,
            );
            let response = source_box(ui, p, (left, width), galley, &t!("ide-markdown-math"), frame, Part::of(index));
            if let Shown::Failed(error) = shown {
                let message = t!("ide-markdown-math-error", error = error.as_str());
                let note = ui.painter().layout(message.clone(), theme::regular(12.5), p.error_text, width);
                let (rect, noted) = ui.allocate_exact_size(vec2(context.width, note.size().y + 6.0), Sense::hover());
                crate::accessibility::name(&noted, egui::Role::Label, &message);
                ui.painter().galley(pos2(left, rect.top() + 6.0), note, p.error_text);
            }
            return response;
        };
        let shrink = if metrics.width > 0.0 { ((width - 8.0) / metrics.width).clamp(0.4, 1.0) } else { 1.0 };
        let size = vec2(metrics.width, metrics.height()) * shrink;
        let pad = 8.0;
        let (rect, response) = ui.allocate_exact_size(vec2(context.width, size.y + 2.0 * pad), Sense::click());
        let origin = pos2(left + ((width - size.x) / 2.0).max(0.0), rect.top() + pad);
        let copy = markdown_math::copied(tex, true);
        let proxy = markdown_math::unbreakable(&copy);
        let format = markdown_math::proxy(ui, &proxy, size.x, size.y);
        let mut job = LayoutJob::single_section(proxy, format);
        job.wrap.max_width = f32::INFINITY;
        let galley = ui.painter().layout_job(job);
        crate::accessibility::name(&response, egui::Role::Math, tex);
        frame.drawn_as(ui, Part::of(index), &galley, origin, Some(copy));
        markdown_math::draw(ui, texture, Rect::from_min_size(origin, size));
        response
    }

    #[allow(clippy::too_many_arguments, reason = "the block, where it goes and what it reports")]
    fn code_block(
        &self,
        ui: &mut Ui,
        context: &Context<'_>,
        index: usize,
        language: &str,
        text: &str,
        left: f32,
        width: f32,
        frame: &mut Frame,
    ) -> egui::Response {
        let p = context.p;
        let mermaid = zen_workbench::markdown::mermaid::is_mermaid(language);
        let shown = mermaid.then(|| self.markdown_views.mermaids.get(ui.ctx(), text, p));
        if let Some(MermaidShown::Ready(size, texture)) = shown {
            return self.mermaid_diagram(ui, context, index, text, (size, texture), (left, width), frame);
        }
        let highlighter = (!language.is_empty())
            .then(|| self.highlighter_for(&format!("fence.{}", fence_extension(language))))
            .flatten();
        let code = text.strip_suffix('\n').unwrap_or(text);
        let size = context.scaled(CODE);
        let job = super::doc_popups::code_job_sized(code, "", p, highlighter.as_ref(), width - 24.0, size);
        let galley = ui.painter().layout_job(job);
        let label = if mermaid { t!("ide-markdown-mermaid") } else { language.to_owned() };
        let response = source_box(ui, p, (left, width), galley, &label, frame, Part::of(index));
        let copy_text = if context.copied { t!("ide-markdown-copied") } else { t!("ide-markdown-copy") };
        let copy = ui.painter().layout_no_wrap(copy_text, theme::regular(11.5), p.text_secondary);
        let copy_rect = Rect::from_min_size(
            pos2(left + width - copy.size().x - 18.0, response.rect.top() + 6.0),
            copy.size() + vec2(10.0, 6.0),
        );
        if response.hovered() || context.copied || ui.rect_contains_pointer(copy_rect) {
            let button = ui.interact(copy_rect, ui.scope_id().with(("copy-code", index)), Sense::click());
            ui.painter().rect_filled(copy_rect, 4, if button.hovered() { p.hover } else { p.editor_background });
            ui.painter().rect_stroke(copy_rect, 4, Stroke::new(1.0, p.tab_border), egui::StrokeKind::Inside);
            ui.painter().galley(copy_rect.min + vec2(5.0, 3.0), copy, p.text_secondary);
            crate::accessibility::name(&button, egui::Role::Button, &t!("ide-markdown-copy"));
            if button.clicked() {
                frame.asked.push(Asked::Copy(index, code.to_owned()));
            }
        }
        if let Some(MermaidShown::Failed(error)) = shown {
            let message = t!("ide-markdown-mermaid-error", error = error.as_str());
            let note = ui.painter().layout(message.clone(), theme::regular(12.5), p.error_text, width);
            let (rect, noted) = ui.allocate_exact_size(vec2(context.width, note.size().y + 6.0), Sense::hover());
            crate::accessibility::name(&noted, egui::Role::Label, &message);
            ui.painter().galley(pos2(left, rect.top() + 6.0), note, p.error_text);
        }
        response
    }

    #[allow(clippy::too_many_arguments, reason = "the diagram, where it goes and what it reports")]
    fn mermaid_diagram(
        &self,
        ui: &mut Ui,
        context: &Context<'_>,
        index: usize,
        source: &str,
        (natural, texture): (egui::Vec2, egui::TextureId),
        (left, width): (f32, f32),
        frame: &mut Frame,
    ) -> egui::Response {
        let shrink = if natural.x > 0.0 { (width / natural.x).min(1.0) } else { 1.0 };
        let size = natural * shrink;
        let pad = 8.0;
        let (rect, response) = ui.allocate_exact_size(vec2(context.width, size.y + 2.0 * pad), Sense::click());
        let origin = pos2(left + ((width - size.x) / 2.0).max(0.0), rect.top() + pad);
        let copy = format!("```mermaid\n{}\n```", source.trim_end());
        let proxy = markdown_math::unbreakable(&copy);
        let format = markdown_math::proxy(ui, &proxy, size.x, size.y);
        let mut job = LayoutJob::single_section(proxy, format);
        job.wrap.max_width = f32::INFINITY;
        let galley = ui.painter().layout_job(job);
        crate::accessibility::name(&response, egui::Role::Image, &t!("ide-markdown-mermaid-diagram"));
        frame.drawn_as(ui, Part::of(index), &galley, origin, Some(copy));
        markdown_math::draw(ui, texture, Rect::from_min_size(origin, size));
        response
    }
}

impl Frame {
    fn drawn(&mut self, ui: &Ui, part: Part, galley: &Arc<Galley>, origin: egui::Pos2) {
        self.drawn_as(ui, part, galley, origin, None);
    }

    fn drawn_as(&mut self, ui: &Ui, part: Part, galley: &Arc<Galley>, origin: egui::Pos2, text: Option<String>) {
        let highlight = ui.painter().add(egui::Shape::Noop);
        self.drawn.push(Drawn { part, galley: Arc::clone(galley), origin, highlight, text });
    }

    fn spot_at(&self, pos: egui::Pos2) -> Option<Spot> {
        let rect = |drawn: &Drawn| Rect::from_min_size(drawn.origin, drawn.galley.size());
        let cursor =
            |drawn: &Drawn| Spot { part: drawn.part, at: drawn.galley.cursor_from_pos(pos - drawn.origin).index.0 };
        if let Some(drawn) = self.drawn.iter().find(|drawn| rect(drawn).expand(2.0).contains(pos)) {
            return Some(cursor(drawn));
        }
        let beside = self
            .drawn
            .iter()
            .filter(|drawn| rect(drawn).y_range().contains(pos.y))
            .min_by(|a, b| distance_x(rect(a), pos.x).total_cmp(&distance_x(rect(b), pos.x)));
        if let Some(drawn) = beside {
            return Some(cursor(drawn));
        }
        match self.drawn.iter().rfind(|drawn| rect(drawn).top() <= pos.y) {
            Some(drawn) => Some(Spot { part: drawn.part, at: drawn.galley.text().chars().count() }),
            None => self.drawn.first().map(|drawn| Spot { part: drawn.part, at: 0 }),
        }
    }
}

fn range_area(galley: &Galley, origin: egui::Pos2, start: usize, end: usize) -> Rect {
    let first = galley.pos_from_cursor(egui::text::CCursor::new(start));
    let last = galley.pos_from_cursor(egui::text::CCursor::new(end));
    let right = if (last.top() - first.top()).abs() < 1.0 { last.left() } else { galley.size().x };
    Rect::from_min_max(origin + first.left_top().to_vec2(), origin + vec2(right, first.bottom()))
}

fn distance_x(rect: Rect, x: f32) -> f32 {
    if x < rect.left() {
        rect.left() - x
    } else if x > rect.right() {
        x - rect.right()
    } else {
        0.0
    }
}

fn selection_rects(galley: &Galley, origin: egui::Pos2, (from, to): (usize, usize)) -> Vec<Rect> {
    let mut rects = Vec::new();
    let mut index = 0;
    for row in &galley.rows {
        let count = row.char_count_excluding_newline().0;
        let start = from.max(index);
        let end = to.min(index + count);
        let newline = row.ends_with_newline && from <= index + count && to > index + count;
        if start < end || newline {
            let x = |at: usize| row.pos.x + row.x_offset(egui::text::CharIndex(at.saturating_sub(index).min(count)));
            let left = x(start);
            let right = if newline { x(index + count) + 4.0 } else { x(end) };
            rects.push(Rect::from_min_max(
                origin + vec2(left, row.pos.y),
                origin + vec2(right.max(left), row.pos.y + row.size.y),
            ));
        }
        index += row.char_count_including_newline().0;
    }
    rects
}

impl IdeView {
    fn select_in_preview(
        &mut self,
        ui: &Ui,
        path: &Path,
        rendered: &Arc<Rendered>,
        frame: &mut Frame,
        surface: &egui::Response,
        focus: egui::Id,
    ) {
        let views = &mut self.markdown_views;
        let fragments = frame
            .drawn
            .iter()
            .map(|drawn| Fragment::new(drawn.part, drawn.text.as_deref().unwrap_or(drawn.galley.text())))
            .collect::<Vec<_>>();
        let fresh = views
            .texts
            .get(path)
            .is_none_or(|texts| !Arc::ptr_eq(&texts.rendered, rendered) || texts.fragments != fragments);
        if fresh {
            views.texts.insert(path.to_owned(), Texts { rendered: Arc::clone(rendered), fragments });
        }
        let mut selection = views.held(path, rendered);
        let pointer = ui.input(|input| input.pointer.interact_pos());
        if surface.drag_started_by(egui::PointerButton::Primary) {
            ui.memory_mut(|memory| memory.request_focus(focus));
            let origin = ui.input(|input| input.pointer.press_origin()).or(pointer);
            let shift = ui.input(|input| input.modifiers.shift);
            let anchor = match (shift, selection) {
                (true, Some(held)) => Some(held.anchor),
                _ => origin.and_then(|origin| frame.spot_at(origin)),
            };
            selection = anchor.map(Selection::caret);
        }
        if (surface.dragged_by(egui::PointerButton::Primary) || surface.drag_stopped_by(egui::PointerButton::Primary))
            && let (Some(held), Some(pos)) = (selection.as_mut(), pointer)
            && let Some(head) = frame.spot_at(pos)
        {
            held.head = head;
            let clip = ui.clip_rect();
            let push = if pos.y < clip.top() {
                clip.top() - pos.y
            } else if pos.y > clip.bottom() {
                clip.bottom() - pos.y
            } else {
                0.0
            };
            if push != 0.0 && surface.dragged() {
                ui.scroll_with_delta(vec2(0.0, push.clamp(-40.0, 40.0)));
                ui.ctx().request_repaint();
            }
        }
        let texts = views.texts.get(path).map(|texts| texts.fragments.as_slice()).unwrap_or_default();
        match frame.gesture.take() {
            Some(Gesture::Block(index)) => selection = selection::select_block(texts, index).or(selection),
            Some(Gesture::Word(pos)) => {
                if let Some(spot) = frame.spot_at(pos) {
                    selection = Some(selection::select_word(texts, spot));
                }
            }
            None => {}
        }
        if surface.drag_stopped_by(egui::PointerButton::Primary) {
            ui.memory_mut(|memory| memory.request_focus(focus));
        }
        let has_focus = ui.memory(|memory| memory.has_focus(focus));
        if has_focus {
            let keymap = crate::keymap::current();
            let (copy, all) = ui.input(|input| {
                let copy = input.events.iter().any(|event| matches!(event, egui::Event::Copy));
                let all = input.events.iter().any(|event| {
                    matches!(event, egui::Event::Key { key, physical_key, pressed: true, modifiers, .. }
                        if crate::keymap::lookup(&keymap, *key, *physical_key, *modifiers, zen_core::keymap::Context::Editor)
                            .is_some_and(|binding| binding.command == "editor.selection.selectAll"))
                });
                (copy, all)
            });
            if all {
                selection = selection::select_all(texts);
            } else if copy {
                frame.menu = Some(COPY);
            }
        }
        if let Some(held) = selection {
            let colour = self.palette.selection;
            for (drawn, fragment) in frame.drawn.iter().zip(texts) {
                if let Some(range) = held.range_in(fragment) {
                    let shapes = selection_rects(&drawn.galley, drawn.origin, range)
                        .into_iter()
                        .map(|rect| egui::Shape::rect_filled(rect, 0, colour))
                        .collect();
                    ui.painter().set(drawn.highlight, egui::Shape::Vec(shapes));
                }
            }
        }
        if ui.rect_contains_pointer(ui.clip_rect())
            && ui.ctx().output(|output| output.cursor_icon) == CursorIcon::Default
            && pointer.is_some_and(|pos| {
                frame.drawn.iter().any(|drawn| Rect::from_min_size(drawn.origin, drawn.galley.size()).contains(pos))
            })
        {
            ui.ctx().set_cursor_icon(CursorIcon::Text);
        }
        self.markdown_views.hold(path, rendered, selection);
    }

    fn publish_selection(&mut self, session: &mut Session, path: &Path) {
        let views = &self.markdown_views;
        let now = views
            .selection
            .as_ref()
            .filter(|held| held.path == path && !held.selection.is_empty())
            .map(|held| (held.path.clone(), held.selection));
        let ours = views.published.as_ref().is_some_and(|(at, _)| at == path);
        if now == views.published || (now.is_none() && !ours) {
            return;
        }
        let source = session
            .state::<Workbench>()
            .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
            .map(|document| document.text().to_owned());
        let state = session.state_mut::<PreviewSelection>();
        match (views.selected(path, None), source.and_then(|source| views.selected(path, Some(&source)))) {
            (Some(text), markdown) => state.set(path, text, markdown.unwrap_or_default()),
            (None, _) => state.clear(),
        }
        self.markdown_views.published = now;
    }

    pub(super) fn markdown_preview_action(
        &mut self,
        ctx: &egui::Context,
        session: &mut Session,
        action: &str,
    ) -> Result<serde_json::Value, String> {
        let path = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .map(|document| document.path().to_owned())
            .filter(|path| self.markdown_views.texts.contains_key(path))
            .ok_or_else(|| t!("ide-markdown-not-shown"))?;
        if action == SELECT_ALL {
            self.markdown_views.select_all(&path);
            self.publish_selection(session, &path);
            ctx.request_repaint();
        }
        let source = (action == COPY_MARKDOWN)
            .then(|| {
                session
                    .state::<Workbench>()
                    .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
                    .map(|document| document.text().to_owned())
            })
            .flatten();
        let text = self.markdown_views.selected(&path, source.as_deref()).unwrap_or_default();
        if action != SELECT_ALL && !text.is_empty() {
            ctx.copy_text(text.clone());
        }
        Ok(serde_json::json!({ "path": path, "text": text }))
    }
}

enum Blocked {
    Remote(String),
    Failed,
}

fn placeholder_text(image: &Inline, blocked: &Blocked) -> String {
    let description = if image.text.trim().is_empty() { t!("ide-markdown-image") } else { image.text.clone() };
    match blocked {
        Blocked::Remote(_) => description,
        Blocked::Failed => t!("ide-markdown-image-failed", image = description.as_str()),
    }
}

fn load_image(key: &str) -> Result<zen_image::Picture, String> {
    let (bytes, name) = if key.starts_with("http://") || key.starts_with("https://") {
        #[cfg(not(target_arch = "wasm32"))]
        let bytes = zen_update::fetch::document(key, IMAGE_LIMIT, std::time::Duration::from_secs(15))
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "not found".to_owned())?;
        #[cfg(target_arch = "wasm32")]
        let bytes: Vec<u8> = Vec::new();
        let name = key.split(['?', '#']).next().unwrap_or(key).to_owned();
        (bytes, name)
    } else {
        (std::fs::read(key).map_err(|error| error.to_string())?, key.to_owned())
    };
    let svg = zen_image::is_svg(Path::new(&name))
        || bytes.trim_ascii_start().starts_with(b"<svg")
        || bytes.trim_ascii_start().starts_with(b"<?xml");
    if svg { zen_image::decode_svg(&bytes) } else { zen_image::decode(Path::new(&name), &bytes) }
        .map_err(|error| error.to_string())
}

struct Laid {
    galley: Arc<Galley>,
    links: Vec<(usize, usize, String)>,
    marks: Vec<Mark>,
    errors: Vec<(usize, usize, String)>,
    copy: Option<String>,
}

struct Context<'a> {
    p: &'a IdePalette,
    left: f32,
    width: f32,
    base: &'a Path,
    root: Option<&'a Path>,
    copied: bool,
    selected: bool,
    index: usize,
    body: f32,
}

impl Context<'_> {
    fn scaled(&self, size: f32) -> f32 {
        size * self.body / BODY
    }
}

#[allow(clippy::too_many_arguments, reason = "the text and how it is set")]
fn inline_galley(
    ui: &Ui,
    maths: &Maths,
    inlines: &[Inline],
    p: &IdePalette,
    width: f32,
    size: f32,
    heading: bool,
    colour: Color32,
) -> Laid {
    let mut job = LayoutJob::default();
    job.wrap.max_width = width.max(40.0);
    let mut found = Vec::new();
    let mut marks = Vec::new();
    let mut errors = Vec::new();
    let mut copy = String::new();
    let mut chars = 0;
    for inline in inlines {
        let mut text = inline.text.clone();
        if inline.math {
            let shown = maths.get(ui.ctx(), &inline.text, false, size * markdown_math::INLINE_SCALE, colour);
            if let Shown::Ready(metrics, texture) = shown {
                let copied = markdown_math::copied(&inline.text, false);
                let proxy = markdown_math::unbreakable(&copied);
                let count = proxy.chars().count();
                let format =
                    markdown_math::proxy(ui, &proxy, metrics.width, markdown_math::line_height(&metrics, size));
                marks.push(Mark { start: chars, end: chars + count, metrics, texture });
                chars += count;
                copy.push_str(&copied);
                job.append(&proxy, 0.0, format);
                continue;
            }
            if let Shown::Failed(error) = shown {
                errors.push((chars, chars + text.chars().count(), error));
            }
        }
        let font = if inline.code || inline.math {
            FontId::new(
                size - 1.5,
                if inline.math { theme::family::mono_italic() } else { egui::FontFamily::Monospace },
            )
        } else if inline.footnote.is_some() {
            theme::regular(size * 0.72)
        } else if inline.strong || heading {
            if heading { theme::bold(size) } else { theme::semibold(size) }
        } else {
            theme::regular(size)
        };
        let mut colour = if inline.link.is_some() || inline.footnote.is_some() {
            p.link
        } else if inline.math {
            p.constant
        } else {
            colour
        };
        if inline.code {
            colour = p.code;
        }
        let mut format = TextFormat::simple(font, colour);
        format.italics = inline.emphasis;
        format.line_height = Some(size * 1.55);
        if inline.code {
            format.background = p.popup_code;
            text = format!("\u{2009}{text}\u{2009}");
        }
        if inline.strike {
            format.strikethrough = Stroke::new(1.0, colour);
        }
        if errors.last().is_some_and(|(start, _, _)| *start == chars) {
            format.underline = Stroke::new(1.0, p.error_text);
        }
        if inline.footnote.is_some() {
            format.valign = egui::Align::TOP;
            text = format!("[{text}]");
        }
        let target = inline.link.clone().or_else(|| inline.footnote.map(|number| format!("#fn-{number}")));
        if let Some(link) = target {
            if inline.link.is_some() {
                format.underline = Stroke::new(1.0, p.link.gamma_multiply(0.5));
            }
            found.push((chars, chars + text.chars().count(), link));
        }
        chars += text.chars().count();
        copy.push_str(&text);
        job.append(&text, 0.0, format);
    }
    if !marks.is_empty() {
        for section in &mut job.sections {
            if section.format.valign != egui::Align::TOP {
                section.format.valign = egui::Align::Center;
            }
        }
    }
    let copy = (!marks.is_empty()).then_some(copy);
    Laid { galley: ui.painter().layout_job(job), links: found, marks, errors, copy }
}

#[allow(clippy::too_many_arguments, reason = "the box, where it goes and what it reports")]
fn source_box(
    ui: &mut Ui,
    p: &IdePalette,
    (left, width): (f32, f32),
    galley: Arc<Galley>,
    label: &str,
    frame: &mut Frame,
    part: Part,
) -> egui::Response {
    let height = galley.size().y + 24.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let rect = Rect::from_min_size(pos2(left, rect.top()), vec2(width, height));
    ui.painter().rect_filled(rect, 6, p.sticky_background);
    ui.painter().rect_stroke(rect, 6, Stroke::new(1.0, p.tab_border), egui::StrokeKind::Inside);
    frame.drawn(ui, part, &galley, rect.min + vec2(12.0, 12.0));
    ui.painter().galley(rect.min + vec2(12.0, 12.0), galley, p.code);
    if !label.is_empty() && !response.hovered() {
        ui.painter().text(
            rect.right_top() + vec2(-10.0, 9.0),
            Align2::RIGHT_CENTER,
            label,
            theme::regular(11.0),
            p.text_secondary,
        );
    }
    response
}

#[allow(clippy::too_many_arguments, reason = "the table and where it goes")]
fn table(
    ui: &mut Ui,
    maths: &Maths,
    p: &IdePalette,
    (left, width, body): (f32, f32, f32),
    align: &[Align],
    head: &[Vec<Inline>],
    rows: &[Vec<Vec<Inline>>],
    frame: &mut Frame,
    index: usize,
) -> egui::Response {
    let columns = align.len().max(head.len()).max(1);
    let pad = 8.0;
    let natural = |cell: &[Inline], strong: bool| {
        inline_galley(ui, maths, cell, p, f32::INFINITY, body - 0.5, strong, p.text).galley.size().x + 2.0 * pad
    };
    let mut widths = vec![40.0_f32; columns];
    for (column, cell) in head.iter().enumerate().take(columns) {
        widths[column] = widths[column].max(natural(cell, true));
    }
    for row in rows {
        for (column, cell) in row.iter().enumerate().take(columns) {
            widths[column] = widths[column].max(natural(cell, false));
        }
    }
    let total: f32 = widths.iter().sum();
    if total > width {
        let scale = width / total;
        for column in &mut widths {
            *column = (*column * scale).max(48.0);
        }
    }
    let lay = |cells: &[Vec<Inline>], strong: bool| -> Vec<Laid> {
        (0..columns)
            .map(|column| {
                let cell = cells.get(column).map(Vec::as_slice).unwrap_or(&[]);
                inline_galley(ui, maths, cell, p, widths[column] - 2.0 * pad, body - 0.5, strong, p.text)
            })
            .collect()
    };
    let mut laid: Vec<Vec<Laid>> = vec![lay(head, true)];
    laid.extend(rows.iter().map(|row| lay(row, false)));
    let heights: Vec<f32> = laid
        .iter()
        .map(|row| row.iter().map(|laid| laid.galley.size().y).fold(0.0, f32::max) + 2.0 * pad - 2.0)
        .collect();
    let table_width: f32 = widths.iter().sum();
    let total_height: f32 = heights.iter().sum();
    let (rect, response) = ui.allocate_exact_size(vec2(width, total_height), Sense::click());
    let mut y = rect.top();
    for (row_index, row) in laid.into_iter().enumerate() {
        let height = heights[row_index];
        let row_rect = Rect::from_min_size(pos2(left, y), vec2(table_width, height));
        if row_index == 0 {
            ui.painter().rect_filled(row_rect, 0, p.sticky_background);
        } else if row_index % 2 == 0 {
            ui.painter().rect_filled(row_rect, 0, p.current_line);
        }
        let mut x = left;
        for (column, Laid { galley, links, marks, copy, .. }) in row.into_iter().enumerate() {
            let cell_width = widths[column];
            let free = cell_width - 2.0 * pad - galley.size().x;
            let shift = match align.get(column) {
                Some(Align::Center) => free / 2.0,
                Some(Align::Right) => free,
                _ => 0.0,
            }
            .max(0.0);
            let origin = pos2(x + pad + shift, y + pad - 1.0);
            let cell = Rect::from_min_size(pos2(x, y), vec2(cell_width, height));
            if !links.is_empty() {
                let response =
                    ui.interact(cell, ui.scope_id().with(("cell", row_index, column, left as i32)), Sense::click());
                if response.clicked()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let index = galley.cursor_from_pos(pos - origin).index.0;
                    if let Some((_, _, link)) = links.iter().find(|(start, end, _)| (*start..*end).contains(&index)) {
                        frame.asked.push(Asked::Link(link.clone()));
                    }
                }
            }
            frame.drawn_as(ui, Part::cell(index, row_index, column), &galley, origin, copy);
            markdown_math::paint(ui, &galley, origin, &marks);
            ui.painter().galley(origin, galley, p.text);
            ui.painter().vline(x, y..=y + height, Stroke::new(1.0, p.tab_border));
            x += cell_width;
        }
        ui.painter().vline(x, y..=y + height, Stroke::new(1.0, p.tab_border));
        ui.painter().hline(left..=x, y, Stroke::new(1.0, p.tab_border));
        y += height;
    }
    ui.painter().hline(left..=left + table_width, y, Stroke::new(1.0, p.tab_border));
    response
}

fn alert_colour(alert: Alert, p: &IdePalette) -> Color32 {
    match alert {
        Alert::Note => p.link,
        Alert::Tip => p.run,
        Alert::Important => p.keyword,
        Alert::Warning => p.warning_text,
        Alert::Caution => p.error_text,
    }
}

fn alert_title(alert: Alert) -> String {
    match alert {
        Alert::Note => t!("ide-markdown-alert-note"),
        Alert::Tip => t!("ide-markdown-alert-tip"),
        Alert::Important => t!("ide-markdown-alert-important"),
        Alert::Warning => t!("ide-markdown-alert-warning"),
        Alert::Caution => t!("ide-markdown-alert-caution"),
    }
}

fn draw_alert_icon(painter: &egui::Painter, rect: Rect, alert: Alert, colour: Color32) {
    let stroke = Stroke::new(1.4, colour);
    let c = rect.center();
    let r = rect.width() / 2.0;
    match alert {
        Alert::Note => {
            painter.circle_stroke(c, r - 0.7, stroke);
            painter.line_segment([c + vec2(0.0, -0.5), c + vec2(0.0, r * 0.55)], stroke);
            painter.circle_filled(c + vec2(0.0, -r * 0.45), 1.0, colour);
        }
        Alert::Tip => {
            painter.circle_stroke(c + vec2(0.0, -r * 0.2), r * 0.6, stroke);
            painter.line_segment([c + vec2(-r * 0.3, r * 0.75), c + vec2(r * 0.3, r * 0.75)], stroke);
        }
        Alert::Important => {
            painter.rect_stroke(
                Rect::from_min_max(rect.min + vec2(0.7, 0.7), rect.max - vec2(0.7, 3.0)),
                2,
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.line_segment([c + vec2(0.0, -r * 0.5), c + vec2(0.0, r * 0.05)], stroke);
            painter.circle_filled(c + vec2(0.0, r * 0.35), 1.0, colour);
        }
        Alert::Warning => {
            let points = vec![c + vec2(0.0, -r), c + vec2(r, r * 0.85), c + vec2(-r, r * 0.85)];
            painter.add(egui::Shape::closed_line(points, stroke));
            painter.line_segment([c + vec2(0.0, -r * 0.35), c + vec2(0.0, r * 0.25)], stroke);
            painter.circle_filled(c + vec2(0.0, r * 0.55), 1.0, colour);
        }
        Alert::Caution => {
            let points: Vec<egui::Pos2> = (0..8)
                .map(|step| {
                    let angle = std::f32::consts::PI / 8.0 + step as f32 * std::f32::consts::PI / 4.0;
                    c + vec2(angle.cos(), angle.sin()) * (r - 0.5)
                })
                .collect();
            painter.add(egui::Shape::closed_line(points, stroke));
            painter.line_segment([c + vec2(0.0, -r * 0.45), c + vec2(0.0, r * 0.15)], stroke);
            painter.circle_filled(c + vec2(0.0, r * 0.45), 1.0, colour);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zen_workbench::markdown::render;

    #[test]
    fn the_preview_and_the_source_map_lines_and_offsets_both_ways() {
        let text = "# One\n\npara one\nstill one\n\n## Two\n\npara two\n";
        let rendered = render(text);
        let tops = vec![0.0, 50.0, 100.0, 150.0];
        assert_eq!(rendered.blocks.len(), 4);
        assert_eq!(offset_of_line(&rendered, &tops, 1), Some(0.0));
        assert_eq!(offset_of_line(&rendered, &tops, 3), Some(50.0));
        let into = offset_of_line(&rendered, &tops, 4).unwrap();
        assert!((into - (50.0 + 50.0 / 3.0)).abs() < 1e-3, "a line into its block: {into}");
        assert_eq!(offset_of_line(&rendered, &tops, 8), Some(150.0));
        assert_eq!(line_at(&rendered, &tops, 0.0), Some(1));
        assert_eq!(line_at(&rendered, &tops, 50.0), Some(3));
        assert_eq!(line_at(&rendered, &tops, 100.0), Some(6));
        assert_eq!(line_at(&rendered, &tops, 175.0), Some(8));
        assert_eq!(line_at(&render(""), &[], 10.0), None);
    }

    #[test]
    fn an_inline_formula_takes_its_pictures_width_on_the_baseline_of_the_text() {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let metrics = zen_workbench::markdown::math::Metrics { width: 40.0, ascent: 14.0, descent: 5.0 };
        let mut checked = false;
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let shown = markdown_math::unbreakable(&markdown_math::copied(r"\frac{a b}{c-d}", false));
                assert!(!shown.contains(' ') && !shown.contains('-'), "{shown}");
                let height = markdown_math::line_height(&metrics, BODY);
                assert!(height > BODY * 1.55, "a tall formula makes its line taller: {height}");
                let mut job = LayoutJob::default();
                job.wrap.max_width = 600.0;
                let text = |job: &mut LayoutJob, text: &str| {
                    let mut format = TextFormat::simple(theme::regular(BODY), Color32::WHITE);
                    format.line_height = Some(BODY * 1.55);
                    format.valign = egui::Align::Center;
                    job.append(text, 0.0, format);
                };
                text(&mut job, "Area ");
                job.append(&shown, 0.0, markdown_math::proxy(ui, &shown, metrics.width, height));
                text(&mut job, " here");
                let galley = ui.painter().layout_job(job);
                let count = shown.chars().count();
                let mark = Mark { start: 5, end: 5 + count, metrics, texture: egui::TextureId::default() };
                let placed = markdown_math::placed(&galley, egui::Pos2::ZERO, &[mark]);
                let (rect, _) = placed[0];
                let x = |at: usize| galley.pos_from_cursor(egui::text::CCursor::new(at)).left();
                assert!((rect.left() - x(5)).abs() < 0.5, "{rect:?} starts where the formula's text does");
                assert!(
                    (x(5 + count) - rect.right()).abs() < 1.5,
                    "the text goes on after it: {rect:?} {}",
                    x(5 + count)
                );
                assert_eq!(galley.rows.len(), 1, "the formula never breaks");
                let row = &galley.rows[0];
                let baseline = row.pos.y + row.glyphs[0].pos.y;
                assert!(
                    (rect.top() + metrics.ascent - baseline).abs() < 0.5,
                    "{rect:?} sits on the baseline {baseline}"
                );
                assert!(rect.top() >= row.pos.y && rect.bottom() <= row.pos.y + row.size.y, "{rect:?} fits its row");
                checked = true;
            });
            output.textures_delta.clear();
        }
        assert!(checked);
    }

    #[test]
    fn fences_name_their_highlighter() {
        assert_eq!(fence_extension("Rust"), "rs");
        assert_eq!(fence_extension("bash"), "sh");
        assert_eq!(fence_extension("typescript"), "ts");
        assert_eq!(fence_extension("go"), "go");
        assert_eq!(fence_extension("toml"), "toml");
    }

    #[test]
    fn a_fenced_block_is_highlighted_by_the_ides_highlighter_of_its_language() {
        let palette = IdePalette::zen_dark();
        let highlighter: Arc<dyn zen_syntax::Highlighter> = Arc::new(crate::ide::app::markdown_tests::Keywords);
        assert!(highlighter.handles(&format!("fence.{}", fence_extension("rust"))));
        let job = crate::ide::doc_popups::code_job("fn main() { \"hi\" }", "", &palette, Some(&highlighter), 400.0);
        let colour_of = |text: &str| {
            job.sections
                .iter()
                .find(|section| job.text[section.byte_range.start.0..section.byte_range.end.0] == *text)
                .map(|section| section.format.color)
        };
        assert_eq!(
            colour_of("fn"),
            Some(palette.syntax(zen_syntax::HighlightKind::Keyword).format(FontId::monospace(12.5)).color)
        );
        assert_ne!(colour_of("fn"), colour_of(" main() { "));
        assert!(colour_of("\"hi\"").is_some());
    }
}
