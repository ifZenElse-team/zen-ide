use serde::Serialize;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};

use crate::theme::{self, EditorFaces};

pub const BUNDLED: &str = "JetBrains Mono";

pub(crate) const INTER_REGULAR: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Regular.ttf");

pub(crate) const INTER_BOLD: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Bold.ttf");

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontList {
    pub bundled: String,
    pub installed: Vec<String>,
}

pub struct ListFonts;

impl Command for ListFonts {
    type Params = NoParams;
    type Output = FontList;
    const ID: &'static str = "ui.fonts.list";
    const TITLE: &'static str = "List Editor Fonts";
    const DESCRIPTION: &'static str = "Lists the bundled editor font and the monospaced fonts installed on the machine, \
         the values of `editorFont.family` in the settings.";

    fn run(&self, _: &mut Session, _: NoParams) -> Result<FontList, CommandError> {
        Ok(FontList { bundled: BUNDLED.to_owned(), installed: system::installed_blocking() })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListFonts)
}

pub fn installed(ctx: &egui::Context) -> Option<Vec<String>> {
    system::installed(ctx)
}

pub fn atlas_generation(ctx: &egui::Context) -> u64 {
    #[derive(Clone)]
    struct Probe {
        pass: u64,
        galley: std::sync::Arc<egui::Galley>,
        generation: u64,
    }
    let id = egui::Id::unique(("zen-font-atlas-probe", ctx.viewport_id()));
    let pass = ctx.cumulative_pass_nr();
    let last: Option<Probe> = ctx.data(|data| data.get_temp(id));
    if let Some(last) = &last
        && last.pass == pass
    {
        return last.generation;
    }
    let galley = ctx
        .fonts_mut(|fonts| fonts.layout_no_wrap("Zen".to_owned(), egui::FontId::monospace(10.0), egui::Color32::WHITE));
    let generation = match last {
        Some(last) if std::sync::Arc::ptr_eq(&last.galley, &galley) => last.generation,
        Some(last) => last.generation + 1,
        None => 0,
    };
    ctx.data_mut(|data| data.insert_temp(id, Probe { pass, galley, generation }));
    generation
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Wanted {
    editor: Option<String>,
    preview: Option<String>,
}

pub fn use_editor_font(ctx: &egui::Context, family: Option<&str>) {
    want(ctx, |wanted| wanted.editor = family.map(str::to_owned));
}

pub fn use_preview_font(ctx: &egui::Context, family: Option<&str>) {
    want(ctx, |wanted| wanted.preview = family.map(str::to_owned));
}

fn want(ctx: &egui::Context, change: impl FnOnce(&mut Wanted)) {
    let id = egui::Id::unique("zen-editor-fonts");
    let installed: Wanted = ctx.data(|data| data.get_temp(id)).unwrap_or_default();
    let mut wanted = installed.clone();
    change(&mut wanted);
    if wanted == installed {
        return;
    }
    let chosen = [&wanted.editor, &wanted.preview];
    let faces = if chosen.iter().all(|family| family.is_none()) {
        Some([EditorFaces::default(), EditorFaces::default()])
    } else {
        system::faces(ctx, chosen)
    };
    let Some([editor, preview]) = faces else { return };
    ctx.set_fonts(theme::font_definitions(&editor, &preview));
    ctx.data_mut(|data| data.insert_temp(id, wanted));
}

#[cfg(not(target_arch = "wasm32"))]
mod system {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, OnceLock};

    use fontdb::{Database, Family, Query, Style, Weight};

    use crate::theme::{EditorFaces, Face};

    static DATABASE: OnceLock<Arc<Database>> = OnceLock::new();
    static STARTED: AtomicBool = AtomicBool::new(false);

    fn load() -> Arc<Database> {
        let mut database = Database::new();
        database.load_system_fonts();
        Arc::new(database)
    }

    fn database(ctx: &egui::Context) -> Option<Arc<Database>> {
        if let Some(database) = DATABASE.get() {
            return Some(Arc::clone(database));
        }
        if !STARTED.swap(true, Ordering::SeqCst) {
            let ctx = ctx.clone();
            let _ = std::thread::Builder::new().name("zen-fonts".to_owned()).spawn(move || {
                DATABASE.get_or_init(load);
                ctx.request_repaint();
            });
        }
        None
    }

    pub(super) fn installed(ctx: &egui::Context) -> Option<Vec<String>> {
        database(ctx).map(|database| monospaced_families(&database))
    }

    pub(super) fn installed_blocking() -> Vec<String> {
        monospaced_families(DATABASE.get_or_init(load))
    }

    pub(super) fn faces(ctx: &egui::Context, families: [&Option<String>; 2]) -> Option<[EditorFaces; 2]> {
        let database = database(ctx)?;
        Some(families.map(|family| family.as_deref().map(|name| faces_of(&database, name)).unwrap_or_default()))
    }

    pub(super) fn monospaced_families(database: &Database) -> Vec<String> {
        let mut names: Vec<String> = database
            .faces()
            .filter(|face| face.monospaced)
            .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
            .filter(|name| !name.starts_with('.') && name != super::BUNDLED)
            .collect();
        names.sort_by_key(|name| name.to_lowercase());
        names.dedup();
        names
    }

    pub(super) fn faces_of(database: &Database, name: &str) -> EditorFaces {
        let face = |weight: Weight, style: Style| -> Option<Face> {
            let query = Query { families: &[Family::Name(name)], weight, style, ..Query::default() };
            let id = database.query(&query)?;
            let info = database.face(id)?;
            let bold_enough = weight != Weight::BOLD || info.weight.0 >= Weight::SEMIBOLD.0;
            if !bold_enough || info.style != style {
                return None;
            }
            database.with_face_data(id, |bytes, index| Face { bytes: Arc::new(bytes.to_vec()), index })
        };
        EditorFaces {
            regular: face(Weight::NORMAL, Style::Normal),
            bold: face(Weight::BOLD, Style::Normal),
            italic: face(Weight::NORMAL, Style::Italic),
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod system {
    use crate::theme::EditorFaces;

    pub(super) fn installed(_: &egui::Context) -> Option<Vec<String>> {
        Some(Vec::new())
    }

    pub(super) fn installed_blocking() -> Vec<String> {
        Vec::new()
    }

    pub(super) fn faces(_: &egui::Context, _: [&Option<String>; 2]) -> Option<[EditorFaces; 2]> {
        Some([EditorFaces::default(), EditorFaces::default()])
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::system::{faces_of, monospaced_families};

    fn fixture() -> fontdb::Database {
        let mut database = fontdb::Database::new();
        for bytes in [
            include_bytes!("../../../../assets/fonts/JetBrainsMono-Regular.ttf").as_slice(),
            include_bytes!("../../../../assets/fonts/JetBrainsMono-Bold.ttf"),
            include_bytes!("../../../../assets/fonts/Inter-Regular.ttf"),
            include_bytes!("../../../../assets/fonts/Inter-Bold.ttf"),
        ] {
            database.load_font_data(bytes.to_vec());
        }
        database
    }

    #[test]
    fn only_monospaced_fonts_are_listed() {
        let database = fixture();
        let monospaced: Vec<&str> = database
            .faces()
            .filter(|face| face.monospaced)
            .filter_map(|face| face.families.first().map(|(name, _)| name.as_str()))
            .collect();
        assert_eq!(monospaced, [super::BUNDLED, super::BUNDLED], "Inter is proportional");
        assert!(monospaced_families(&database).is_empty(), "the bundled family is offered on its own");
    }

    #[test]
    fn a_family_gives_the_faces_it_has() {
        let database = fixture();
        let faces = faces_of(&database, super::BUNDLED);
        assert!(faces.regular.is_some() && faces.bold.is_some());
        assert!(faces.italic.is_none(), "the fixture has no italic face; the bundled italic stands in");
        let inter = faces_of(&database, "Inter");
        assert!(inter.regular.is_some() && inter.bold.is_some());
        assert!(faces_of(&database, "No Such Font").regular.is_none());
    }
}
