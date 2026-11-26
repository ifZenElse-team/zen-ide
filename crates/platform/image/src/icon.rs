pub const MAX_BYTES: usize = 4096;

pub const MAX_DEPTH: usize = 16;

const ELEMENTS: &[&str] = &["svg", "g", "path", "circle", "ellipse", "rect", "line", "polyline", "polygon"];

const ATTRIBUTES: &[&str] = &[
    "xmlns",
    "viewBox",
    "width",
    "height",
    "d",
    "x",
    "y",
    "x1",
    "y1",
    "x2",
    "y2",
    "cx",
    "cy",
    "r",
    "rx",
    "ry",
    "points",
    "transform",
    "fill",
    "fill-rule",
    "fill-opacity",
    "clip-rule",
    "stroke",
    "stroke-width",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-opacity",
    "opacity",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconProblem {
    NotSvg,
    TooBig,
    TooDeep,
    Element,
    Attribute,
    Reference,
    NoViewBox,
    Unreadable,
}

fn name_end(bytes: &[u8], from: usize) -> usize {
    let mut at = from;
    while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || matches!(bytes[at], b'-' | b':' | b'_')) {
        at += 1;
    }
    at
}

fn skip_space(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() && bytes[at].is_ascii_whitespace() {
        at += 1;
    }
    at
}

pub fn problem(svg: &str) -> Option<IconProblem> {
    if svg.len() > MAX_BYTES {
        return Some(IconProblem::TooBig);
    }
    let text = svg.trim();
    if !text.starts_with("<svg") {
        return Some(IconProblem::NotSvg);
    }
    let bytes = text.as_bytes();
    let (mut at, mut depth, mut view_box, mut elements) = (0, 0usize, false, 0usize);
    while at < bytes.len() {
        if bytes[at] != b'<' {
            if bytes[at] == b'&' {
                return Some(IconProblem::Reference);
            }
            if !bytes[at].is_ascii_whitespace() {
                return Some(IconProblem::Element);
            }
            at += 1;
            continue;
        }
        at += 1;
        if matches!(bytes.get(at), Some(b'!' | b'?')) {
            return Some(IconProblem::Element);
        }
        if bytes.get(at) == Some(&b'/') {
            let end = name_end(bytes, at + 1);
            at = skip_space(bytes, end);
            if bytes.get(at) != Some(&b'>') || depth == 0 {
                return Some(IconProblem::Unreadable);
            }
            depth -= 1;
            at += 1;
            continue;
        }
        let end = name_end(bytes, at);
        let name = &text[at..end];
        if !ELEMENTS.contains(&name) || (elements == 0) != (name == "svg") {
            return Some(IconProblem::Element);
        }
        elements += 1;
        at = end;
        loop {
            at = skip_space(bytes, at);
            match bytes.get(at) {
                Some(b'>') => {
                    depth += 1;
                    at += 1;
                    break;
                }
                Some(b'/') if bytes.get(at + 1) == Some(&b'>') => {
                    at += 2;
                    break;
                }
                Some(_) => {}
                None => return Some(IconProblem::Unreadable),
            }
            let end = name_end(bytes, at);
            let attribute = &text[at..end];
            if !ATTRIBUTES.contains(&attribute) {
                return Some(IconProblem::Attribute);
            }
            at = skip_space(bytes, end);
            if bytes.get(at) != Some(&b'=') {
                return Some(IconProblem::Unreadable);
            }
            at = skip_space(bytes, at + 1);
            let Some(&quote) = bytes.get(at).filter(|quote| matches!(quote, b'"' | b'\'')) else {
                return Some(IconProblem::Unreadable);
            };
            let Some(length) = text[at + 1..].find(quote as char) else { return Some(IconProblem::Unreadable) };
            let value = &text[at + 1..at + 1 + length];
            if value.contains(['&', '<']) || value.to_ascii_lowercase().contains("url(") {
                return Some(IconProblem::Reference);
            }
            if name == "svg" && attribute == "viewBox" {
                view_box = true;
            }
            at += length + 2;
        }
        if depth > MAX_DEPTH {
            return Some(IconProblem::TooDeep);
        }
    }
    if depth != 0 {
        return Some(IconProblem::Unreadable);
    }
    (!view_box).then_some(IconProblem::NoViewBox)
}

fn options() -> resvg::usvg::Options<'static> {
    resvg::usvg::Options {
        resources_dir: None,
        fontdb: std::sync::Arc::new(resvg::usvg::fontdb::Database::new()),
        image_href_resolver: resvg::usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        style_sheet: None,
        ..resvg::usvg::Options::default()
    }
}

