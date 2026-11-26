use std::collections::BTreeMap;
use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;

use super::Group;

const PAGE_WIDTH: f32 = 595.0;
const PAGE_HEIGHT: f32 = 842.0;
const MARGIN: f32 = 36.0;
const GUTTER: f32 = 18.0;
const TITLE: f32 = 15.0;
const HEADING: f32 = 10.0;
const ROW: f32 = 8.0;
const ROW_HEIGHT: f32 = 11.0;
const HEADING_HEIGHT: f32 = 18.0;

struct Font<'a> {
    face: ttf_parser::Face<'a>,
    used: BTreeMap<u16, char>,
}

impl Font<'_> {
    fn scale(&self) -> f32 {
        1000.0 / f32::from(self.face.units_per_em())
    }

    fn glyph(&self, letter: char) -> u16 {
        self.face.glyph_index(letter).map_or(0, |glyph| glyph.0)
    }

    fn advance(&self, glyph: u16) -> f32 {
        f32::from(self.face.glyph_hor_advance(ttf_parser::GlyphId(glyph)).unwrap_or(0)) * self.scale()
    }

    fn width(&self, text: &str, size: f32) -> f32 {
        text.chars().map(|letter| self.advance(self.glyph(letter))).sum::<f32>() * size / 1000.0
    }

    fn fitted(&self, text: &str, size: f32, width: f32) -> String {
        if self.width(text, size) <= width {
            return text.to_owned();
        }
        let mut letters: Vec<char> = text.chars().collect();
        while !letters.is_empty() {
            letters.pop();
            let shorter: String = letters.iter().chain(['…'].iter()).collect();
            if self.width(&shorter, size) <= width {
                return shorter;
            }
        }
        String::new()
    }

    fn encoded(&mut self, text: &str) -> String {
        let mut hex = String::with_capacity(text.len() * 4);
        for letter in text.chars() {
            let glyph = self.glyph(letter);
            self.used.entry(glyph).or_insert(letter);
            hex.push_str(&format!("{glyph:04X}"));
        }
        hex
    }
}

fn page_text(content: &mut String, font: &mut Font<'_>, text: &str, size: f32, (x, y): (f32, f32)) {
    let hex = font.encoded(text);
    content.push_str(&format!("BT /F1 {size} Tf {x:.2} {y:.2} Td <{hex}> Tj ET\n"));
}

fn pages(title: &str, groups: &[Group], font: &mut Font<'_>) -> Vec<String> {
    let column = (PAGE_WIDTH - 2.0 * MARGIN - GUTTER) / 2.0;
    let top = PAGE_HEIGHT - MARGIN;
    let mut pages = Vec::new();
    let mut content = String::new();
    page_text(&mut content, font, title, TITLE, (MARGIN, top - TITLE));
    let (mut left, mut y) = (MARGIN, top - TITLE - 2.0 * ROW_HEIGHT);
    let mut second = false;
    let mut next_column = |content: &mut String, left: &mut f32, y: &mut f32| {
        if second {
            pages.push(std::mem::take(content));
            *left = MARGIN;
        } else {
            *left = MARGIN + column + GUTTER;
        }
        second = !second;
        *y = top;
    };
    for group in groups {
        if y - HEADING_HEIGHT - ROW_HEIGHT < MARGIN {
            next_column(&mut content, &mut left, &mut y);
        }
        y -= HEADING_HEIGHT;
        page_text(&mut content, font, &group.title, HEADING, (left, y));
        y -= 3.0;
        for row in &group.rows {
            if y - ROW_HEIGHT < MARGIN {
                next_column(&mut content, &mut left, &mut y);
            }
            y -= ROW_HEIGHT;
            let keys = font.width(&row.keys, ROW);
            let name = font.fitted(&row.title, ROW, column - keys - 8.0);
            page_text(&mut content, font, &name, ROW, (left, y));
            page_text(&mut content, font, &row.keys, ROW, (left + column - keys, y));
        }
    }
    if !content.is_empty() {
        pages.push(content);
    }
    pages
}

