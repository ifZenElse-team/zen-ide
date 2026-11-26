use std::sync::OnceLock;

use latex_rust::{BoxContent, Color, Dim, MathBox, MathFont, MathStyle, SvgOptions};

const SVG_POINTS_PER_PIXEL: f32 = 0.75;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metrics {
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
}

impl Metrics {
    pub fn height(&self) -> f32 {
        self.ascent + self.descent
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    Glyph(char),
    Rule,
    Line,
    Frame,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub ink: Ink,
    pub left: f32,
    pub raise: f32,
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
}

impl Piece {
    pub fn right(&self) -> f32 {
        self.left + self.width
    }

    pub fn top(&self) -> f32 {
        self.raise + self.ascent
    }

    pub fn bottom(&self) -> f32 {
        self.raise - self.descent
    }
}

#[derive(Clone, Debug)]
pub struct Formula {
    pub metrics: Metrics,
    pub picture: zen_image::Picture,
}

fn font() -> Result<&'static MathFont, String> {
    static FONT: OnceLock<Result<MathFont, String>> = OnceLock::new();
    FONT.get_or_init(|| MathFont::stix_two_math().map_err(|error| error.to_string())).as_ref().map_err(Clone::clone)
}

fn points(dim: &Dim) -> f32 {
    f32::from_bits(dim.to_ieee32_bits())
}

fn dim(value: f32) -> Dim {
    Dim::from_ieee32_bits(value.to_bits())
}

fn laid(tex: &str, display: bool) -> Result<MathBox, String> {
    let ast = latex_rust::parse(tex).map_err(|error| error.to_string())?;
    let style = if display { MathStyle::Display } else { MathStyle::Text };
    latex_rust::layout(&ast, font()?, style).map_err(|error| error.to_string())
}

fn metrics_of(tree: &MathBox, size: f32) -> Metrics {
    Metrics {
        width: points(&tree.width) * size,
        ascent: points(&tree.height) * size,
        descent: points(&tree.depth) * size,
    }
}

pub fn check(tex: &str) -> Result<(), String> {
    laid(tex, true).map(drop)
}

pub fn measure(tex: &str, display: bool, size: f32) -> Result<Metrics, String> {
    laid(tex, display).map(|tree| metrics_of(&tree, size))
}

pub fn pieces(tex: &str, display: bool, size: f32) -> Result<Vec<Piece>, String> {
    let tree = laid(tex, display)?;
    let mut out = Vec::new();
    let mut stack = vec![(&tree, 0.0_f32, 0.0_f32)];
    while let Some((node, left, parent_raise)) = stack.pop() {
        let raise = parent_raise + points(&node.shift) * size;
        let piece = |ink| Piece {
            ink,
            left,
            raise,
            width: points(&node.width) * size,
            ascent: points(&node.height) * size,
            descent: points(&node.depth) * size,
        };
        match &node.content {
            BoxContent::Glyph { ch, .. } => out.push(piece(Ink::Glyph(*ch))),
            BoxContent::Rule => out.push(piece(Ink::Rule)),
            BoxContent::Line { .. } => out.push(piece(Ink::Line)),
            BoxContent::HList(children) => {
                let mut x = left;
                for child in children {
                    stack.push((child, x, raise));
                    x += points(&child.width) * size;
                }
            }
            BoxContent::VList(children) => {
                let mut below = 0.0;
                for (index, child) in children.iter().enumerate() {
                    if index == 0 {
                        stack.push((child, left, raise));
                        below = points(&child.depth) * size;
                        continue;
                    }
                    let ascent = points(&child.height) * size;
                    stack.push((child, left, raise - below - ascent));
                    below += ascent + points(&child.depth) * size;
                }
            }
            BoxContent::Overlap(children) => stack.extend(children.iter().map(|child| (child, left, raise))),
            BoxContent::Color(_, inner) | BoxContent::BackColor(_, inner) => stack.push((inner, left, raise)),
            BoxContent::Frame { inner, .. } => {
                out.push(piece(Ink::Frame));
                stack.push((inner, left, raise));
            }
            _ => {}
        }
    }
    out.sort_by(|a, b| a.left.total_cmp(&b.left).then(b.raise.total_cmp(&a.raise)));
    Ok(out)
}

pub fn typeset(tex: &str, display: bool, size: f32, scale: f32, colour: [u8; 3]) -> Result<Formula, String> {
    let tree = laid(tex, display)?;
    let metrics = metrics_of(&tree, size);
    let mut options = SvgOptions::new();
    options.font_size_pt = dim(size * SVG_POINTS_PER_PIXEL);
    options.color = Color::rgb(colour[0], colour[1], colour[2]);
    options.display = display;
    let svg = latex_rust::render_svg(&tree, font()?, &options).map_err(|error| error.to_string())?;
    let picture = zen_image::render_svg(svg.as_bytes(), scale).map_err(|error| error.to_string())?;
    Ok(Formula { metrics, picture })
}
