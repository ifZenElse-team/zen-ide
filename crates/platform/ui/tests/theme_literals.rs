use std::path::Path;

const EXEMPT_FILES: &[(&str, &str)] = &[
    ("ide/splash.rs", "the loader's artwork, the same in every theme"),
    ("ide/palette_pin.rs", "test code: the colours from before themes, pinned as literals"),
];

const ALLOWED_EVERYWHERE: &[(&str, &str)] = &[("Color32::TRANSPARENT", "no fill: the absence of a colour")];

const ALLOWED: &[(&str, &str, usize, &str)] = &[
    (
        "ide/inlay_hints.rs",
        "fonts.layout_no_wrap(display, font.clone(), Color32::WHITE)",
        1,
        "layout measurement: the galley is measured, never drawn",
    ),
    (
        "ide/inlay_hints.rs",
        "font_id: font.clone(), color: Color32::WHITE,",
        1,
        "layout measurement: the galley is measured, never drawn",
    ),
    (
        "ide/inlay_hints.rs",
        "font.clone(), Color32::WHITE).size().x",
        1,
        "layout measurement: the label is measured, never drawn",
    ),
    (
        "ide/color_swatches.rs",
        "Color32::from_rgba_unmultiplied(r, g, b, a)",
        1,
        "a colour preview: the colour the file's literal stands for",
    ),
    ("ide/image_view.rs", "board_uv, Color32::WHITE)", 1, "image tint: white draws the chessboard unchanged"),
    ("ide/image_view.rs", "picture_rect, uv, Color32::WHITE)", 1, "image tint: white draws the picture unchanged"),
    (
        "ide/markdown_view.rs",
        "rect, uv, Color32::WHITE);",
        1,
        "image tint: white draws the Markdown's picture unchanged",
    ),
    (
        "ide/markdown_math.rs",
        "rect, uv, Color32::WHITE);",
        1,
        "image tint: white draws the formula, typeset in the theme's text colour, unchanged",
    ),
    (
        "ide/git_workspace/avatars.rs",
        "rect, uv, Color32::WHITE)",
        1,
        "image tint: white draws the author's picture unchanged",
    ),
    (
        "ide/todo_marks.rs",
        "Some(Color32::from_rgb(red, green, blue))",
        1,
        "a TODO pattern's own colour, the person's setting",
    ),
    (
        "ide/mod.rs",
        "Color32::from_rgb(mix(p.frame.r(), color.r())",
        1,
        "computed: the toolbar glow mixes the window and the project colours",
    ),
    (
        "ide/caffeine.rs",
        "Color32::from_rgb(0xE0, 0x24, 0x2E)",
        1,
        "the bloodshot eyes of Caffeine's drinker: one fixed red, as the card asks (9cbsCgjA)",
    ),
    (
        "ide/caffeine.rs",
        "Color32::from_rgb(deeper(dark.r())",
        1,
        "computed: the theme's darkest colour, darker, for the pixel art's outline",
    ),
    ("terminal.rs", "Some((r, g, b)) => Color32::from_rgb(r, g, b)", 1, "the program's colour from the xterm cube"),
    ("terminal.rs", "Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b)", 1, "the program's own 24-bit colour"),
    (
        "terminal.rs",
        "Color32::from_rgb(mix(color.r(), background.r())",
        1,
        "computed: dim text halfway to the background",
    ),
];

fn constructors(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = line;
    while let Some(at) = rest.find("Color32::") {
        let tail = &rest[at + "Color32::".len()..];
        let name: String = tail.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if name.starts_with("from_") || (!name.is_empty() && name.chars().all(|c| c.is_ascii_uppercase() || c == '_')) {
            found.push(format!("Color32::{name}"));
        }
        rest = &tail[name.len()..];
    }
    found
}

fn sources(dir: &Path, root: &Path, files: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, root, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
        }
    }
}

#[test]
fn ide_colours_come_from_the_theme() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec!["terminal.rs".to_owned()];
    sources(&root.join("ide"), &root, &mut files);
    let mut problems = Vec::new();
    let mut used = vec![0; ALLOWED.len()];
    for file in &files {
        if EXEMPT_FILES.iter().any(|(exempt, _)| exempt == file) {
            continue;
        }
        let text = std::fs::read_to_string(root.join(file)).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let tests = lines
            .windows(2)
            .position(|pair| pair[0].starts_with("#[cfg(") && pair[0].contains("test") && pair[1].ends_with('{'))
            .unwrap_or(lines.len());
        for (number, line) in lines[..tests].iter().enumerate() {
            for constructor in constructors(line) {
                if ALLOWED_EVERYWHERE.iter().any(|(allowed, _)| *allowed == constructor) {
                    continue;
                }
                match ALLOWED.iter().position(|(allowed, snippet, _, _)| allowed == file && line.contains(snippet)) {
                    Some(index) => used[index] += 1,
                    None => problems.push(format!("src/{file}:{}: {constructor} in `{}`", number + 1, line.trim())),
                }
            }
        }
    }
    for ((file, snippet, count, _), seen) in ALLOWED.iter().zip(&used) {
        if seen != count {
            problems.push(format!("src/{file}: `{snippet}` is allowed {count} times but appears {seen} times"));
        }
    }
    for (file, _) in EXEMPT_FILES {
        assert!(files.contains(&(*file).to_owned()), "the exempt file src/{file} no longer exists");
    }
    assert!(problems.is_empty(), "colours that do not come from the theme:\n{}", problems.join("\n"));
}

#[test]
fn the_scan_finds_constructors() {
    assert_eq!(
        constructors("x(Color32::from_rgb(1, 2, 3), Color32::WHITE, Color32::from_black_alpha(9), c.r())"),
        ["Color32::from_rgb", "Color32::WHITE", "Color32::from_black_alpha"]
    );
    assert!(constructors("let c: Color32 = p.text; Color32::TRANSPARENT").contains(&"Color32::TRANSPARENT".to_owned()));
}