fn compressed(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).map_err(|error| error.to_string())?;
    encoder.finish().map_err(|error| error.to_string())
}

#[derive(Default)]
struct Writer {
    bytes: Vec<u8>,
    offsets: Vec<usize>,
}

impl Writer {
    fn object(&mut self, id: usize, body: &str) {
        self.opened(id);
        self.bytes.extend_from_slice(body.as_bytes());
        self.bytes.extend_from_slice(b"\nendobj\n");
    }

    fn opened(&mut self, id: usize) {
        if self.offsets.len() < id {
            self.offsets.resize(id, 0);
        }
        self.offsets[id - 1] = self.bytes.len();
        self.bytes.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
    }

    fn stream(&mut self, id: usize, extra: &str, data: &[u8]) -> Result<(), String> {
        let packed = compressed(data)?;
        self.opened(id);
        let head = format!("<< /Length {} /Filter /FlateDecode {extra} >>\nstream\n", packed.len());
        self.bytes.extend_from_slice(head.as_bytes());
        self.bytes.extend_from_slice(&packed);
        self.bytes.extend_from_slice(b"\nendstream\nendobj\n");
        Ok(())
    }

    fn finish(mut self) -> Vec<u8> {
        let table = self.bytes.len();
        let mut tail = format!("xref\n0 {}\n0000000000 65535 f \n", self.offsets.len() + 1);
        for offset in &self.offsets {
            tail.push_str(&format!("{offset:010} 00000 n \n"));
        }
        tail.push_str(&format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{table}\n%%EOF\n",
            self.offsets.len() + 1
        ));
        self.bytes.extend_from_slice(tail.as_bytes());
        self.bytes
    }
}

fn to_unicode(used: &BTreeMap<u16, char>) -> String {
    let mut map = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CMapName /Zen def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let rows: Vec<(&u16, &char)> = used.iter().collect();
    for chunk in rows.chunks(100) {
        map.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (glyph, letter) in chunk {
            let mut units = [0u16; 2];
            let hex: String = letter.encode_utf16(&mut units).iter().map(|unit| format!("{unit:04X}")).collect();
            map.push_str(&format!("<{glyph:04X}> <{hex}>\n"));
        }
        map.push_str("endbfchar\n");
    }
    map.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    map
}

