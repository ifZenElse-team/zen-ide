use egui::{Align2, Color32, FontFamily, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2, vec2};

use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileIcon {
    Go,
    GoModule,
    GoSum,
    Rust,
    CargoManifest,
    CargoLock,
    Python,
    PythonProject,
    PythonRequirements,
    JavaScript,
    NodePackage,
    Jsx,
    TypeScript,
    Tsx,
    Json,
    Yaml,
    Toml,
    Xml,
    Html,
    Css,
    Scss,
    Less,
    Vue,
    Nuxt,
    NextJs,
    Svelte,
    SvelteConfig,
    Markdown,
    Text,
    Log,
    Shell,
    Docker,
    Compose,
    Makefile,
    Kotlin,
    KotlinScript,
    Java,
    C,
    CHeader,
    Cpp,
    CppHeader,
    CSharp,
    Php,
    Ruby,
    Sql,
    Swift,
    Lua,
    Protobuf,
    GraphQl,
    Image,
    VectorImage,
    Archive,
    Pdf,
    Font,
    Lock,
    Git,
    Ignore,
    GitLabCi,
    EditorConfig,
    Env,
    License,
    Fluent,
    WebAssembly,
    Csv,
    Generic,
}

impl FileIcon {
    pub const ALL: [FileIcon; 65] = [
        FileIcon::Go,
        FileIcon::GoModule,
        FileIcon::GoSum,
        FileIcon::Rust,
        FileIcon::CargoManifest,
        FileIcon::CargoLock,
        FileIcon::Python,
        FileIcon::PythonProject,
        FileIcon::PythonRequirements,
        FileIcon::JavaScript,
        FileIcon::NodePackage,
        FileIcon::Jsx,
        FileIcon::TypeScript,
        FileIcon::Tsx,
        FileIcon::Json,
        FileIcon::Yaml,
        FileIcon::Toml,
        FileIcon::Xml,
        FileIcon::Html,
        FileIcon::Css,
        FileIcon::Scss,
        FileIcon::Less,
        FileIcon::Vue,
        FileIcon::Nuxt,
        FileIcon::NextJs,
        FileIcon::Svelte,
        FileIcon::SvelteConfig,
        FileIcon::Markdown,
        FileIcon::Text,
        FileIcon::Log,
        FileIcon::Shell,
        FileIcon::Docker,
        FileIcon::Compose,
        FileIcon::Makefile,
        FileIcon::Kotlin,
        FileIcon::KotlinScript,
        FileIcon::Java,
        FileIcon::C,
        FileIcon::CHeader,
        FileIcon::Cpp,
        FileIcon::CppHeader,
        FileIcon::CSharp,
        FileIcon::Php,
        FileIcon::Ruby,
        FileIcon::Sql,
        FileIcon::Swift,
        FileIcon::Lua,
        FileIcon::Protobuf,
        FileIcon::GraphQl,
        FileIcon::Image,
        FileIcon::VectorImage,
        FileIcon::Archive,
        FileIcon::Pdf,
        FileIcon::Font,
        FileIcon::Lock,
        FileIcon::Git,
        FileIcon::Ignore,
        FileIcon::GitLabCi,
        FileIcon::EditorConfig,
        FileIcon::Env,
        FileIcon::License,
        FileIcon::Fluent,
        FileIcon::WebAssembly,
        FileIcon::Csv,
        FileIcon::Generic,
    ];
}

pub fn classify(file_name: &str) -> FileIcon {
    let lower = file_name.to_ascii_lowercase();
    let name = lower.as_str();

    let special = match name {
        "go.mod" | "go.work" => Some(FileIcon::GoModule),
        "go.sum" | "go.work.sum" => Some(FileIcon::GoSum),
        "cargo.toml" => Some(FileIcon::CargoManifest),
        "cargo.lock" => Some(FileIcon::CargoLock),
        "pyproject.toml" | "setup.py" | "setup.cfg" => Some(FileIcon::PythonProject),
        "package.json" => Some(FileIcon::NodePackage),
        "nuxt.config.ts" | "nuxt.config.js" | "nuxt.config.mjs" => Some(FileIcon::Nuxt),
        "next.config.js" | "next.config.mjs" | "next.config.ts" | "next-env.d.ts" => Some(FileIcon::NextJs),
        "svelte.config.js" | "svelte.config.ts" | "svelte.config.mjs" => Some(FileIcon::SvelteConfig),
        "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml" | "poetry.lock" | "pipfile.lock" | "gemfile.lock"
        | "composer.lock" | "bun.lockb" | "flake.lock" => Some(FileIcon::Lock),
        "dockerfile" | "containerfile" => Some(FileIcon::Docker),
        ".gitlab-ci.yml" | ".gitlab-ci.yaml" => Some(FileIcon::GitLabCi),
        "compose.yml" | "compose.yaml" | "docker-compose.yml" | "docker-compose.yaml" => Some(FileIcon::Compose),
        "makefile" | "gnumakefile" | "justfile" => Some(FileIcon::Makefile),
        "gemfile" | "rakefile" => Some(FileIcon::Ruby),
        ".gitattributes" | ".gitmodules" | ".gitkeep" | ".git-blame-ignore-revs" | ".mailmap" => Some(FileIcon::Git),
        ".editorconfig" => Some(FileIcon::EditorConfig),
        _ => None,
    };
    if let Some(icon) = special {
        return icon;
    }
    if name.starts_with("requirements") && name.ends_with(".txt") {
        return FileIcon::PythonRequirements;
    }
    if name.starts_with('.') && name.ends_with("ignore") {
        return FileIcon::Ignore;
    }
    if name.ends_with(".gitlab-ci.yml") || name.ends_with(".gitlab-ci.yaml") {
        return FileIcon::GitLabCi;
    }
    if name == ".env" || name.starts_with(".env.") || name.ends_with(".env") {
        return FileIcon::Env;
    }
    if name.starts_with("dockerfile.") {
        return FileIcon::Docker;
    }
    if ["license", "licence", "notice", "copying", "unlicense"]
        .iter()
        .any(|stem| name == *stem || name.strip_prefix(stem).is_some_and(|rest| rest.starts_with(['.', '-', '_'])))
    {
        return FileIcon::License;
    }
    if name.ends_with(".d.ts") {
        return FileIcon::TypeScript;
    }

    let Some((_, extension)) = name.rsplit_once('.') else { return FileIcon::Generic };
    match extension {
        "go" => FileIcon::Go,
        "rs" => FileIcon::Rust,
        "py" | "pyi" | "pyw" => FileIcon::Python,
        "js" | "mjs" | "cjs" => FileIcon::JavaScript,
        "jsx" => FileIcon::Jsx,
        "ts" | "mts" | "cts" => FileIcon::TypeScript,
        "tsx" => FileIcon::Tsx,
        "json" | "jsonc" | "json5" => FileIcon::Json,
        "yml" | "yaml" => FileIcon::Yaml,
        "toml" => FileIcon::Toml,
        "xml" | "xsd" | "xsl" | "xslt" | "plist" | "iml" | "wsdl" | "pom" | "csproj" | "fsproj" | "vbproj"
        | "props" | "targets" | "resx" | "xaml" | "rss" | "atom" | "kml" | "gpx" | "opml" | "dtd" => FileIcon::Xml,
        "html" | "htm" | "xhtml" => FileIcon::Html,
        "css" => FileIcon::Css,
        "scss" | "sass" => FileIcon::Scss,
        "less" => FileIcon::Less,
        "vue" => FileIcon::Vue,
        "svelte" => FileIcon::Svelte,
        "md" | "markdown" | "mdx" => FileIcon::Markdown,
        "txt" | "text" => FileIcon::Text,
        "log" => FileIcon::Log,
        "sh" | "bash" | "zsh" | "fish" | "ksh" => FileIcon::Shell,
        "dockerfile" => FileIcon::Docker,
        "mk" | "mak" => FileIcon::Makefile,
        "kt" => FileIcon::Kotlin,
        "kts" => FileIcon::KotlinScript,
        "java" => FileIcon::Java,
        "c" => FileIcon::C,
        "h" => FileIcon::CHeader,
        "cpp" | "cc" | "cxx" | "c++" => FileIcon::Cpp,
        "hpp" | "hh" | "hxx" | "h++" => FileIcon::CppHeader,
        "cs" => FileIcon::CSharp,
        "php" | "phtml" => FileIcon::Php,
        "rb" | "gemspec" | "rake" => FileIcon::Ruby,
        "sql" => FileIcon::Sql,
        "swift" => FileIcon::Swift,
        "lua" => FileIcon::Lua,
        "proto" => FileIcon::Protobuf,
        "graphql" | "gql" | "graphqls" => FileIcon::GraphQl,
        "png" | "jpg" | "jpeg" | "gif" | "ico" | "webp" | "bmp" | "tif" | "tiff" | "avif" | "icns" => FileIcon::Image,
        "svg" => FileIcon::VectorImage,
        "zip" | "tar" | "gz" | "tgz" | "xz" | "bz2" | "zst" | "7z" | "rar" | "jar" | "crate" => FileIcon::Archive,
        "pdf" => FileIcon::Pdf,
        "ttf" | "otf" | "woff" | "woff2" => FileIcon::Font,
        "lock" => FileIcon::Lock,
        "ftl" => FileIcon::Fluent,
        "wasm" | "wat" => FileIcon::WebAssembly,
        "csv" | "tsv" => FileIcon::Csv,
        _ => FileIcon::Generic,
    }
}