pub fn mask(svg: &str, side: u32) -> Option<Vec<u8>> {
    if problem(svg).is_some() || side == 0 || side > 256 {
        return None;
    }
    let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &options()).ok()?;
    let size = tree.size();
    let scale = side as f32 / size.width().max(size.height());
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let (dx, dy) = ((side as f32 - size.width() * scale) / 2.0, (side as f32 - size.height() * scale) / 2.0);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(side, side)?;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale).post_translate(dx, dy);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Some(pixmap.pixels().iter().map(|pixel| pixel.alpha()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOXES: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="black" stroke-width="1.2"><rect x="2" y="8" width="5" height="5"/><rect x="9" y="8" width="5" height="5"/><rect x="5.5" y="2.5" width="5" height="5"/></g></svg>"#;

    #[test]
    fn an_icon_of_plain_shapes_is_accepted_and_drawn_as_coverage() {
        assert_eq!(problem(BOXES), None);
        let mask = mask(BOXES, 32).unwrap();
        assert_eq!(mask.len(), 32 * 32);
        assert!(mask.iter().any(|alpha| *alpha > 200), "the outlines are drawn");
        assert!(mask.iter().filter(|alpha| **alpha == 0).count() > 32 * 32 / 2, "the inside stays empty");
    }

    #[test]
    fn anything_but_plain_shapes_is_refused() {
        let cases = [
            (r#"<svg viewBox="0 0 1 1"><image href="file:///etc/passwd"/></svg>"#, IconProblem::Element),
            (r#"<svg viewBox="0 0 1 1"><path href="x" d="M0 0"/></svg>"#, IconProblem::Attribute),
            (r#"<svg viewBox="0 0 1 1"><path xlink:href="x" d="M0 0"/></svg>"#, IconProblem::Attribute),
            (r#"<svg viewBox="0 0 1 1"><path fill="url(#a)" d="M0 0"/></svg>"#, IconProblem::Reference),
            (r#"<svg viewBox="0 0 1 1"><path style="fill:red" d="M0 0"/></svg>"#, IconProblem::Attribute),
            (r#"<svg viewBox="0 0 1 1"><text>Hi</text></svg>"#, IconProblem::Element),
            (r#"<svg viewBox="0 0 1 1"><script>x</script></svg>"#, IconProblem::Element),
            (r#"<!DOCTYPE svg [<!ENTITY a "b">]><svg viewBox="0 0 1 1"/>"#, IconProblem::NotSvg),
            (r#"<svg viewBox="0 0 1 1"><!ENTITY a "b"></svg>"#, IconProblem::Element),
            (r#"<svg viewBox="0 0 1 1">&a;</svg>"#, IconProblem::Reference),
            (r#"<svg><path d="M0 0"/></svg>"#, IconProblem::NoViewBox),
            (r#"<svg viewBox="0 0 1 1"><g><svg viewBox="0 0 1 1"/></g></svg>"#, IconProblem::Element),
            (r#"<svg viewBox="0 0 1 1"><g>"#, IconProblem::Unreadable),
            ("docker", IconProblem::NotSvg),
        ];
        for (svg, expected) in cases {
            assert_eq!(problem(svg), Some(expected), "{svg}");
            assert!(mask(svg, 16).is_none(), "{svg}");
        }
    }

    #[test]
    fn a_hostile_icon_is_refused_without_reaching_the_renderer() {
        let huge = format!(r#"<svg viewBox="0 0 1 1"><path d="M0 0{}"/></svg>"#, " L1 1".repeat(2000));
        assert_eq!(problem(&huge), Some(IconProblem::TooBig));
        let deep =
            format!(r#"<svg viewBox="0 0 1 1">{}{}</svg>"#, "<g>".repeat(MAX_DEPTH + 1), "</g>".repeat(MAX_DEPTH + 1));
        assert_eq!(problem(&deep), Some(IconProblem::TooDeep));
        let fine =
            format!(r#"<svg viewBox="0 0 1 1">{}{}</svg>"#, "<g>".repeat(MAX_DEPTH - 1), "</g>".repeat(MAX_DEPTH - 1));
        assert_eq!(problem(&fine), None);
        assert!(mask(&huge, 16).is_none() && mask(&deep, 16).is_none());
        assert!(mask(BOXES, 0).is_none() && mask(BOXES, 100_000).is_none());
    }
}
