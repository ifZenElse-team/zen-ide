use std::f32::consts::TAU;
use std::io::Write;

pub(crate) const TOOLBOX: ([u8; 3], [u8; 3]) = ([0x8F, 0xB0, 0xFA], [0x3B, 0x54, 0xD6]);

pub(crate) fn product_colors(id: &str) -> ([u8; 3], [u8; 3]) {
    match id {
        "rustronin" => ([0xFF, 0x9A, 0x5A], [0xD9, 0x3A, 0x2B]),
        "gogarden" => ([0x5E, 0xE7, 0xD0], [0x0E, 0x8F, 0xB0]),
        "pypetal" => ([0xFF, 0xB8, 0xD0], [0xD9, 0x4F, 0x8A]),
        "webwave" => ([0x6F, 0xB6, 0xFF], [0x25, 0x5F, 0xDB]),
        "jvmjizo" => ([0xB8, 0xC4, 0xA6], [0x5E, 0x6E, 0x58]),
        "sqlscroll" => ([0x6F, 0xE0, 0xC8], [0x1C, 0x7F, 0x86]),
        "devopsdaruma" => ([0xFF, 0x9E, 0x8E], [0xC7, 0x3B, 0x3B]),
        _ => TOOLBOX,
    }
}

fn render(size: u32, (from, to): ([u8; 3], [u8; 3])) -> Vec<u8> {
    let s = size as f32;
    let (inset, tile) = (s * 0.1, s * 0.8);
    let radius = tile * 0.225;
    let center = s / 2.0;
    let ring_radius = tile * 0.25;
    let ring_half = tile * 0.045;
    let (gap_start, gap_end) = (-0.34 * TAU, -0.22 * TAU);
    let tail = (center + ring_radius * (0.66 * TAU).cos(), center + ring_radius * (0.66 * TAU).sin());

    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let (qx, qy) = ((px - center).abs() - (tile / 2.0 - radius), (py - center).abs() - (tile / 2.0 - radius));
            let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - radius;
            let tile_alpha = (0.5 - outside).clamp(0.0, 1.0);
            if tile_alpha == 0.0 {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let t = (((px - inset) + (py - inset)) / (2.0 * tile)).clamp(0.0, 1.0);
            let mut color = [0f32; 3];
            for (channel, value) in color.iter_mut().enumerate() {
                *value = from[channel] as f32 + (to[channel] as f32 - from[channel] as f32) * t;
            }
            let gloss = (1.0 - ((py - inset) / tile * 1.8)).clamp(0.0, 1.0) * 0.18;
            for value in &mut color {
                *value += (255.0 - *value) * gloss;
            }
            let (dx, dy) = (px - center, py - center);
            let distance = (dx * dx + dy * dy).sqrt();
            let angle = dy.atan2(dx);
            let in_gap = angle > gap_start && angle < gap_end;
            let mut ink = if in_gap { 0.0 } else { (ring_half + 0.5 - (distance - ring_radius).abs()).clamp(0.0, 1.0) };
            let tail_distance = ((px - tail.0).powi(2) + (py - tail.1).powi(2)).sqrt();
            ink = ink.max((ring_half * 1.4 + 0.5 - tail_distance).clamp(0.0, 1.0));
            for value in &mut color {
                *value += (255.0 - *value) * ink;
            }
            rgba.extend_from_slice(&[color[0] as u8, color[1] as u8, color[2] as u8, (tile_alpha * 255.0) as u8]);
        }
    }
    rgba
}

fn png(size: u32, rgba: &[u8]) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    let mut raw = Vec::with_capacity(rgba.len() + size as usize);
    for row in rgba.chunks(size as usize * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(&raw).expect("writing to memory");
    let compressed = encoder.finish().expect("writing to memory");

    let mut header = Vec::new();
    header.extend_from_slice(&size.to_be_bytes());
    header.extend_from_slice(&size.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &compressed);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn render_svg(tree: &resvg::usvg::Tree, size: u32) -> Result<Vec<u8>, String> {
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).ok_or("cannot allocate the icon")?;
    let tile = size as f32 * 0.8;
    let scale = tile / tree.size().width().max(tree.size().height());
    let inset = size as f32 * 0.1;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale).post_translate(inset, inset);
    resvg::render(tree, transform, &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|error| error.to_string())
}

pub(crate) fn product_png(id: &str, size: u32) -> Result<Vec<u8>, String> {
    match std::fs::read(format!("assets/icons/products/{id}.svg")) {
        Ok(svg) => {
            let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default())
                .map_err(|error| format!("invalid icon SVG: {error}"))?;
            render_svg(&tree, size)
        }
        Err(_) => Ok(png(size, &render(size, product_colors(id)))),
    }
}