pub fn is_test_file(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    let name = lower.as_str();
    let has_stem = |suffix: &str| name.len() > suffix.len() && name.ends_with(suffix);
    if has_stem("_test.go") || has_stem("_test.py") || has_stem("_spec.rb") {
        return true;
    }
    if name.starts_with("test_") && has_stem(".py") {
        return true;
    }
    if ["Test.kt", "Test.java"].iter().any(|suffix| file_name.len() > suffix.len() && file_name.ends_with(suffix)) {
        return true;
    }
    if let Some((stem, extension)) = name.rsplit_once('.')
        && matches!(extension, "ts" | "js" | "tsx" | "jsx" | "mjs" | "cjs" | "mts" | "cts")
    {
        return [".test", ".spec"].iter().any(|kind| stem.len() > kind.len() && stem.ends_with(kind));
    }
    false
}

mod color {
    use egui::Color32;

    const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
        Color32::from_rgb(r, g, b)
    }

    const fn muted(r: u8, g: u8, b: u8) -> Color32 {
        const fn mix(c: u8, grey: u8) -> u8 {
            ((c as u16 * 55 + grey as u16 * 45) / 100 * 92 / 100) as u8
        }
        Color32::from_rgb(mix(r, 0x8C), mix(g, 0x90), mix(b, 0x98))
    }

    pub(super) const INK_DARK: Color32 = rgb(0x1F, 0x20, 0x23);
    pub(super) const WHITE: Color32 = rgb(0xF4, 0xF5, 0xF7);
    pub(super) const PAGE: Color32 = rgb(0xA8, 0xAD, 0xB6);
    pub(super) const PAGE_FILL: Color32 = rgb(0x2B, 0x2D, 0x31);

    pub(super) const GO: Color32 = muted(0x45, 0xB5, 0xD3);
    pub(super) const RUST: Color32 = muted(0xDE, 0x7A, 0x48);
    pub(super) const PY_BLUE: Color32 = muted(0x45, 0x83, 0xD4);
    pub(super) const PY_YELLOW: Color32 = muted(0xF5, 0xD2, 0x5A);
    pub(super) const JS: Color32 = muted(0xE6, 0xC3, 0x4A);
    pub(super) const TS: Color32 = muted(0x3D, 0x80, 0xDE);
    pub(super) const JSON: Color32 = muted(0xE6, 0xA8, 0x4E);
    pub(super) const YAML: Color32 = muted(0xE0, 0x72, 0x80);
    pub(super) const TOML: Color32 = muted(0xC9, 0xA1, 0x6E);
    pub(super) const XML: Color32 = muted(0x5C, 0xBF, 0xA8);
    pub(super) const HTML: Color32 = muted(0xE5, 0x6B, 0x45);
    pub(super) const CSS: Color32 = muted(0x7B, 0x8C, 0xF2);
    pub(super) const SCSS: Color32 = muted(0xD6, 0x6E, 0xAA);
    pub(super) const LESS: Color32 = muted(0x5A, 0x88, 0xC8);
    pub(super) const VUE: Color32 = muted(0x42, 0xB8, 0x83);
    pub(super) const SVELTE: Color32 = muted(0xFF, 0x3E, 0x00);
    pub(super) const NUXT: Color32 = muted(0x00, 0xC5, 0x75);
    pub(super) const NEXT: Color32 = muted(0x5A, 0x5E, 0x66);
    pub(super) const MARKDOWN_MARK: Color32 = rgb(0x54, 0x8A, 0xF7);
    pub(super) const SHELL: Color32 = muted(0x7F, 0xC2, 0x6F);
    pub(super) const DOCKER: Color32 = muted(0x4B, 0x9E, 0xEC);
    pub(super) const MAKE: Color32 = muted(0xCB, 0x9C, 0x5E);
    pub(super) const KOTLIN: Color32 = muted(0x9C, 0x6E, 0xF2);
    pub(super) const JAVA: Color32 = muted(0xE0, 0x5A, 0x4C);
    pub(super) const C: Color32 = muted(0x5C, 0x93, 0xDB);
    pub(super) const CPP: Color32 = muted(0x6A, 0x72, 0xE0);
    pub(super) const CSHARP: Color32 = muted(0x8E, 0x62, 0xD4);
    pub(super) const PHP: Color32 = muted(0x8C, 0x96, 0xD6);
    pub(super) const RUBY: Color32 = muted(0xDE, 0x45, 0x48);
    pub(super) const SQL: Color32 = muted(0x4A, 0xBA, 0xC6);
    pub(super) const SWIFT: Color32 = muted(0xF0, 0x74, 0x3E);
    pub(super) const LUA: Color32 = muted(0x66, 0x78, 0xE8);
    pub(super) const PROTO: Color32 = muted(0x3E, 0xBF, 0x8F);
    pub(super) const GRAPHQL: Color32 = muted(0xE2, 0x5D, 0xB6);
    pub(super) const IMAGE: Color32 = muted(0x5F, 0xBA, 0x8B);
    pub(super) const VECTOR: Color32 = muted(0xE8, 0xA2, 0x4F);
    pub(super) const ARCHIVE: Color32 = muted(0xC6, 0x9F, 0x6A);
    pub(super) const PDF: Color32 = muted(0xE0, 0x50, 0x52);
    pub(super) const FONT: Color32 = muted(0xB0, 0xB8, 0xE0);
    pub(super) const LOCK: Color32 = muted(0xB8, 0xBC, 0xC4);
    pub(super) const GIT: Color32 = muted(0xEC, 0x6B, 0x4C);
    pub(super) const EDITOR_CONFIG: Color32 = muted(0xA0, 0xA6, 0xAE);
    pub(super) const ENV: Color32 = muted(0xD8, 0xC2, 0x4E);
    pub(super) const LICENSE: Color32 = muted(0xDB, 0xB4, 0x4E);
    pub(super) const FLUENT: Color32 = muted(0x5B, 0xB2, 0xE8);
    pub(super) const WASM: Color32 = muted(0x86, 0x6A, 0xF2);
    pub(super) const CSV: Color32 = muted(0x5D, 0xBA, 0x70);

    pub(super) const FOLDER: Color32 = rgb(0x9A, 0xA7, 0xB8);
    pub(super) const FOLDER_SOURCE: Color32 = muted(0x5C, 0x9F, 0xEA);
    pub(super) const FOLDER_TEST: Color32 = TEST;
    pub(super) const TEST: Color32 = muted(0x62, 0xB8, 0x6A);
    pub(super) const FOLDER_DOCS: Color32 = muted(0xB1, 0x8E, 0xE6);
    pub(super) const FOLDER_ASSETS: Color32 = muted(0xD9, 0xB4, 0x4C);
    pub(super) const FOLDER_VCS: Color32 = muted(0xE5, 0x7D, 0x62);
    pub(super) const FOLDER_RESOURCE: Color32 = rgb(0x9C, 0x84, 0xD8);
    pub(super) const FOLDER_EXCLUDED: Color32 = rgb(0xD8, 0x8A, 0x3C);
}