pub(super) fn document(title: &str, groups: &[Group]) -> Result<Vec<u8>, String> {
    let face = ttf_parser::Face::parse(crate::fonts::INTER_REGULAR, 0).map_err(|error| error.to_string())?;
    let mut font = Font { face, used: BTreeMap::new() };
    let pages = pages(title, groups, &mut font);

    let mut writer = Writer { bytes: b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n".to_vec(), offsets: Vec::new() };
    const FIRST_PAGE: usize = 8;
    let kids: Vec<String> = (0..pages.len()).map(|page| format!("{} 0 R", FIRST_PAGE + page * 2)).collect();
    writer.object(1, "<< /Type /Catalog /Pages 2 0 R >>");
    writer.object(2, &format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages.len()));
    writer.object(
        3,
        "<< /Type /Font /Subtype /Type0 /BaseFont /Inter /Encoding /Identity-H /DescendantFonts [4 0 R] \
         /ToUnicode 7 0 R >>",
    );
    let widths: Vec<String> = font.used.keys().map(|glyph| format!("{glyph} [{:.0}]", font.advance(*glyph))).collect();
    writer.object(
        4,
        &format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Inter /CIDSystemInfo << /Registry (Adobe) /Ordering \
             (Identity) /Supplement 0 >> /FontDescriptor 5 0 R /CIDToGIDMap /Identity /DW 600 /W [{}] >>",
            widths.join(" ")
        ),
    );
    let scale = font.scale();
    let edges = font.face.global_bounding_box();
    let scaled = |value: i16| (f32::from(value) * scale).round();
    writer.object(
        5,
        &format!(
            "<< /Type /FontDescriptor /FontName /Inter /Flags 4 /FontBBox [{} {} {} {}] /ItalicAngle 0 /Ascent {} \
             /Descent {} /CapHeight {} /StemV 80 /FontFile2 6 0 R >>",
            scaled(edges.x_min),
            scaled(edges.y_min),
            scaled(edges.x_max),
            scaled(edges.y_max),
            scaled(font.face.ascender()),
            scaled(font.face.descender()),
            scaled(font.face.capital_height().unwrap_or(0)),
        ),
    );
    let program = crate::fonts::INTER_REGULAR;
    writer.stream(6, &format!("/Length1 {}", program.len()), program)?;
    writer.stream(7, "", to_unicode(&font.used).as_bytes())?;
    for (index, content) in pages.iter().enumerate() {
        let page = FIRST_PAGE + index * 2;
        writer.object(
            page,
            &format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] /Resources << /Font << /F1 3 \
                 0 R >> >> /Contents {} 0 R >>",
                page + 1
            ),
        );
        writer.stream(page + 1, "", content.as_bytes())?;
    }
    Ok(writer.finish())
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::super::Row;
    use super::*;

    fn group(rows: usize) -> Group {
        let row = |index| Row { title: format!("Действие {index}"), keys: "⌃⌥⇧⌘A".to_owned() };
        Group { title: "Навигация".to_owned(), rows: (0..rows).map(row).collect() }
    }

    #[test]
    fn the_font_has_the_keys_of_every_platform_and_cyrillic() {
        let face = ttf_parser::Face::parse(crate::fonts::INTER_REGULAR, 0).unwrap();
        for letter in "⌘⌥⌃⇧⏎⌫⎋⇥←→↑↓Яя".chars() {
            assert!(face.glyph_index(letter).is_some(), "no {letter}");
        }
    }

    #[test]
    fn every_object_is_where_the_table_says_and_long_lists_take_more_pages() {
        let bytes = document("Сочетания клавиш", &[group(300)]).unwrap();
        assert!(bytes.starts_with(b"%PDF-1.5"));
        assert!(bytes.ends_with(b"%%EOF\n"));
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.matches("/Type /Page ").count() >= 2, "300 rows do not fit two columns");
        let table = bytes.windows(6).rposition(|window| window == b"\nxref\n").unwrap();
        let listed = String::from_utf8_lossy(&bytes[table + 1..]).into_owned();
        let rows: Vec<&str> = listed.lines().skip(3).take_while(|line| line.ends_with("00000 n ")).collect();
        assert!(!rows.is_empty());
        for (index, row) in rows.iter().enumerate() {
            let offset: usize = row[..10].parse().unwrap();
            assert!(bytes[offset..].starts_with(format!("{} 0 obj", index + 1).as_bytes()), "object {}", index + 1);
        }
    }

    #[test]
    fn the_text_is_written_with_the_fonts_glyphs_and_maps_back_to_unicode() {
        let face = ttf_parser::Face::parse(crate::fonts::INTER_REGULAR, 0).unwrap();
        let mut font = Font { face, used: BTreeMap::new() };
        let pages = pages("Keys", &[group(2)], &mut font);
        assert_eq!(pages.len(), 1);
        let command = font.glyph('⌘');
        assert!(pages[0].contains(&format!("{command:04X}")), "{}", pages[0]);
        assert!(to_unicode(&font.used).contains(&format!("<{command:04X}> <2318>")));
        let packed = compressed(pages[0].as_bytes()).unwrap();
        let mut unpacked = String::new();
        flate2::read::ZlibDecoder::new(packed.as_slice()).read_to_string(&mut unpacked).unwrap();
        assert_eq!(unpacked, pages[0]);
    }

    #[test]
    fn a_long_name_is_cut_and_never_reaches_the_keys() {
        let face = ttf_parser::Face::parse(crate::fonts::INTER_REGULAR, 0).unwrap();
        let font = Font { face, used: BTreeMap::new() };
        let cut = font.fitted(&"long ".repeat(100), ROW, 100.0);
        assert!(cut.ends_with('…'));
        assert!(font.width(&cut, ROW) <= 100.0);
    }
}
