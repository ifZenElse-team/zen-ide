use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use egui::{Align2, Rect, Stroke, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_db::diagram::{Colors, ErDiagram, Layout};
use zen_i18n::t;
use zen_workbench::database::diagrams::{self, Built, Diagrams, Renderer, View};
use zen_workbench::workspaces::InnerTab;

use super::super::{IdePalette, theme, vcs};
use super::DatabaseView;
use super::object::{button, centered, width_of};
use crate::accessibility;
use crate::diagram::{Camera, Edge, End, Interaction, Marker, Node, Row, Scene};

const BAR: f32 = 34.0;

#[derive(Default)]
pub(super) struct DiagramView {
    cameras: HashMap<String, (Camera, View)>,
    scenes: HashMap<String, (Arc<Built>, Arc<Scene>)>,
    exporting: HashSet<String>,
    message: HashMap<String, (String, bool)>,
}

pub(crate) fn scene_of(diagram: &ErDiagram, layout: &Layout) -> Scene {
    let metrics = &layout.metrics;
    let nodes = diagram
        .entities
        .iter()
        .zip(&layout.boxes)
        .zip(&layout.rows)
        .map(|((entity, bounds), rows)| Node {
            rect: Rect::from_min_size(pos2(bounds.x, bounds.y), vec2(bounds.width, bounds.height)),
            title: entity.label(),
            rows: entity
                .columns
                .iter()
                .take(*rows)
                .map(|column| Row {
                    text: column.name.clone(),
                    detail: column.type_name.clone(),
                    marker: if column.primary {
                        Marker::Key
                    } else if column.foreign {
                        Marker::ForeignKey
                    } else {
                        Marker::None
                    },
                })
                .collect(),
            more: (entity.columns.len() > *rows)
                .then(|| t!("ide-database-diagram-more", count = entity.columns.len() - rows)),
            dashed: entity.kind != "table",
            muted: entity.external,
        })
        .collect();
    let edges = diagram
        .relations
        .iter()
        .zip(&layout.edges)
        .map(|(relation, points)| Edge {
            points: points.iter().map(|point| pos2(point[0], point[1])).collect(),
            from: relation.from,
            to: relation.to,
            start: if relation.unique { End::One } else { End::Many },
            end: if relation.optional { End::ZeroOrOne } else { End::One },
            dashed: false,
        })
        .collect();
    Scene {
        nodes,
        edges,
        size: vec2(layout.width, layout.height),
        header: metrics.header,
        row: metrics.row,
        padding: metrics.padding,
    }
}

fn camera_of(view: View) -> Camera {
    Camera { zoom: view.zoom as f32 / 100.0, origin: pos2(view.x as f32, view.y as f32) }
}

fn view_of(camera: &Camera) -> View {
    View {
        zoom: (camera.zoom * 100.0).round() as u32,
        x: camera.origin.x.round() as i32,
        y: camera.origin.y.round() as i32,
    }
}

fn rgba(color: egui::Color32) -> [u8; 4] {
    color.to_srgba_unmultiplied()
}

pub(crate) struct ThemeRenderer(pub(crate) Arc<dyn Fn() -> IdePalette + Send + Sync>);

impl Renderer for ThemeRenderer {
    fn colors(&self) -> Colors {
        let [background, fill, header, border, text, secondary, line, key, foreign] =
            crate::diagram::export_colors(&(self.0)()).map(rgba);
        Colors { background, fill, header, border, text, secondary, line, key, foreign }
    }

    fn png(&self, diagram: &ErDiagram, layout: &Layout, scale: f32, path: &Path) -> Result<(), String> {
        let image = crate::diagram::render(&scene_of(diagram, layout), &(self.0)(), scale);
        crate::offscreen::write_png(path, &image)
    }
}

fn save_dialog(format: &str, name: &str) -> Option<PathBuf> {
    let (extension, title) = match format {
        "png" => ("png", t!("ide-database-diagram-export-png")),
        "svg" => ("svg", t!("ide-database-diagram-export-svg")),
        _ => ("mmd", t!("ide-database-diagram-export-mermaid")),
    };
    rfd::FileDialog::new()
        .set_title(title)
        .set_file_name(format!("{name}.{extension}"))
        .add_filter(extension.to_uppercase(), &[extension])
        .save_file()
}

impl DatabaseView {
    pub(super) fn diagram_tab(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        tab: &InnerTab,
    ) {
        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, 0, p.island);
        let mut child =
            ui.new_child(UiBuilder::new().id_salt(("database-diagram-tab", tab.id.as_str())).max_rect(area));
        self.diagram_body(&mut child, p, registry, session, &tab.id);
    }

    pub(super) fn diagram_body(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        tab: &str,
    ) {
        let area = ui.max_rect();
        if let Err(error) = diagrams::refresh(session, tab) {
            centered(ui, p, area, &error.to_string());
            return;
        }
        let built = session.state_mut::<Diagrams>().shown(tab);
        let computing = session.state::<Diagrams>().is_some_and(|diagrams| diagrams.computing(tab));
        let bar = Rect::from_min_size(area.min, vec2(area.width(), BAR));
        let canvas = Rect::from_min_max(pos2(area.left(), bar.bottom()), area.max);
        self.diagram_bar(ui, p, registry, session, (tab, bar, canvas), (built.as_ref(), computing));
        let Some(built) = built else {
            let text = if computing { t!("ide-database-diagram-computing") } else { t!("ide-database-diagram-empty") };
            ui.painter().rect_filled(canvas, 0, p.editor_background);
            centered(ui, p, canvas, &text);
            if computing {
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
            return;
        };
        if computing {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        let scene = match self.diagram.scenes.get(tab) {
            Some((known, scene)) if Arc::ptr_eq(known, &built) => Arc::clone(scene),
            _ => {
                let scene = Arc::new(scene_of(&built.diagram, &built.layout));
                self.diagram.scenes.insert(tab.to_owned(), (Arc::clone(&built), Arc::clone(&scene)));
                scene
            }
        };
        let stored = diagrams::view_of(session, tab).unwrap_or_default();
        let entry = self.diagram.cameras.entry(tab.to_owned()).or_insert((camera_of(stored), stored));
        if entry.1 != stored {
            *entry = (camera_of(stored), stored);
        }
        let selected_name = session.state::<Diagrams>().and_then(|diagrams| diagrams.selected(tab).map(str::to_owned));
        let selected = selected_name.and_then(|name| built.diagram.find(&name));
        let mut camera = entry.0;
        let interaction: Interaction =
            crate::diagram::show(ui, p, canvas, (&scene, &mut camera, selected), &t!("ide-database-diagram-canvas"));
        let entry = self.diagram.cameras.entry(tab.to_owned()).or_insert((camera, stored));
        entry.0 = camera;
        if interaction.moved {
            let view = view_of(&camera);
            if view != entry.1 {
                entry.1 = view;
                let _ = registry.dispatch(
                    session,
                    "db.diagram.view",
                    json!({ "tab": tab, "zoom": view.zoom, "x": view.x, "y": view.y }),
                );
            }
        }
        let name = |at: usize| built.diagram.entities[at].label();
        if let Some(at) = interaction.opened {
            self.diagram_say(
                tab,
                registry.dispatch(
                    session,
                    "db.diagram.select",
                    json!({ "tab": tab, "entity": name(at), "open": true }),
                ),
            );
        } else if let Some(at) = interaction.clicked {
            self.diagram_say(
                tab,
                registry.dispatch(session, "db.diagram.select", json!({ "tab": tab, "entity": name(at) })),
            );
        } else if interaction.cleared && selected.is_some() {
            self.diagram_say(tab, registry.dispatch(session, "db.diagram.select", json!({ "tab": tab })));
        }
        if let Some(at) = interaction.hovered {
            let entity = &built.diagram.entities[at];
            let tip = format!("{}.{} · {}", entity.schema, entity.name, t!("ide-database-diagram-open-hint"));
            egui::Tooltip::always_open(
                ui.ctx().clone(),
                ui.layer_id(),
                ui.scope_id().with("diagram-tip"),
                egui::PopupAnchor::Pointer,
            )
            .gap(12.0)
            .show(|ui| ui.label(tip));
        }
    }

    fn diagram_say(&mut self, tab: &str, answer: Result<Value, zen_core::DispatchError>) {
        match answer {
            Ok(_) => {
                self.diagram.message.remove(tab);
            }
            Err(error) => {
                self.diagram.message.insert(tab.to_owned(), (super::message(error), true));
            }
        }
    }

    fn diagram_bar(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        (tab, bar, canvas): (&str, Rect, Rect),
        (built, computing): (Option<&Arc<Built>>, bool),
    ) {
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, p.frame));
        let font = theme::regular(12.0);
        let height = bar.height() - 10.0;
        let mut x = bar.left() + 8.0;
        let place = |ui: &Ui, x: &mut f32, label: &str| {
            let width = width_of(ui, label, &font) + 18.0;
            let rect = Rect::from_min_size(pos2(*x, bar.top() + 5.0), vec2(width, height));
            *x = rect.right() + 4.0;
            rect
        };
        let size = json!({ "tab": tab, "width": canvas.width(), "height": canvas.height() });
        let zoom = |action: &str| {
            let mut params = size.clone();
            params["action"] = Value::from(action);
            params
        };
        let mut chosen = None;
        let ready = built.is_some();
        let shown = self.diagram.cameras.get(tab).map_or(100, |(camera, _)| (camera.zoom * 100.0).round() as u32);
        let percent = format!("{shown}%");
        for (label, action, tip) in [
            ("−", "out", t!("ide-database-diagram-zoom-out")),
            ("+", "in", t!("ide-database-diagram-zoom-in")),
            (percent.as_str(), "reset", t!("ide-database-diagram-actual-size")),
        ] {
            let rect = place(ui, &mut x, label);
            if button(ui, p, rect, label, action).on_hover_text(tip).clicked() {
                chosen = Some(zoom(action));
            }
        }
        let fit_rect = Rect::from_min_size(pos2(x, bar.top() + 5.0), vec2(height, height));
        x = fit_rect.right() + 4.0;
        let fit = button(ui, p, fit_rect, "", "fit").on_hover_text(t!("ide-database-diagram-fit"));
        let glyph = Rect::from_center_size(fit_rect.center(), vec2(14.0, 14.0));
        super::super::image_view::glyph_fit(ui.painter(), glyph, if ready { p.text } else { p.text_secondary });
        if fit.clicked() && ready {
            chosen = Some(zoom("fit"));
        }
        let export_label = t!("ide-database-diagram-export");
        let export_rect = place(ui, &mut x, &export_label).with_max_x(x + 10.0);
        x = export_rect.right() + 4.0;
        let export = button(ui, p, export_rect, "", "export");
        accessibility::button_with(&export, || export_label.clone());
        ui.painter().text(
            pos2(export_rect.left() + 9.0, export_rect.center().y),
            Align2::LEFT_CENTER,
            &export_label,
            font.clone(),
            p.text,
        );
        let chevron =
            Rect::from_center_size(pos2(export_rect.right() - 12.0, export_rect.center().y), vec2(12.0, 12.0));
        crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
        let mut format = None;
        egui::Popup::menu(&export).show(|ui| {
            ui.set_min_width(200.0);
            for (key, label) in [
                ("png", t!("ide-database-diagram-export-png")),
                ("svg", t!("ide-database-diagram-export-svg")),
                ("mermaid", t!("ide-database-diagram-export-mermaid")),
                ("copy", t!("ide-database-diagram-copy-mermaid")),
            ] {
                if key == "copy" {
                    ui.separator();
                }
                if ui.add_enabled(ready || key == "mermaid" || key == "copy", egui::Button::new(label)).clicked() {
                    format = Some(key);
                    ui.close();
                }
            }
        });
        if let Some(params) = chosen {
            let answer = registry.dispatch(session, "db.diagram.zoom", params);
            if let Ok(view) = &answer
                && let Ok(view) = serde_json::from_value::<View>(view.clone())
            {
                self.diagram.cameras.insert(tab.to_owned(), (camera_of(view), view));
            }
            self.diagram_say(tab, answer);
        }
        if let Some(format) = format {
            self.diagram_export(ui, registry, session, tab, format);
        }
        self.diagram_status(ui, p, registry, session, (tab, bar, x), (built, computing));
    }

    fn diagram_export(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, tab: &str, format: &str) {
        if format == "copy" {
            let answer = registry.dispatch(
                session,
                "db.diagram.export",
                json!({ "tab": tab, "format": "mermaid", "limit": usize::MAX }),
            );
            if let Ok(answer) = &answer
                && let Some(text) = answer["text"].as_str()
            {
                ui.ctx().copy_text(text.to_owned());
                self.diagram.message.insert(tab.to_owned(), (t!("ide-database-diagram-copied"), false));
                return;
            }
            self.diagram_say(tab, answer);
            return;
        }
        let name = session
            .state::<zen_workbench::workspaces::Workspaces>()
            .and_then(|workspaces| {
                workspaces
                    .tabs(zen_workbench::database::WORKSPACE)
                    .iter()
                    .find(|known| known.id == tab)
                    .map(|known| known.title.clone())
            })
            .unwrap_or_else(|| "diagram".to_owned());
        let Some(path) = save_dialog(format, &name) else { return };
        let answer = registry.dispatch(
            session,
            "db.diagram.export",
            json!({ "tab": tab, "format": format, "path": path, "overwrite": true }),
        );
        if answer.is_ok() {
            self.diagram.exporting.insert(tab.to_owned());
            self.diagram.message.remove(tab);
        } else {
            self.diagram_say(tab, answer);
        }
    }

    fn diagram_status(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        (tab, bar, left): (&str, Rect, f32),
        (built, computing): (Option<&Arc<Built>>, bool),
    ) {
        if self.diagram.exporting.contains(tab) {
            let state =
                registry.dispatch(session, "db.diagram.state", json!({ "tab": tab, "limit": 0, "columns": false }));
            let export = state.as_ref().ok().map(|state| state["export"].clone()).unwrap_or(Value::Null);
            match export["status"].as_str() {
                Some("running") => {
                    self.diagram.message.insert(tab.to_owned(), (t!("ide-database-diagram-exporting"), false));
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                }
                Some("done") => {
                    let path = export["path"].as_str().unwrap_or_default();
                    self.diagram
                        .message
                        .insert(tab.to_owned(), (t!("ide-database-diagram-exported", path = path), false));
                    self.diagram.exporting.remove(tab);
                }
                _ => {
                    let error = export["error"].as_str().unwrap_or_default().to_owned();
                    self.diagram
                        .message
                        .insert(tab.to_owned(), (t!("ide-database-diagram-export-failed", error = error), true));
                    self.diagram.exporting.remove(tab);
                }
            }
        }
        let mut right = bar.right() - 10.0;
        if computing {
            let label = t!("ide-database-diagram-cancel");
            let width = width_of(ui, &label, &theme::regular(12.0)) + 18.0;
            let rect = Rect::from_min_size(pos2(right - width, bar.top() + 5.0), vec2(width, bar.height() - 10.0));
            if button(ui, p, rect, &label, "cancel").clicked() {
                let answer = registry.dispatch(session, "db.diagram.cancel", json!({ "tab": tab }));
                self.diagram_say(tab, answer);
            }
            right = rect.left() - 8.0;
        }
        let counts = built.map(|built| {
            let diagram = &built.diagram;
            let mut text = t!(
                "ide-database-diagram-counts",
                entities = diagram.entities.len(),
                relations = diagram.relations.len()
            );
            if diagram.partial() {
                text = format!("{text} · {}", t!("ide-database-diagram-partial"));
            }
            text
        });
        let (text, error) = match self.diagram.message.get(tab) {
            Some((message, error)) => (Some(message.clone()), *error),
            None if computing => (Some(t!("ide-database-diagram-computing")), false),
            None => (counts, false),
        };
        let Some(text) = text else { return };
        let color = if error { p.error_text } else { p.text_secondary };
        let job = vcs::truncated(&text, theme::regular(12.0), color, (right - left - 8.0).max(40.0));
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        ui.painter().galley(pos2(right - galley.size().x, bar.center().y - galley.size().y / 2.0), galley, color);
    }
}

#[cfg(test)]
mod tests;