#[derive(Clone, Copy)]
struct Grid {
    rect: Rect,
}

impl Grid {
    fn new(rect: Rect) -> Grid {
        let side = rect.width().min(rect.height());
        let min = (rect.center() - Vec2::splat(side / 2.0)).round();
        Grid { rect: Rect::from_min_size(min, Vec2::splat(side)) }
    }

    fn u(self) -> f32 {
        self.rect.width() / 16.0
    }

    fn p(self, x: f32, y: f32) -> Pos2 {
        self.rect.min + vec2(x, y) * self.u()
    }

    fn r(self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(self.p(x0, y0), self.p(x1, y1))
    }

    fn s(self, width: f32, color: Color32) -> Stroke {
        Stroke::new(width * self.u(), color)
    }
}

fn tint(color: Color32, alpha: f32) -> Color32 {
    color.gamma_multiply(alpha)
}

fn mark_font(painter: &Painter, size: f32) -> FontId {
    let bold = theme::bold(size);
    if painter.fonts(|fonts| fonts.families().contains(&bold.family)) {
        bold
    } else {
        FontId::new(size, FontFamily::Proportional)
    }
}

fn mark(painter: &Painter, g: Grid, center: Pos2, text: &str, size: f32, color: Color32) {
    let at = center + vec2(0.0, 0.04 * size * g.u());
    painter.text(at, Align2::CENTER_CENTER, text, mark_font(painter, size * g.u()), color);
}

fn mark_size(text: &str) -> f32 {
    match text.chars().count() {
        1 => 10.0,
        2 => 8.0,
        _ => 6.4,
    }
}

#[derive(Clone, Copy)]
enum Badge {
    Square,
    Circle,
    Hexagon,
    Oval,
}

fn badge_shape(painter: &Painter, g: Grid, badge: Badge, fill: Color32) {
    match badge {
        Badge::Square => {
            painter.rect_filled(g.r(1.0, 1.0, 15.0, 15.0), 3.0 * g.u(), fill);
        }
        Badge::Circle => {
            painter.circle_filled(g.p(8.0, 8.0), 7.0 * g.u(), fill);
        }
        Badge::Hexagon => {
            let points = [(4.0, 1.5), (12.0, 1.5), (15.5, 8.0), (12.0, 14.5), (4.0, 14.5), (0.5, 8.0)];
            painter.add(Shape::convex_polygon(points.iter().map(|&(x, y)| g.p(x, y)).collect(), fill, Stroke::NONE));
        }
        Badge::Oval => {
            let center = g.p(8.0, 8.0);
            let points = (0..48)
                .map(|i| {
                    let angle = i as f32 / 48.0 * std::f32::consts::TAU;
                    center + vec2(7.5 * angle.cos(), 5.5 * angle.sin()) * g.u()
                })
                .collect();
            painter.add(Shape::convex_polygon(points, fill, Stroke::NONE));
        }
    }
}

fn badge(painter: &Painter, g: Grid, shape: Badge, fill: Color32, text: &str, ink: Color32) {
    badge_shape(painter, g, shape, fill);
    mark(painter, g, g.p(8.0, 8.0), text, mark_size(text), ink);
}

fn page(painter: &Painter, g: Grid, outline: Color32, fill: Color32) {
    let body: Vec<Pos2> =
        [(2.5, 1.5), (9.5, 1.5), (13.5, 5.5), (13.5, 14.5), (2.5, 14.5)].iter().map(|&(x, y)| g.p(x, y)).collect();
    painter.add(Shape::convex_polygon(body.clone(), fill, Stroke::NONE));
    painter.add(Shape::closed_line(body, g.s(1.0, outline)));
    painter.add(Shape::line(vec![g.p(9.5, 1.5), g.p(9.5, 5.5), g.p(13.5, 5.5)], g.s(1.0, outline)));
}

fn page_lines(painter: &Painter, g: Grid, y0: f32, count: usize, color: Color32) {
    for i in 0..count {
        let y = y0 + 2.5 * i as f32;
        let right = if i % 2 == 1 { 9.5 } else { 11.0 };
        painter.line_segment([g.p(5.0, y), g.p(right, y)], g.s(1.0, color));
    }
}

fn module_box(painter: &Painter, g: Grid, color: Color32) {
    let top: Vec<Pos2> = [(8.0, 1.5), (14.0, 4.5), (8.0, 7.5), (2.0, 4.5)].iter().map(|&(x, y)| g.p(x, y)).collect();
    let left: Vec<Pos2> = [(2.0, 4.5), (8.0, 7.5), (8.0, 14.5), (2.0, 11.5)].iter().map(|&(x, y)| g.p(x, y)).collect();
    let right: Vec<Pos2> =
        [(8.0, 7.5), (14.0, 4.5), (14.0, 11.5), (8.0, 14.5)].iter().map(|&(x, y)| g.p(x, y)).collect();
    painter.add(Shape::convex_polygon(top, color, Stroke::NONE));
    painter.add(Shape::convex_polygon(left, tint(color, 0.55), Stroke::NONE));
    painter.add(Shape::convex_polygon(right, tint(color, 0.78), Stroke::NONE));
}

fn padlock(painter: &Painter, g: Grid, color: Color32) {
    let shackle = (0..=24)
        .map(|i| {
            let angle = std::f32::consts::PI * (1.0 + i as f32 / 24.0);
            g.p(8.0, 6.5) + 3.0 * g.u() * Vec2::angled(angle)
        })
        .collect::<Vec<_>>();
    let mut path = vec![g.p(5.0, 7.5)];
    path.extend(shackle);
    path.push(g.p(11.0, 7.5));
    painter.add(Shape::line(path, g.s(1.6, color)));
    painter.rect_filled(g.r(3.0, 7.0, 13.0, 15.0), 1.5 * g.u(), color);
    painter.circle_filled(g.p(8.0, 10.5), 1.2 * g.u(), color::INK_DARK);
    painter.line_segment([g.p(8.0, 10.5), g.p(8.0, 12.8)], g.s(1.2, color::INK_DARK));
}

pub fn file_icon(painter: &Painter, rect: Rect, file_name: &str) {
    paint(painter, rect, icon_of(file_name));
    if is_test_file(file_name) {
        test_mark(painter, rect);
    }
}

pub fn file_icon_at(painter: &Painter, rect: Rect, path: &std::path::Path) {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    paint(painter, rect, icon_at(path));
    if is_test_file(&name) {
        test_mark(painter, rect);
    }
}

pub fn icon_at(path: &std::path::Path) -> FileIcon {
    let registry = zen_vfs::file_types::current();
    if !registry.overrides().contains_key(path) {
        return icon_of(&path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default());
    }
    registry.known_name_at(path, None).map_or(FileIcon::Generic, |known| classify(&known))
}

pub fn icon_of(file_name: &str) -> FileIcon {
    let registry = zen_vfs::file_types::current();
    if !registry.types_changed() {
        return classify(file_name);
    }
    registry.known_name(file_name, None).map_or(FileIcon::Generic, |known| classify(&known))
}

