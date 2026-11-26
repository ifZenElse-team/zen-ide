use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Align2, Color32, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};

use super::IdePalette;
use crate::accessibility;

const ASK_EVERY: f64 = 5.0;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct MarkerTarget {
    name: String,
    detail: String,
    path: PathBuf,
    line: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Marker {
    pub(super) line: usize,
    bean: bool,
    tooltip: String,
    targets: Vec<MarkerTarget>,
}

impl Marker {
    fn from_json(value: &Value) -> Option<Self> {
        Some(Self {
            line: value["line"].as_u64()?.checked_sub(1)? as usize,
            bean: value["kind"] == "bean",
            tooltip: value["tooltip"].as_str().unwrap_or_default().to_owned(),
            targets: value["targets"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|target| {
                    Some(MarkerTarget {
                        name: target["name"].as_str()?.to_owned(),
                        detail: target["detail"].as_str().unwrap_or_default().to_owned(),
                        path: PathBuf::from(target["path"].as_str()?),
                        line: target["line"].as_u64()?,
                    })
                })
                .collect(),
        })
    }
}

struct Menu {
    path: PathBuf,
    marker: Marker,
    at: egui::Pos2,
    fresh: bool,
}

#[derive(Default)]
pub(super) struct LineMarkers {
    shown: Option<(PathBuf, u64)>,
    generation: u64,
    asked: f64,
    list: Arc<Vec<Marker>>,
    menu: Option<Menu>,
    pub(super) notice: Option<String>,
}

pub(super) fn glyph(painter: &egui::Painter, rect: Rect, bean: bool, color: Color32) {
    let center = rect.center();
    let radius = rect.width().min(rect.height()) / 2.0 - 1.0;
    if bean {
        painter.circle_filled(center, radius, color);
    } else {
        painter.circle_stroke(center, radius - 0.5, egui::Stroke::new(1.4, color));
        painter.circle_filled(center, radius * 0.35, color);
    }
}

impl LineMarkers {
    pub(super) fn follow(&mut self, registry: &Registry, session: &mut Session, path: &Path, version: u64, now: f64) {
        let key = (path.to_owned(), version);
        if self.shown.as_ref() == Some(&key) && now - self.asked < ASK_EVERY {
            return;
        }
        self.asked = now;
        let answer = registry.dispatch(session, "editor.lineMarkers.list", json!({ "path": path })).unwrap_or_default();
        let generation = answer["generation"].as_u64().unwrap_or(0);
        if self.shown.as_ref() == Some(&key) && generation == self.generation {
            return;
        }
        self.shown = Some(key);
        self.generation = generation;
        let list: Vec<Marker> =
            answer["markers"].as_array().into_iter().flatten().filter_map(Marker::from_json).collect();
        if self.menu.as_ref().is_some_and(|menu| !list.contains(&menu.marker)) {
            self.menu = None;
        }
        self.list = Arc::new(list);
    }

    pub(super) fn clear(&mut self) {
        self.shown = None;
        self.list = Arc::default();
        self.menu = None;
    }

    pub(super) fn shown(&self, path: &Path) -> Arc<Vec<Marker>> {
        match &self.shown {
            Some((shown, _)) if shown == path => Arc::clone(&self.list),
            _ => Arc::default(),
        }
    }

    pub(super) fn paint(
        ui: &Ui,
        painter: &egui::Painter,
        id: egui::Id,
        icon: Rect,
        marker: &Marker,
        palette: &IdePalette,
    ) -> bool {
        let response = ui.interact(icon, id.with(("line-marker", marker.line)), Sense::click());
        accessibility::button_with(&response, || marker.tooltip.clone());
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            painter.rect_filled(icon.expand(1.0), 3, palette.hover);
        }
        glyph(painter, icon.shrink(2.0), marker.bean, palette.run);
        response.on_hover_text(marker.tooltip.clone()).clicked()
    }

    pub(super) fn clicked(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        path: &Path,
        marker: Marker,
        at: egui::Pos2,
    ) {
        if marker.targets.len() > 1 {
            self.menu = Some(Menu { path: path.to_owned(), marker, at, fresh: true });
            return;
        }
        let params = json!({ "path": path, "line": marker.line + 1 });
        if let Err(zen_core::DispatchError::Failed { error, .. }) =
            registry.dispatch(session, "editor.lineMarkers.navigate", params)
        {
            self.notice = Some(error.message);
        }
    }

    pub(super) fn menu(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, palette: &IdePalette) {
        const ROW: f32 = 24.0;
        let Some(menu) = &mut self.menu else { return };
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.menu = None;
            return;
        }
        let fresh = std::mem::take(&mut menu.fresh);
        let rows: Vec<(String, String)> = menu
            .marker
            .targets
            .iter()
            .map(|target| {
                let file = target.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                (target.name.clone(), format!("{}  {file}:{}", target.detail, target.line))
            })
            .collect();
        let mut chosen = None;
        let area = egui::Area::new(ui.scope_id().with("line-marker-menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(menu.at)
            .show(ui.ctx(), |ui| {
                super::doc_popups::popup_frame(palette).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let width = rows
                        .iter()
                        .map(|(name, detail)| {
                            let measure = |text: &str| {
                                ui.painter()
                                    .layout_no_wrap(text.to_owned(), crate::theme::regular(13.0), palette.text)
                                    .size()
                                    .x
                            };
                            measure(name) + measure(detail) + 60.0
                        })
                        .fold(220.0_f32, f32::max);
                    for (index, (name, detail)) in rows.iter().enumerate() {
                        let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
                        accessibility::name(&response, egui::Role::MenuItem, name);
                        if response.hovered() {
                            ui.painter().rect_filled(rect, 4, palette.selection);
                        }
                        let at = Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(11.0));
                        glyph(ui.painter(), at, !menu.marker.bean, palette.run);
                        ui.painter().text(
                            pos2(rect.left() + 28.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            name,
                            crate::theme::regular(13.0),
                            palette.text,
                        );
                        ui.painter().text(
                            pos2(rect.right() - 10.0, rect.center().y),
                            Align2::RIGHT_CENTER,
                            detail,
                            crate::theme::regular(13.0),
                            palette.text_secondary,
                        );
                        if response.clicked() {
                            chosen = Some(index);
                        }
                    }
                });
            });
        match chosen {
            Some(index) => {
                let (path, line) = (menu.path.clone(), menu.marker.line + 1);
                self.menu = None;
                let params = json!({ "path": path, "line": line, "index": index });
                if let Err(zen_core::DispatchError::Failed { error, .. }) =
                    registry.dispatch(session, "editor.lineMarkers.navigate", params)
                {
                    self.notice = Some(error.message);
                }
            }
            None if !fresh && area.response.clicked_elsewhere() => self.menu = None,
            None => {}
        }
    }

    pub(super) fn menu_open(&self) -> bool {
        self.menu.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spring_project() -> (Registry, Session, tempfile::TempDir, PathBuf) {
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let java = root.join("src/main/java/shop");
        std::fs::create_dir_all(&java).unwrap();
        std::fs::write(root.join("pom.xml"), "<project/>").unwrap();
        std::fs::write(java.join("Repo.java"), "@Repository\nclass Repo {}\n").unwrap();
        std::fs::write(java.join("Shop.java"), "@Service\nclass Shop {\n  Shop(Repo repo) {}\n}\n").unwrap();
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/spring-boot/plugin.json");
        let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
        let mut session = Session::new();
        session.state_mut::<zen_workbench::plugin_projects::PluginProjects>().set(vec![
            zen_workbench::plugin_projects::PluginProject {
                id: "spring-boot".to_owned(),
                name: "Spring Boot".to_owned(),
                support: zen_project::support::ProjectSupport::parse(&manifest["project"].to_string()).unwrap(),
            },
        ]);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
        (registry, session, dir, java)
    }

    #[test]
    fn a_mark_with_one_target_goes_there_at_once() {
        let (registry, mut session, _dir, java) = spring_project();
        let shop = java.join("Shop.java");
        registry.dispatch(&mut session, "editor.lineMarkers.list", json!({ "path": shop, "waitMs": 60_000 })).unwrap();
        let mut markers = LineMarkers::default();
        markers.follow(&registry, &mut session, &shop, 0, 0.0);
        let shown = markers.shown(&shop);
        let lines: Vec<(usize, bool)> = shown.iter().map(|marker| (marker.line, marker.bean)).collect();
        assert_eq!(lines, [(1, true), (2, false)]);
        assert!(markers.shown(&java.join("Repo.java")).is_empty(), "the marks are of the file shown");
        markers.clicked(&registry, &mut session, &shop, shown[1].clone(), egui::Pos2::ZERO);
        assert!(!markers.menu_open() && markers.notice.is_none());
        let active = session.state::<zen_workbench::Workbench>().unwrap().active_document().unwrap().path().to_owned();
        assert_eq!(active, java.join("Repo.java"));
        markers.clicked(&registry, &mut session, &shop, shown[0].clone(), egui::Pos2::ZERO);
        assert!(markers.notice.is_some(), "a bean injected nowhere says so");
        let mut two = shown[1].clone();
        two.targets.push(two.targets[0].clone());
        markers.clicked(&registry, &mut session, &shop, two, egui::Pos2::ZERO);
        assert!(markers.menu_open(), "several targets open a menu to choose from");
    }
}
