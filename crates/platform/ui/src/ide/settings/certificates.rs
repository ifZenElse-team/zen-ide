use egui::Ui;
use zen_i18n::t;
use zen_net::trust;

use super::{SettingsDialog, widgets};
use crate::ide::IdePalette;

#[derive(Default)]
pub(super) struct CertificatesPage {
    list: Option<Vec<Vec<u8>>>,
    changed: bool,
    selected: Option<usize>,
    error: Option<String>,
}

impl CertificatesPage {
    #[cfg(test)]
    pub(super) fn set_list(&mut self, list: Vec<Vec<u8>>) {
        self.list = Some(list);
    }

    #[cfg(test)]
    pub(super) fn select(&mut self, index: usize) {
        self.selected = Some(index);
    }

    #[cfg(test)]
    pub(super) fn list(&self) -> &[Vec<u8>] {
        self.list.as_deref().unwrap_or_default()
    }

    pub(super) fn commit(&mut self) {
        if !std::mem::take(&mut self.changed) {
            return;
        }
        if let Some(list) = self.list.clone()
            && let Err(error) = zen_workbench::network::certificates::set_accepted(list)
        {
            self.error = Some(error.to_string());
        }
    }

    fn add(&mut self, path: &std::path::Path) {
        match zen_workbench::network::certificates::read_file(path) {
            Ok(found) => {
                let list = self.list.get_or_insert_with(trust::accepted);
                for der in found {
                    if !list.contains(&der) {
                        list.push(der);
                        self.changed = true;
                    }
                }
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_certificate() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title(t!("settings-certificates-add"))
        .add_filter(t!("settings-certificates-files"), &["crt", "cer", "pem"])
        .pick_file()
}

#[cfg(target_arch = "wasm32")]
fn pick_certificate() -> Option<std::path::PathBuf> {
    None
}

impl SettingsDialog {
    pub(super) fn certificates_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        let label = t!("settings-certificates-auto");
        widgets::checkbox(ui, p, &mut draft.accept_untrusted_certificates, &label, query);
        if draft.accept_untrusted_certificates {
            ui.add_space(4.0);
            let warning = egui::RichText::new(t!("settings-certificates-auto-warning")).color(p.error_text);
            ui.add(egui::Label::new(warning).wrap());
        }
        ui.add_space(12.0);
        let page = &mut self.certificates;
        ui.horizontal(|ui| {
            ui.label(widgets::label_job(ui, p, &t!("settings-certificates-accepted"), query));
            let add = ui.small_button("+");
            crate::accessibility::name(&add, egui::Role::Button, &t!("settings-certificates-add"));
            if add.clicked()
                && let Some(path) = pick_certificate()
            {
                page.add(&path);
            }
            let remove = ui.add_enabled(page.selected.is_some(), egui::Button::new("−").small());
            crate::accessibility::name(&remove, egui::Role::Button, &t!("settings-certificates-remove"));
            if remove.clicked()
                && let Some(index) = page.selected.take()
                && let Some(list) = &mut page.list
                && index < list.len()
            {
                list.remove(index);
                page.changed = true;
            }
        });
        ui.add_space(6.0);
        let list = page.list.get_or_insert_with(trust::accepted);
        if list.is_empty() {
            ui.label(egui::RichText::new(t!("settings-certificates-empty")).color(p.text_secondary));
        }
        let infos: Vec<_> = list.iter().map(|der| trust::describe(der)).collect();
        for (index, info) in infos.iter().enumerate() {
            let name = info.as_ref().map_or_else(|| "?".to_owned(), |info| info.subject.clone());
            if ui.selectable_label(page.selected == Some(index), name).clicked() {
                page.selected = Some(index);
            }
        }
        if let Some(info) = page.selected.and_then(|index| infos.get(index)).and_then(Option::as_ref) {
            ui.add_space(10.0);
            for (key, value) in [
                ("settings-certificates-issued-to", &info.subject),
                ("settings-certificates-issued-by", &info.issuer),
                ("settings-certificates-valid-from", &info.not_before),
                ("settings-certificates-valid-to", &info.not_after),
                ("settings-certificates-fingerprint", &info.fingerprint),
            ] {
                ui.label(widgets::label_job(ui, p, &t!(key), query));
                ui.add(egui::Label::new(egui::RichText::new(value).color(p.text_secondary)).wrap());
                ui.add_space(4.0);
            }
        }
        let pinned = trust::pinned();
        if !pinned.is_empty() {
            ui.add_space(12.0);
            ui.label(widgets::label_job(ui, p, &t!("settings-certificates-for-hosts"), query));
            for pin in pinned {
                let subject = trust::describe(&pin.der).map_or_else(|| "?".to_owned(), |info| info.subject);
                ui.horizontal(|ui| {
                    let remove = ui.small_button("−");
                    crate::accessibility::name(&remove, egui::Role::Button, &t!("settings-certificates-remove"));
                    if remove.clicked() {
                        page.error = zen_workbench::network::certificates::remove(&trust::fingerprint(&pin.der));
                    }
                    ui.label(format!("{}: {subject}", pin.host));
                });
            }
        }
        if let Some(error) = &page.error {
            ui.add_space(8.0);
            ui.add(egui::Label::new(egui::RichText::new(error).color(p.error_text)).wrap());
        }
    }
}