pub(crate) fn write_iconset_from_svg(directory: &std::path::Path, svg: &[u8]) -> Result<(), String> {
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default())
        .map_err(|error| format!("invalid icon SVG: {error}"))?;
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    for base in [16u32, 32, 128, 256, 512] {
        for scale in [1u32, 2] {
            let size = base * scale;
            let name =
                if scale == 1 { format!("icon_{base}x{base}.png") } else { format!("icon_{base}x{base}@2x.png") };
            std::fs::write(directory.join(name), render_svg(&tree, size)?).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

pub(crate) fn write_iconset(directory: &std::path::Path, colors: ([u8; 3], [u8; 3])) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    for base in [16u32, 32, 128, 256, 512] {
        for scale in [1u32, 2] {
            let size = base * scale;
            let name =
                if scale == 1 { format!("icon_{base}x{base}.png") } else { format!("icon_{base}x{base}@2x.png") };
            std::fs::write(directory.join(name), png(size, &render(size, colors)))
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_svgs_render() {
        for entry in std::fs::read_dir("../assets/icons/products").unwrap().flatten() {
            let svg = std::fs::read(entry.path()).unwrap();
            let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default()).unwrap();
            let png = render_svg(&tree, 64).unwrap();
            assert!(png.starts_with(b"\x89PNG"), "{}", entry.path().display());
        }
    }

    #[test]
    fn crc_of_iend_matches_the_png_specification() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }

    #[test]
    fn the_icon_is_transparent_at_the_corners_and_opaque_in_the_middle() {
        let size = 64;
        let rgba = render(size, TOOLBOX);
        assert_eq!(rgba[3], 0, "corner");
        let middle = ((size / 2) * size + size / 4) as usize * 4;
        assert_eq!(rgba[middle + 3], 255, "inside the tile");
    }
}

pub(crate) fn clean_tile_svg(raw: &str) -> Result<String, String> {
    let tile = &raw[raw.find(r#"<g id="Tile""#).ok_or("no \"Tile\" group in the export")?..];
    let rect_start = tile.find("<rect ").ok_or("no tile rectangle")?;
    let rect_end = rect_start + tile[rect_start..].find("/>").ok_or("unterminated tile rectangle")? + 2;
    let rect = &tile[rect_start..rect_end];
    let open_at = tile.find(r#"<g id="Zen Icon""#).ok_or("no \"Zen Icon\" group in the tile")?;
    let icon = &tile[open_at + tile[open_at..].find('>').ok_or("unterminated \"Zen Icon\" group")? + 1..];
    let mut depth = 1;
    let mut end = None;
    let mut index = 0;
    while let Some(offset) = icon[index..]
        .find("<g")
        .map(|o| (o, true))
        .into_iter()
        .chain(icon[index..].find("</g>").map(|o| (o, false)))
        .min_by_key(|(o, _)| *o)
    {
        let (position, opens) = offset;
        if opens {
            depth += 1;
        } else {
            depth -= 1;
            if depth == 0 {
                end = Some(index + position);
                break;
            }
        }
        index += position + 2;
    }
    let body = &icon[..end.ok_or("unterminated \"Zen Icon\" group")?];
    let body = strip_ids(body.trim());
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"104\" height=\"104\" viewBox=\"0 0 104 104\" fill=\"none\">\n{rect}\n{body}\n</svg>\n"
    ))
}

fn strip_ids(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(" id=\"") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 5..];
        rest = after.find('"').map_or("", |end| &after[end + 1..]);
    }
    out.push_str(rest);
    out
}

pub(crate) fn import(args: &[String]) -> Result<(), String> {
    let [source, id] = args else {
        return Err("usage: cargo xtask icon-import <figma-export.svg> <product-id>".to_owned());
    };
    let raw = std::fs::read_to_string(source).map_err(|error| format!("cannot read {source}: {error}"))?;
    let clean = clean_tile_svg(&raw)?;
    resvg::usvg::Tree::from_data(clean.as_bytes(), &resvg::usvg::Options::default())
        .map_err(|error| format!("the result is not valid SVG: {error}"))?;
    let target = std::path::PathBuf::from("assets/icons/products").join(format!("{id}.svg"));
    std::fs::write(&target, clean).map_err(|error| error.to_string())?;
    println!("Wrote {}.", target.display());
    Ok(())
}

#[cfg(test)]
mod import_tests {
    use super::*;

    #[test]
    fn accepts_groups_with_attributes() {
        let raw = r##"<svg><g id="Tile" clip-path="url(#a)"><rect width="104" height="104" rx="24" fill="#84BCED"/><g id="Zen Icon" clip-path="url(#b)"><path d="M1 1Z"/></g></g></svg>"##;
        let clean = clean_tile_svg(raw).unwrap();
        assert!(clean.contains(r#"<path d="M1 1Z"/>"#) && clean.contains("#84BCED"));
    }

    #[test]
    fn keeps_only_the_tile_and_the_glyph() {
        let raw = r##"<svg><rect width="104" height="104" fill="#1E1E1E"/><g id="Card"><rect fill="#111"/><g id="Tile"><rect width="104" height="104" rx="24" fill="#5E726D"/><g id="Zen Icon"><g id="Inner"><path id="Mark" d="M1 1Z" fill="#FFF"/></g><path d="M2 2Z"/></g></g></g></svg>"##;
        let clean = clean_tile_svg(raw).unwrap();
        assert!(clean.contains(r##"<rect width="104" height="104" rx="24" fill="#5E726D"/>"##));
        assert!(clean.contains(r##"<path d="M1 1Z" fill="#FFF"/>"##));
        assert!(clean.contains(r#"<path d="M2 2Z"/>"#));
        assert!(!clean.contains("#1E1E1E") && !clean.contains("#111") && !clean.contains("id="));
    }
}