pub fn test_mark(painter: &Painter, rect: Rect) {
    let g = Grid::new(rect);
    let u = g.u();
    let center = g.p(12.5, 12.5);
    painter.circle_filled(center, 4.1 * u, color::INK_DARK);
    painter.circle_filled(center, 3.2 * u, color::TEST);
    painter.add(Shape::line(
        vec![center + vec2(-1.7, 0.1) * u, center + vec2(-0.5, 1.3) * u, center + vec2(1.7, -1.2) * u],
        g.s(1.0, color::INK_DARK),
    ));
}

pub fn paint(painter: &Painter, rect: Rect, icon: FileIcon) {
    let g = Grid::new(rect);
    let u = g.u();
    let p = |x: f32, y: f32| g.p(x, y);
    use color as c;
    match icon {
        FileIcon::Go => badge(painter, g, Badge::Square, c::GO, "Go", c::INK_DARK),
        FileIcon::GoModule => module_box(painter, g, c::GO),
        FileIcon::GoSum => padlock(painter, g, c::GO),
        FileIcon::Rust => badge(painter, g, Badge::Circle, c::RUST, "Rs", c::INK_DARK),
        FileIcon::CargoManifest => module_box(painter, g, c::RUST),
        FileIcon::CargoLock => padlock(painter, g, c::RUST),
        FileIcon::Python => badge(painter, g, Badge::Square, c::PY_BLUE, "Py", c::PY_YELLOW),
        FileIcon::PythonProject => module_box(painter, g, c::PY_BLUE),
        FileIcon::PythonRequirements => {
            page(painter, g, c::PY_BLUE, c::PAGE_FILL);
            page_lines(painter, g, 7.5, 3, c::PY_YELLOW);
        }
        FileIcon::JavaScript => badge(painter, g, Badge::Square, c::JS, "JS", c::INK_DARK),
        FileIcon::NodePackage => module_box(painter, g, c::JS),
        FileIcon::Jsx => badge(painter, g, Badge::Square, c::JS, "JX", c::INK_DARK),
        FileIcon::TypeScript => badge(painter, g, Badge::Square, c::TS, "TS", c::WHITE),
        FileIcon::Tsx => badge(painter, g, Badge::Square, c::TS, "TX", c::WHITE),
        FileIcon::Json => {
            let s = g.s(1.5, c::JSON);
            for (outer, inner, tip) in [(5.5, 4.0, 2.0), (10.5, 12.0, 14.0)] {
                let points = [
                    (outer, 2.25),
                    (inner, 3.25),
                    (inner, 6.75),
                    (tip, 8.0),
                    (inner, 9.25),
                    (inner, 12.75),
                    (outer, 13.75),
                ];
                painter.add(Shape::line(points.iter().map(|&(x, y)| p(x, y)).collect(), s));
            }
            painter.circle_filled(p(8.0, 8.0), 1.2 * u, c::JSON);
        }
        FileIcon::Yaml => {
            let s = g.s(2.0, c::YAML);
            painter.line_segment([p(2.0, 3.5), p(11.0, 3.5)], s);
            painter.line_segment([p(5.0, 8.0), p(14.0, 8.0)], s);
            painter.line_segment([p(5.0, 12.5), p(12.0, 12.5)], s);
            painter.circle_filled(p(2.8, 8.0), 1.0 * u, c::YAML);
            painter.circle_filled(p(2.8, 12.5), 1.0 * u, c::YAML);
        }
        FileIcon::Toml => {
            let s = g.s(1.5, c::TOML);
            painter.add(Shape::line(vec![p(5.0, 2.5), p(2.5, 2.5), p(2.5, 13.5), p(5.0, 13.5)], s));
            painter.add(Shape::line(vec![p(11.0, 2.5), p(13.5, 2.5), p(13.5, 13.5), p(11.0, 13.5)], s));
            painter.line_segment([p(5.5, 8.0), p(10.5, 8.0)], g.s(2.0, c::TOML));
        }
        FileIcon::Xml => {
            let s = g.s(1.5, c::XML);
            painter.add(Shape::line(vec![p(5.0, 4.0), p(1.5, 8.0), p(5.0, 12.0)], s));
            painter.add(Shape::line(vec![p(11.0, 4.0), p(14.5, 8.0), p(11.0, 12.0)], s));
            painter.line_segment([p(9.5, 2.5), p(6.5, 13.5)], s);
        }
        FileIcon::Html => {
            badge_shape(painter, g, Badge::Square, c::HTML);
            let s = g.s(1.6, c::WHITE);
            painter.add(Shape::line(vec![p(6.5, 4.5), p(3.5, 8.0), p(6.5, 11.5)], s));
            painter.add(Shape::line(vec![p(9.5, 4.5), p(12.5, 8.0), p(9.5, 11.5)], s));
        }
        FileIcon::Css => {
            badge_shape(painter, g, Badge::Square, c::CSS);
            let s = g.s(1.4, c::WHITE);
            painter.line_segment([p(6.8, 3.8), p(5.8, 12.2)], s);
            painter.line_segment([p(10.2, 3.8), p(9.2, 12.2)], s);
            painter.line_segment([p(3.8, 6.5), p(12.2, 6.5)], s);
            painter.line_segment([p(3.8, 9.5), p(12.2, 9.5)], s);
        }
        FileIcon::Scss => badge(painter, g, Badge::Square, c::SCSS, "S", c::WHITE),
        FileIcon::Less => badge(painter, g, Badge::Square, c::LESS, "L", c::WHITE),
        FileIcon::Vue => badge(painter, g, Badge::Square, c::VUE, "V", c::WHITE),
        FileIcon::Nuxt => badge(painter, g, Badge::Hexagon, c::NUXT, "N", c::WHITE),
        FileIcon::NextJs => badge(painter, g, Badge::Circle, c::NEXT, "N", c::WHITE),
        FileIcon::Svelte => badge(painter, g, Badge::Circle, c::SVELTE, "S", c::WHITE),
        FileIcon::SvelteConfig => badge(painter, g, Badge::Hexagon, c::SVELTE, "S", c::WHITE),
        FileIcon::Markdown => {
            let blue = c::MARKDOWN_MARK;
            let s = Stroke::new(1.9 * u, blue);
            painter.add(Shape::line(vec![p(1.6, 11.6), p(1.6, 4.6), p(4.7, 8.4), p(7.8, 4.6), p(7.8, 11.6)], s));
            painter.line_segment([p(12.4, 4.4), p(12.4, 8.8)], s);
            painter.add(Shape::convex_polygon(vec![p(9.6, 8.2), p(15.2, 8.2), p(12.4, 11.8)], blue, Stroke::NONE));
        }
        FileIcon::Text => {
            page(painter, g, c::PAGE, c::PAGE_FILL);
            page_lines(painter, g, 7.5, 3, c::PAGE);
        }
        FileIcon::Log => {
            page(painter, g, c::PAGE, c::PAGE_FILL);
            for (i, color) in [c::SHELL, c::JS, c::PDF].into_iter().enumerate() {
                let y = 7.5 + 2.5 * i as f32;
                painter.line_segment([p(4.5, y), p(6.0, y)], g.s(1.0, color));
                painter.line_segment([p(7.0, y), p(11.0, y)], g.s(1.0, c::PAGE));
            }
        }
        FileIcon::Shell => {
            painter.rect_filled(g.r(1.0, 2.0, 15.0, 14.0), 2.0 * u, tint(c::SHELL, 0.22));
            painter.rect_stroke(g.r(1.5, 2.5, 14.5, 13.5), 2.0 * u, g.s(1.0, c::SHELL), StrokeKind::Middle);
            let s = g.s(1.5, c::SHELL);
            painter.add(Shape::line(vec![p(4.0, 5.5), p(6.5, 8.0), p(4.0, 10.5)], s));
            painter.line_segment([p(8.0, 10.75), p(12.0, 10.75)], s);
        }
        FileIcon::Docker => {
            for (x, y) in [(1.5, 8.5), (5.75, 8.5), (10.0, 8.5), (3.6, 3.0), (7.9, 3.0)] {
                painter.rect_filled(Rect::from_min_size(p(x, y), vec2(4.0, 4.5) * u), 0.8 * u, c::DOCKER);
            }
            painter.line_segment([p(1.0, 14.5), p(15.0, 14.5)], g.s(1.0, c::DOCKER));
        }
        FileIcon::Compose => {
            for (x, y) in [(1.5, 1.5), (8.5, 1.5), (1.5, 8.5), (8.5, 8.5)] {
                let r = Rect::from_min_size(p(x, y), vec2(6.0, 6.0) * u);
                painter.rect_stroke(r.shrink(0.5 * u), 1.2 * u, g.s(1.0, c::DOCKER), StrokeKind::Middle);
            }
            painter.rect_filled(Rect::from_min_size(p(8.5, 8.5), vec2(6.0, 6.0) * u), 1.2 * u, c::DOCKER);
        }
        FileIcon::Makefile => {
            let handle = g.s(1.8, c::MAKE);
            painter.line_segment([p(3.0, 14.0), p(10.0, 7.0)], handle);
            let head: Vec<Pos2> =
                [(7.0, 3.0), (10.5, -0.5), (15.5, 4.5), (12.0, 8.0)].iter().map(|&(x, y)| p(x, y + 1.5)).collect();
            painter.add(Shape::convex_polygon(head, c::MAKE, Stroke::NONE));
        }
        FileIcon::Kotlin => badge(painter, g, Badge::Square, c::KOTLIN, "K", c::WHITE),
        FileIcon::KotlinScript => badge(painter, g, Badge::Circle, c::KOTLIN, "K", c::WHITE),
        FileIcon::Java => badge(painter, g, Badge::Square, c::JAVA, "J", c::WHITE),
        FileIcon::C => badge(painter, g, Badge::Hexagon, c::C, "C", c::WHITE),
        FileIcon::CHeader => badge(painter, g, Badge::Hexagon, c::C, "H", c::WHITE),
        FileIcon::Cpp => {
            badge_shape(painter, g, Badge::Hexagon, c::CPP);
            mark(painter, g, p(6.0, 8.0), "C", 9.0, c::WHITE);
            plus(painter, g, 11.3, 8.0, c::WHITE);
        }
        FileIcon::CppHeader => {
            badge_shape(painter, g, Badge::Hexagon, c::CPP);
            mark(painter, g, p(6.0, 8.0), "H", 9.0, c::WHITE);
            plus(painter, g, 11.3, 8.0, c::WHITE);
        }
        FileIcon::CSharp => badge(painter, g, Badge::Hexagon, c::CSHARP, "C#", c::WHITE),
        FileIcon::Php => badge(painter, g, Badge::Oval, c::PHP, "P", c::INK_DARK),
        FileIcon::Ruby => {
            let outline: Vec<Pos2> =
                [(4.5, 2.5), (11.5, 2.5), (15.0, 6.5), (8.0, 14.5), (1.0, 6.5)].iter().map(|&(x, y)| p(x, y)).collect();
            painter.add(Shape::convex_polygon(outline, c::RUBY, Stroke::NONE));
            let facet = g.s(1.0, tint(color::WHITE, 0.55));
            painter.add(Shape::line(vec![p(1.5, 6.5), p(14.5, 6.5)], facet));
            painter.add(Shape::line(vec![p(5.0, 6.5), p(8.0, 13.5), p(11.0, 6.5)], facet));
        }
        FileIcon::Sql => {
            let body = g.r(2.5, 3.5, 13.5, 12.5);
            painter.rect_filled(body, 0.0, c::SQL);
            ellipse(painter, p(8.0, 12.5), 5.5 * u, 2.0 * u, c::SQL);
            ellipse(painter, p(8.0, 3.5), 5.5 * u, 2.0 * u, tint(c::SQL, 0.6));
            painter.add(Shape::closed_line(ellipse_points(p(8.0, 3.5), 5.5 * u, 2.0 * u), g.s(1.0, c::SQL)));
            let band = g.s(1.0, c::INK_DARK);
            let arc = |y: f32| ellipse_points(p(8.0, y), 5.5 * u, 2.0 * u).into_iter().skip(1).take(23).collect();
            painter.add(Shape::line(arc(6.5), band));
            painter.add(Shape::line(arc(9.5), band));
        }
        FileIcon::Swift => badge(painter, g, Badge::Square, c::SWIFT, "Sw", c::WHITE),
        FileIcon::Lua => badge(painter, g, Badge::Circle, c::LUA, "Lu", c::WHITE),
        FileIcon::Protobuf => badge(painter, g, Badge::Square, c::PROTO, "PB", c::INK_DARK),
        FileIcon::GraphQl => {
            let nodes = [p(3.0, 3.5), p(12.5, 4.5), p(4.0, 12.5), p(13.0, 12.0)];
            painter.add(Shape::line(nodes.to_vec(), g.s(1.2, c::GRAPHQL)));
            for node in nodes {
                painter.circle_filled(node, 2.1 * u, c::GRAPHQL);
            }
        }
        FileIcon::Image | FileIcon::VectorImage => {
            let color = if icon == FileIcon::Image { c::IMAGE } else { c::VECTOR };
            painter.rect_filled(g.r(1.0, 2.0, 15.0, 14.0), 2.0 * u, tint(color, 0.22));
            painter.rect_stroke(g.r(1.5, 2.5, 14.5, 13.5), 2.0 * u, g.s(1.0, color), StrokeKind::Middle);
            if icon == FileIcon::Image {
                painter.add(Shape::convex_polygon(vec![p(2.0, 13.0), p(6.5, 7.0), p(10.0, 13.0)], color, Stroke::NONE));
                painter.add(Shape::convex_polygon(
                    vec![p(7.5, 13.0), p(10.5, 9.0), p(14.0, 13.0)],
                    color,
                    Stroke::NONE,
                ));
                painter.circle_filled(p(11.0, 5.8), 1.5 * u, color);
            } else {
                painter.add(Shape::line(
                    (0..=16)
                        .map(|i| {
                            let t = i as f32 / 16.0;
                            let a = vec2(4.0, 10.5);
                            let b = vec2(6.0, 3.5);
                            let d = vec2(12.0, 6.0);
                            let q = a * (1.0 - t) * (1.0 - t) + b * 2.0 * t * (1.0 - t) + d * t * t;
                            p(q.x, q.y)
                        })
                        .collect(),
                    g.s(1.2, color),
                ));
                painter.rect_filled(Rect::from_center_size(p(4.0, 10.5), Vec2::splat(2.6 * u)), 0.0, color);
                painter.rect_filled(Rect::from_center_size(p(12.0, 6.0), Vec2::splat(2.6 * u)), 0.0, color);
            }
        }
        FileIcon::Archive => {
            painter.rect_filled(g.r(2.0, 1.0, 14.0, 15.0), 1.5 * u, c::ARCHIVE);
            for i in 0..5 {
                let y = 2.0 + 2.0 * i as f32;
                let x = if i % 2 == 0 { 6.5 } else { 8.0 };
                painter.rect_filled(Rect::from_min_size(p(x, y), vec2(1.5, 1.0) * u), 0.0, c::INK_DARK);
            }
            painter.rect_filled(g.r(6.0, 11.0, 10.0, 14.0), 0.8 * u, c::INK_DARK);
            painter.rect_filled(g.r(7.0, 12.0, 9.0, 13.0), 0.0, c::ARCHIVE);
        }
        FileIcon::Pdf => {
            page(painter, g, c::PDF, tint(c::PDF, 0.25));
            painter.rect_filled(g.r(0.5, 8.0, 12.0, 13.5), 1.0 * u, c::PDF);
            mark(painter, g, p(6.25, 10.75), "PDF", 5.0, c::WHITE);
        }
        FileIcon::Font => {
            mark(painter, g, p(8.0, 8.5), "Aa", 10.0, c::FONT);
        }
        FileIcon::Lock => padlock(painter, g, c::LOCK),
        FileIcon::Ignore => {
            let s = g.s(1.3, c::PAGE);
            painter.circle_stroke(p(8.0, 8.0), 5.6 * u, s);
            painter.line_segment([p(4.2, 11.8), p(11.8, 4.2)], s);
        }
        FileIcon::GitLabCi => {
            let dark = Color32::from_rgb(0xE2, 0x43, 0x29);
            let mid = Color32::from_rgb(0xFC, 0x6D, 0x26);
            let light = Color32::from_rgb(0xFC, 0xA3, 0x26);
            let fill =
                |points: Vec<Pos2>, color: Color32| painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
            fill(vec![p(2.7, 2.0), p(4.6, 7.4), p(1.3, 7.4)], dark);
            fill(vec![p(13.3, 2.0), p(14.7, 7.4), p(11.4, 7.4)], dark);
            fill(vec![p(1.3, 7.4), p(4.6, 7.4), p(8.0, 14.6)], mid);
            fill(vec![p(11.4, 7.4), p(14.7, 7.4), p(8.0, 14.6)], mid);
            fill(vec![p(4.6, 7.4), p(11.4, 7.4), p(8.0, 14.6)], dark);
            fill(vec![p(1.3, 7.4), p(8.0, 14.6), p(0.6, 9.6)], light);
            fill(vec![p(14.7, 7.4), p(15.4, 9.6), p(8.0, 14.6)], light);
        }
        FileIcon::Git => {
            let s = g.s(1.5, c::GIT);
            painter.line_segment([p(5.0, 3.0), p(5.0, 13.0)], s);
            painter.add(Shape::line(vec![p(11.0, 5.0), p(11.0, 7.0), p(5.0, 11.0)], s));
            for (x, y) in [(5.0, 3.0), (5.0, 13.0), (11.0, 4.5)] {
                painter.circle_filled(p(x, y), 2.2 * u, c::GIT);
            }
        }
        FileIcon::EditorConfig => {
            let s = g.s(1.2, c::EDITOR_CONFIG);
            for (y, knob) in [(3.5, 10.5), (8.0, 5.0), (12.5, 9.0)] {
                painter.line_segment([p(1.5, y), p(14.5, y)], s);
                painter.circle_filled(p(knob, y), 2.0 * u, c::EDITOR_CONFIG);
                painter.circle_filled(p(knob, y), 0.9 * u, c::INK_DARK);
            }
        }
        FileIcon::Env => {
            painter.circle_stroke(p(5.0, 8.0), 3.2 * u, g.s(1.8, c::ENV));
            let s = g.s(1.8, c::ENV);
            painter.line_segment([p(8.0, 8.0), p(15.0, 8.0)], s);
            painter.line_segment([p(13.0, 8.0), p(13.0, 11.5)], s);
            painter.line_segment([p(10.5, 8.0), p(10.5, 10.5)], s);
        }
        FileIcon::License => {
            let ribbon = tint(c::LICENSE, 0.75);
            for points in [
                [(4.0, 9.0), (7.0, 10.0), (6.5, 15.0), (5.0, 13.5), (3.5, 15.0)],
                [(12.0, 9.0), (9.0, 10.0), (9.5, 15.0), (11.0, 13.5), (12.5, 15.0)],
            ] {
                let [a, b, d, e, f] = points.map(|(x, y)| p(x, y));
                painter.add(Shape::convex_polygon(vec![a, b, d, e], ribbon, Stroke::NONE));
                painter.add(Shape::convex_polygon(vec![a, e, f], ribbon, Stroke::NONE));
            }
            painter.circle_filled(p(8.0, 6.5), 5.5 * u, c::LICENSE);
            painter.circle_stroke(p(8.0, 6.5), 3.2 * u, g.s(1.0, c::INK_DARK));
        }
        FileIcon::Fluent => {
            painter.rect_filled(g.r(1.0, 2.0, 15.0, 11.0), 2.5 * u, c::FLUENT);
            painter.add(Shape::convex_polygon(vec![p(4.0, 10.5), p(8.0, 10.5), p(4.0, 14.5)], c::FLUENT, Stroke::NONE));
            let s = g.s(1.2, c::INK_DARK);
            painter.line_segment([p(4.0, 5.0), p(12.0, 5.0)], s);
            painter.line_segment([p(4.0, 8.0), p(9.5, 8.0)], s);
        }
        FileIcon::WebAssembly => badge(painter, g, Badge::Circle, c::WASM, "W", c::WHITE),
        FileIcon::Csv => {
            let r = g.r(1.5, 2.5, 14.5, 13.5);
            painter.rect_filled(g.r(1.5, 2.5, 14.5, 5.5), 0.0, tint(c::CSV, 0.7));
            let s = g.s(1.0, c::CSV);
            painter.rect_stroke(r, 1.0 * u, s, StrokeKind::Middle);
            painter.line_segment([p(1.5, 5.5), p(14.5, 5.5)], s);
            painter.line_segment([p(1.5, 9.5), p(14.5, 9.5)], s);
            painter.line_segment([p(6.0, 2.5), p(6.0, 13.5)], s);
            painter.line_segment([p(10.0, 2.5), p(10.0, 13.5)], s);
        }
        FileIcon::Generic => page(painter, g, c::PAGE, c::PAGE_FILL),
    }
}

