use egui::text::{LayoutJob, TextFormat};
use egui::{FontId, Sense, Ui, pos2, vec2};
use zen_toolbox::markdown::{Block, Marker, Span};

use crate::theme::{self, Palette};

const BODY: f32 = 13.0;
const INDENT: f32 = 14.0;
const MARKER: f32 = 16.0;

pub fn show(ui: &mut Ui, blocks: &[Block], p: Palette) {
    let width = ui.available_width().max(40.0);
    for (index, block) in blocks.iter().enumerate() {
        let first = index == 0;
        match block {
            Block::Heading { level, spans } => {
                if !first {
                    ui.add_space(6.0);
                }
                let size = match level {
                    1 => 15.0,
                    2 => 14.0,
                    _ => 13.5,
                };
                let galley = layout(ui, spans, theme::semibold(size), p, width);
                paint(ui, galley, p);
                ui.add_space(2.0);
            }
            Block::Paragraph(spans) => {
                if !first {
                    ui.add_space(4.0);
                }
                let galley = layout(ui, spans, theme::regular(BODY), p, width);
                paint(ui, galley, p);
            }
            Block::Item { depth, marker, spans } => {
                let indent = f32::from(*depth) * INDENT;
                let text_width = (width - indent - MARKER).max(20.0);
                let galley = layout(ui, spans, theme::regular(BODY), p, text_width);
                let marker = match marker {
                    Marker::Bullet => "•".to_owned(),
                    Marker::Number(number) => format!("{number}."),
                };
                let marker_galley = ui.painter().layout_no_wrap(marker, theme::regular(BODY), p.text_secondary);
                let height = galley.size().y.max(marker_galley.size().y);
                let (rect, _) = ui.allocate_exact_size(vec2(width, height + 2.0), Sense::hover());
                ui.painter().galley(pos2(rect.left() + indent + 2.0, rect.top()), marker_galley, p.text_secondary);
                ui.painter().galley(pos2(rect.left() + indent + MARKER, rect.top()), galley, p.text);
            }
            Block::Code(text) => {
                if !first {
                    ui.add_space(4.0);
                }
                let mut job = LayoutJob::simple(text.clone(), FontId::monospace(12.0), p.text, width - 16.0);
                job.wrap.max_width = width - 16.0;
                let galley = ui.painter().layout_job(job);
                let (rect, _) = ui.allocate_exact_size(vec2(width, galley.size().y + 12.0), Sense::hover());
                ui.painter().rect_filled(rect, 6, p.hover);
                ui.painter().galley(rect.min + vec2(8.0, 6.0), galley, p.text);
            }
        }
    }
}

fn layout(ui: &Ui, spans: &[Span], font: FontId, p: Palette, width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    for span in spans {
        let mut format = TextFormat { font_id: font.clone(), color: p.text, ..TextFormat::default() };
        if span.bold {
            format.font_id = FontId::new(font.size, theme::family::semibold());
        }
        if span.code {
            format.font_id = FontId::monospace(font.size - 1.0);
            format.background = p.hover;
        }
        format.italics = span.italic;
        job.append(&span.text, 0.0, format);
    }
    ui.painter().layout_job(job)
}

fn paint(ui: &mut Ui, galley: std::sync::Arc<egui::Galley>, p: Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), galley.size().y), Sense::hover());
    ui.painter().galley(rect.min, galley, p.text);
}
