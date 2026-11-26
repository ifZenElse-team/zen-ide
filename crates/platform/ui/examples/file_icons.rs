use eframe::egui::{self, Color32, Pos2, Rect, Vec2, pos2, vec2};
use zen_ui::file_icons::{self, FileIcon};
use zen_ui::theme;

const SAMPLES: &[&str] = &[
    "main.go",
    "go.mod",
    "go.sum",
    "lib.rs",
    "Cargo.toml",
    "Cargo.lock",
    "app.py",
    "pyproject.toml",
    "requirements.txt",
    "index.js",
    "package.json",
    "App.jsx",
    "index.ts",
    "App.tsx",
    "data.json",
    "ci.yml",
    "rustfmt.toml",
    "pom.xml",
    "index.html",
    "style.css",
    "theme.scss",
    "theme.less",
    "README.md",
    "notes.txt",
    "build.log",
    "run.sh",
    "Dockerfile",
    "compose.yaml",
    "Makefile",
    "Main.kt",
    "Main.java",
    "main.c",
    "main.h",
    "main.cpp",
    "main.hpp",
    "Program.cs",
    "index.php",
    "Gemfile",
    "schema.sql",
    "App.swift",
    "init.lua",
    "api.proto",
    "schema.graphql",
    "logo.png",
    "logo.svg",
    "release.zip",
    "manual.pdf",
    "Inter.woff2",
    "yarn.lock",
    ".gitignore",
    ".editorconfig",
    ".env",
    "LICENSE",
    "main.ftl",
    "module.wasm",
    "data.csv",
    "unknown.xyz",
    "main_test.go",
    "test_app.py",
    "App.test.tsx",
    "api.spec.js",
    "MainTest.kt",
    "MainTest.java",
    "app_spec.rb",
];

const TESTS: &[FileIcon] =
    &[FileIcon::Go, FileIcon::Python, FileIcon::Tsx, FileIcon::JavaScript, FileIcon::Kotlin, FileIcon::Java];

const FOLDERS: &[(&str, bool)] = &[
    ("misc", false),
    ("src", false),
    ("tests", false),
    ("docs", false),
    ("assets", false),
    (".github", false),
    ("target", true),
    ("node_modules", true),
];

const ISLAND: Color32 = Color32::from_rgb(0x1F, 0x20, 0x23);
const TEXT: Color32 = Color32::from_rgb(0xDF, 0xE1, 0xE5);

const ROWS: usize = 20;

struct Gallery;

impl eframe::App for Gallery {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        zen_ui::capture::tick(ui.ctx());
        let painter = ui.painter().clone();
        painter.rect_filled(ui.max_rect(), 0.0, ISLAND);
        let origin = ui.max_rect().min + vec2(16.0, 16.0);
        let font = theme::regular(13.0);

        let cell = |index: usize, draw: &dyn Fn(Rect), label: &str| {
            let x = origin.x + (index / ROWS) as f32 * 170.0;
            let y = origin.y + (index % ROWS) as f32 * 24.0;
            draw(Rect::from_min_size(pos2(x, y + 2.0), Vec2::splat(16.0)));
            painter.text(pos2(x + 22.0, y + 10.0), egui::Align2::LEFT_CENTER, label, font.clone(), TEXT);
        };
        for (i, name) in SAMPLES.iter().enumerate() {
            cell(i, &|rect| file_icons::file_icon(&painter, rect, name), name);
        }
        for (i, &(name, excluded)) in FOLDERS.iter().enumerate() {
            for open in [false, true] {
                let index = SAMPLES.len().div_ceil(ROWS) * ROWS + i + if open { FOLDERS.len() + 1 } else { 0 };
                cell(index, &|rect| file_icons::folder_icon(&painter, rect, name, excluded, open), name);
            }
        }

        let large_top = origin.y + ROWS as f32 * 24.0 + 16.0;
        let per_row = 20;
        for (i, icon) in FileIcon::ALL.iter().enumerate() {
            let at = Pos2::new(origin.x + (i % per_row) as f32 * 44.0, large_top + (i / per_row) as f32 * 44.0);
            file_icons::paint(&painter, Rect::from_min_size(at, Vec2::splat(32.0)), *icon);
        }
        let folders_top = large_top + FileIcon::ALL.len().div_ceil(per_row) as f32 * 44.0;
        for (i, &(name, excluded)) in FOLDERS.iter().enumerate() {
            for (j, open) in [false, true].into_iter().enumerate() {
                let at = Pos2::new(origin.x + (2 * i + j) as f32 * 44.0, folders_top);
                file_icons::folder_icon(&painter, Rect::from_min_size(at, Vec2::splat(32.0)), name, excluded, open);
            }
        }
        for (i, icon) in TESTS.iter().enumerate() {
            let at = Pos2::new(origin.x + (2 * FOLDERS.len() + 1 + i) as f32 * 44.0, folders_top);
            let rect = Rect::from_min_size(at, Vec2::splat(32.0));
            file_icons::paint(&painter, rect, *icon);
            file_icons::test_mark(&painter, rect);
        }
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("File icons").with_inner_size([1060.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "file-icons",
        options,
        Box::new(|creation| {
            theme::install(&creation.egui_ctx);
            Ok(Box::new(Gallery))
        }),
    )
}