fn plus(painter: &Painter, g: Grid, x: f32, y: f32, color: Color32) {
    let s = g.s(1.4, color);
    painter.line_segment([g.p(x - 2.0, y), g.p(x + 2.0, y)], s);
    painter.line_segment([g.p(x, y - 2.0), g.p(x, y + 2.0)], s);
}

fn ellipse_points(center: Pos2, rx: f32, ry: f32) -> Vec<Pos2> {
    (0..48)
        .map(|i| {
            let angle = i as f32 / 48.0 * std::f32::consts::TAU;
            center + vec2(rx * angle.cos(), ry * angle.sin())
        })
        .collect()
}

fn ellipse(painter: &Painter, center: Pos2, rx: f32, ry: f32, fill: Color32) {
    painter.add(Shape::convex_polygon(ellipse_points(center, rx, ry), fill, Stroke::NONE));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderKind {
    Plain,
    Source,
    Test,
    Docs,
    Assets,
    Vcs,
    Excluded,
    ResourceRoot,
}

pub fn classify_folder(folder_name: &str, excluded: bool) -> FolderKind {
    if excluded {
        return FolderKind::Excluded;
    }
    match folder_name.to_ascii_lowercase().as_str() {
        "src" | "source" | "sources" | "lib" | "crates" | "pkg" | "cmd" | "internal" => FolderKind::Source,
        "test" | "tests" | "__tests__" | "spec" | "specs" | "testdata" | "benches" => FolderKind::Test,
        "docs" | "doc" | "documentation" => FolderKind::Docs,
        "assets" | "resources" | "res" | "static" | "public" | "images" | "locales" => FolderKind::Assets,
        ".github" | ".gitlab" | ".git" | ".forgejo" => FolderKind::Vcs,
        _ => FolderKind::Plain,
    }
}

pub fn folder_color(kind: FolderKind) -> Color32 {
    match kind {
        FolderKind::Plain => color::FOLDER,
        FolderKind::Source => color::FOLDER_SOURCE,
        FolderKind::Test => color::FOLDER_TEST,
        FolderKind::Docs => color::FOLDER_DOCS,
        FolderKind::Assets => color::FOLDER_ASSETS,
        FolderKind::Vcs => color::FOLDER_VCS,
        FolderKind::Excluded => color::FOLDER_EXCLUDED,
        FolderKind::ResourceRoot => color::FOLDER_RESOURCE,
    }
}

pub fn symlink_mark(painter: &Painter, rect: Rect) {
    let g = Grid::new(rect);
    let u = g.u();
    let plate = g.r(0.0, 8.5, 7.5, 16.0);
    painter.rect_filled(plate, 1.5 * u, color::INK_DARK);
    painter.rect_stroke(plate, 1.5 * u, g.s(0.8, color::PAGE), StrokeKind::Inside);
    let s = g.s(1.2, color::WHITE);
    painter.add(Shape::line(vec![g.p(1.8, 14.4), g.p(1.8, 12.4), g.p(5.2, 12.4)], s));
    painter.add(Shape::convex_polygon(
        vec![g.p(4.4, 10.6), g.p(6.4, 12.4), g.p(4.4, 14.2)],
        color::WHITE,
        Stroke::NONE,
    ));
}

pub fn folder_icon(painter: &Painter, rect: Rect, folder_name: &str, excluded: bool, open: bool) {
    folder_icon_of(painter, rect, classify_folder(folder_name, excluded), open);
}

pub fn folder_icon_of(painter: &Painter, rect: Rect, kind: FolderKind, open: bool) {
    let g = Grid::new(rect);
    let u = g.u();
    let p = |x: f32, y: f32| g.p(x, y);
    let color = match kind {
        FolderKind::Excluded | FolderKind::ResourceRoot => folder_color(kind),
        _ => color::FOLDER,
    };
    let mark = folder_color(kind);
    let outline = g.s(1.0, color);

    let back = vec![
        p(1.5, 13.5),
        p(1.5, 3.5),
        p(2.5, 2.5),
        p(6.0, 2.5),
        p(7.5, 4.0),
        p(13.5, 4.0),
        p(14.5, 5.0),
        p(14.5, 13.5),
    ];
    let fill = if kind == FolderKind::ResourceRoot { color } else { tint(color, if open { 0.18 } else { 0.26 }) };
    painter.add(Shape::convex_polygon(
        vec![p(1.5, 3.5), p(2.5, 2.5), p(6.0, 2.5), p(7.5, 4.0), p(1.5, 4.0)],
        fill,
        Stroke::NONE,
    ));
    painter.rect_filled(g.r(1.5, 4.0, 14.5, 13.5), 0.0, fill);
    painter.add(Shape::closed_line(back, outline));

    if open {
        let flap = vec![p(3.5, 7.5), p(15.5, 7.5), p(13.5, 13.5), p(1.5, 13.5)];
        painter.add(Shape::convex_polygon(flap.clone(), color, Stroke::NONE));
        painter.add(Shape::closed_line(flap, outline));
    } else {
        painter.line_segment([p(1.5, 6.5), p(14.5, 6.5)], outline);
    }

    let ink = if open { color::INK_DARK } else { mark };
    let dot = p(11.5, 10.5);
    match kind {
        FolderKind::Plain | FolderKind::ResourceRoot => {}
        FolderKind::Source => {
            let s = g.s(1.2, ink);
            painter.add(Shape::line(
                vec![dot + vec2(-1.0, -1.8) * u, dot + vec2(-2.6, 0.0) * u, dot + vec2(-1.0, 1.8) * u],
                s,
            ));
            painter.add(Shape::line(
                vec![dot + vec2(1.0, -1.8) * u, dot + vec2(2.6, 0.0) * u, dot + vec2(1.0, 1.8) * u],
                s,
            ));
        }
        FolderKind::Test => {
            let s = g.s(1.3, ink);
            painter.add(Shape::line(
                vec![dot + vec2(-2.4, 0.0) * u, dot + vec2(-0.8, 1.6) * u, dot + vec2(2.4, -1.8) * u],
                s,
            ));
        }
        FolderKind::Docs => {
            let s = g.s(1.0, ink);
            for dy in [-1.5, 0.0, 1.5] {
                painter.line_segment([dot + vec2(-2.4, dy) * u, dot + vec2(2.4, dy) * u], s);
            }
        }
        FolderKind::Assets => {
            painter.add(Shape::convex_polygon(
                vec![dot + vec2(-2.6, 1.8) * u, dot + vec2(-0.4, -1.8) * u, dot + vec2(1.8, 1.8) * u],
                ink,
                Stroke::NONE,
            ));
            painter.circle_filled(dot + vec2(2.0, -1.4) * u, 0.9 * u, ink);
        }
        FolderKind::Vcs => {
            painter.circle_filled(dot + vec2(-1.8, -1.6) * u, 1.0 * u, ink);
            painter.circle_filled(dot + vec2(-1.8, 1.8) * u, 1.0 * u, ink);
            painter.circle_filled(dot + vec2(1.8, -1.6) * u, 1.0 * u, ink);
            let s = g.s(1.0, ink);
            painter.line_segment([dot + vec2(-1.8, -1.6) * u, dot + vec2(-1.8, 1.8) * u], s);
            painter.add(Shape::line(vec![dot + vec2(1.8, -1.6) * u, dot + vec2(-1.8, 1.0) * u], s));
        }
        FolderKind::Excluded => {
            let s = g.s(1.2, ink);
            painter.circle_stroke(dot, 2.2 * u, s);
            painter.line_segment([dot + vec2(-1.5, 1.5) * u, dot + vec2(1.5, -1.5) * u], s);
        }
    }
}

#[cfg(test)]
mod tests {
    use egui::pos2;

    use super::*;

    const NAMES: &[(&str, FileIcon)] = &[
        ("main.go", FileIcon::Go),
        ("go.mod", FileIcon::GoModule),
        ("go.work", FileIcon::GoModule),
        ("go.sum", FileIcon::GoSum),
        ("lib.rs", FileIcon::Rust),
        ("Cargo.toml", FileIcon::CargoManifest),
        ("Cargo.lock", FileIcon::CargoLock),
        ("app.py", FileIcon::Python),
        ("types.pyi", FileIcon::Python),
        ("requirements.txt", FileIcon::PythonRequirements),
        ("requirements-dev.txt", FileIcon::PythonRequirements),
        ("pyproject.toml", FileIcon::PythonProject),
        ("index.js", FileIcon::JavaScript),
        ("module.mjs", FileIcon::JavaScript),
        ("config.cjs", FileIcon::JavaScript),
        ("package.json", FileIcon::NodePackage),
        ("App.jsx", FileIcon::Jsx),
        ("index.ts", FileIcon::TypeScript),
        ("env.d.ts", FileIcon::TypeScript),
        ("App.tsx", FileIcon::Tsx),
        ("data.json", FileIcon::Json),
        ("tsconfig.jsonc", FileIcon::Json),
        ("ci.yml", FileIcon::Yaml),
        ("config.yaml", FileIcon::Yaml),
        ("rustfmt.toml", FileIcon::Toml),
        ("pom.xml", FileIcon::Xml),
        ("index.html", FileIcon::Html),
        ("style.css", FileIcon::Css),
        ("theme.scss", FileIcon::Scss),
        ("theme.less", FileIcon::Less),
        ("App.vue", FileIcon::Vue),
        ("App.csproj", FileIcon::Xml),
        ("feed.rss", FileIcon::Xml),
        ("nuxt.config.ts", FileIcon::Nuxt),
        ("nuxt.config.mjs", FileIcon::Nuxt),
        ("next.config.mjs", FileIcon::NextJs),
        ("next.config.ts", FileIcon::NextJs),
        ("next-env.d.ts", FileIcon::NextJs),
        ("Counter.svelte", FileIcon::Svelte),
        ("svelte.config.js", FileIcon::SvelteConfig),
        ("svelte.config.ts", FileIcon::SvelteConfig),
        ("nuxt.config.json", FileIcon::Json),
        ("README.md", FileIcon::Markdown),
        ("notes.txt", FileIcon::Text),
        ("build.log", FileIcon::Log),
        ("run.sh", FileIcon::Shell),
        ("setup.bash", FileIcon::Shell),
        ("init.zsh", FileIcon::Shell),
        ("Dockerfile", FileIcon::Docker),
        ("Dockerfile.dev", FileIcon::Docker),
        ("app.dockerfile", FileIcon::Docker),
        ("compose.yaml", FileIcon::Compose),
        ("docker-compose.yml", FileIcon::Compose),
        ("Makefile", FileIcon::Makefile),
        ("Main.kt", FileIcon::Kotlin),
        ("build.gradle.kts", FileIcon::KotlinScript),
        ("script.main.kts", FileIcon::KotlinScript),
        ("Main.java", FileIcon::Java),
        ("main.c", FileIcon::C),
        ("main.h", FileIcon::CHeader),
        ("main.cpp", FileIcon::Cpp),
        ("main.cc", FileIcon::Cpp),
        ("main.hpp", FileIcon::CppHeader),
        ("Program.cs", FileIcon::CSharp),
        ("index.php", FileIcon::Php),
        ("app.rb", FileIcon::Ruby),
        ("Gemfile", FileIcon::Ruby),
        ("schema.sql", FileIcon::Sql),
        ("App.swift", FileIcon::Swift),
        ("init.lua", FileIcon::Lua),
        ("api.proto", FileIcon::Protobuf),
        ("schema.graphql", FileIcon::GraphQl),
        ("logo.png", FileIcon::Image),
        ("photo.jpg", FileIcon::Image),
        ("photo.jpeg", FileIcon::Image),
        ("anim.gif", FileIcon::Image),
        ("favicon.ico", FileIcon::Image),
        ("hero.webp", FileIcon::Image),
        ("logo.svg", FileIcon::VectorImage),
        ("release.zip", FileIcon::Archive),
        ("release.tar", FileIcon::Archive),
        ("release.tar.gz", FileIcon::Archive),
        ("manual.pdf", FileIcon::Pdf),
        ("Inter.ttf", FileIcon::Font),
        ("Inter.otf", FileIcon::Font),
        ("Inter.woff2", FileIcon::Font),
        ("yarn.lock", FileIcon::Lock),
        ("package-lock.json", FileIcon::Lock),
        ("flake.lock", FileIcon::Lock),
        ("other.lock", FileIcon::Lock),
        (".gitignore", FileIcon::Ignore),
        (".dockerignore", FileIcon::Ignore),
        (".gitlab-ci.yml", FileIcon::GitLabCi),
        ("deploy.gitlab-ci.yml", FileIcon::GitLabCi),
        (".gitattributes", FileIcon::Git),
        (".gitmodules", FileIcon::Git),
        (".editorconfig", FileIcon::EditorConfig),
        (".env", FileIcon::Env),
        (".env.local", FileIcon::Env),
        ("LICENSE", FileIcon::License),
        ("LICENSE-MIT", FileIcon::License),
        ("LICENSE.txt", FileIcon::License),
        ("NOTICE", FileIcon::License),
        ("COPYING", FileIcon::License),
        ("main.ftl", FileIcon::Fluent),
        ("module.wasm", FileIcon::WebAssembly),
        ("data.csv", FileIcon::Csv),
        ("unknown.xyz", FileIcon::Generic),
        ("no-extension", FileIcon::Generic),
        ("", FileIcon::Generic),
        (".", FileIcon::Generic),
        ("licensed.rs", FileIcon::Rust),
        ("main_test.go", FileIcon::Go),
        ("test_app.py", FileIcon::Python),
        ("App.test.tsx", FileIcon::Tsx),
        ("api.spec.ts", FileIcon::TypeScript),
        ("MainTest.kt", FileIcon::Kotlin),
        ("MainTest.java", FileIcon::Java),
        ("app_spec.rb", FileIcon::Ruby),
    ];

    #[test]
    fn every_name_resolves_to_its_icon() {
        for &(name, icon) in NAMES {
            assert_eq!(classify(name), icon, "{name}");
        }
    }

    #[test]
    fn test_files_are_recognised_by_name() {
        for name in [
            "main_test.go",
            "handler_test.go",
            "models_test.py",
            "test_models.py",
            "App.test.tsx",
            "util.test.ts",
            "util.test.js",
            "Button.test.jsx",
            "api.spec.ts",
            "api.spec.js",
            "Form.spec.tsx",
            "View.spec.jsx",
            "UserServiceTest.kt",
            "UserServiceTest.java",
            "user_spec.rb",
        ] {
            assert!(is_test_file(name), "{name}");
            assert_ne!(classify(name), FileIcon::Generic, "{name} keeps its language icon");
        }
        for name in [
            "main.go",
            "_test.go",
            "test.go",
            "testing.go",
            "test_.go",
            "contest.py",
            "test.py",
            "Latest.java",
            "Test.java",
            "test.ts",
            ".test.ts",
            "spec.js",
            "latest.ts",
            "test_data.json",
            "spec_helper.rb",
            "",
        ] {
            assert!(!is_test_file(name), "{name}");
        }
        assert_eq!(classify("main_test.go"), FileIcon::Go);
        assert_eq!(classify("App.test.tsx"), FileIcon::Tsx);
    }

    #[test]
    fn every_icon_is_reachable() {
        for icon in FileIcon::ALL {
            assert!(NAMES.iter().any(|&(_, mapped)| mapped == icon), "{icon:?} has no name in the test list");
        }
    }

    #[test]
    fn folders() {
        assert_eq!(classify_folder("src", false), FolderKind::Source);
        assert_eq!(classify_folder("tests", false), FolderKind::Test);
        assert_eq!(classify_folder("test", false), FolderKind::Test);
        assert_eq!(classify_folder("docs", false), FolderKind::Docs);
        assert_eq!(classify_folder("assets", false), FolderKind::Assets);
        assert_eq!(classify_folder("resources", false), FolderKind::Assets);
        assert_eq!(classify_folder(".github", false), FolderKind::Vcs);
        assert_eq!(classify_folder(".gitlab", false), FolderKind::Vcs);
        assert_eq!(classify_folder("target", true), FolderKind::Excluded);
        assert_eq!(classify_folder("node_modules", true), FolderKind::Excluded);
        assert_eq!(classify_folder("target", false), FolderKind::Plain);
        assert_eq!(classify_folder("widgets", false), FolderKind::Plain);
    }

    #[test]
    fn drawing_does_not_panic() {
        for install in [false, true] {
            let ctx = egui::Context::default();
            if install {
                theme::install_fonts(&ctx);
            }
            for _ in 0..2 {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    let painter = ui.painter();
                    for (i, &(name, _)) in NAMES.iter().enumerate() {
                        for size in [14.0, 16.0, 32.0, 15.3] {
                            let rect = Rect::from_min_size(pos2(i as f32 * 2.0, size), Vec2::splat(size));
                            file_icon(painter, rect, name);
                        }
                    }
                    test_mark(painter, Rect::from_min_size(Pos2::ZERO, Vec2::splat(16.0)));
                    for name in ["src", "tests", "docs", "assets", ".github", "target", "node_modules", "misc"] {
                        for excluded in [false, true] {
                            for open in [false, true] {
                                folder_icon(
                                    painter,
                                    Rect::from_min_size(Pos2::ZERO, Vec2::splat(16.0)),
                                    name,
                                    excluded,
                                    open,
                                );
                            }
                        }
                    }
                });
                output.textures_delta.clear();
            }
        }
    }
}
