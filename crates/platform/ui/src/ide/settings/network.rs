use std::sync::mpsc::{Receiver, channel};

use egui::Ui;
use zen_i18n::t;
use zen_net::{CheckError, ProxyKind, ProxyMode, ProxySettings};

use super::{SettingsDialog, widgets};
use crate::accessibility;
use crate::ide::IdePalette;

const SYSTEM_PROXY_SETTINGS: &str = "x-apple.systempreferences:com.apple.Network-Settings.extension?Proxies";

const FIELD_WIDTH: f32 = 260.0;

#[derive(Default)]
pub(super) struct ProxyPage {
    pub(super) password: Option<String>,
    pub(super) asking: Option<String>,
    pub(super) running: Option<Receiver<Result<u16, CheckError>>>,
    pub(super) answer: Option<Result<String, String>>,
}

impl ProxyPage {
    fn check(&mut self, settings: ProxySettings, url: String) {
        let password = self.password.clone().or_else(|| zen_net::password_for(&settings));
        let (sender, receiver) = channel();
        let spawned = std::thread::Builder::new().name("zen-proxy-check".to_owned()).spawn(move || {
            let _ = sender.send(zen_workbench::network::check_settings_asking(&settings, password.as_deref(), &url));
        });
        self.answer = None;
        self.running = spawned.is_ok().then_some(receiver);
    }

    fn poll(&mut self) {
        let Some(running) = &self.running else { return };
        if let Ok(answer) = running.try_recv() {
            self.running = None;
            self.answer = Some(match answer {
                Ok(status) => Ok(t!("settings-proxy-check-success", status = u32::from(status))),
                Err(error) => Err(zen_workbench::network::check_error(&error)),
            });
        }
    }
}

fn labelled(
    ui: &mut Ui,
    p: &IdePalette,
    label: &str,
    query: Option<&str>,
    add: impl FnOnce(&mut Ui) -> egui::Response,
) {
    ui.horizontal(|ui| {
        let title = ui.label(widgets::label_job(ui, p, label, query));
        ui.add_space((110.0 - title.rect.width()).max(4.0));
        add(ui).labelled_by(title.id);
    });
}

fn open_system_proxy_settings() {
    let _ = std::process::Command::new("open").arg(SYSTEM_PROXY_SETTINGS).spawn();
}

impl SettingsDialog {
    pub(super) fn proxy_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        self.proxy.poll();
        if self.proxy.running.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        let Some(draft) = self.draft.as_mut() else { return };
        let proxy = &mut draft.proxy;
        ui.radio_value(
            &mut proxy.mode,
            ProxyMode::NoProxy,
            widgets::label_job(ui, p, &t!("settings-proxy-none"), query),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let label = widgets::label_job(ui, p, &t!("settings-proxy-auto"), query);
            ui.radio_value(&mut proxy.mode, ProxyMode::AutoDetect, label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.link(t!("settings-proxy-system-link")).clicked() {
                    open_system_proxy_settings();
                }
            });
        });
        let auto = proxy.mode == ProxyMode::AutoDetect;
        ui.indent("proxy-auto", |ui| {
            ui.add_enabled_ui(auto, |ui| {
                let label = t!("settings-proxy-pac");
                widgets::checkbox(ui, p, &mut proxy.automatic_configuration, &label, query);
                ui.add_enabled_ui(proxy.automatic_configuration, |ui| {
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut proxy.automatic_configuration_url)
                            .desired_width(FIELD_WIDTH + 110.0),
                    );
                    accessibility::name(&field, egui::Role::TextInput, &label);
                    ui.label(egui::RichText::new(t!("settings-proxy-pac-example")).color(p.text_secondary));
                });
                if proxy.automatic_configuration && !proxy.automatic_configuration_url.trim().is_empty() {
                    ui.add(
                        egui::Label::new(egui::RichText::new(t!("settings-proxy-pac-unsupported")).color(p.error_text))
                            .wrap(),
                    );
                }
                if ui.button(t!("settings-proxy-clear-passwords")).clicked() {
                    self.proxy.password = None;
                    let _ = zen_workbench::network::clear_passwords(zen_secrets::system().as_ref());
                }
            });
        });
        ui.add_space(6.0);
        let label = widgets::label_job(ui, p, &t!("settings-proxy-manual"), query);
        ui.radio_value(&mut proxy.mode, ProxyMode::Manual, label);
        let manual = proxy.mode == ProxyMode::Manual;
        ui.indent("proxy-manual", |ui| {
            ui.add_enabled_ui(manual, |ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut proxy.kind, ProxyKind::Http, t!("settings-proxy-http"));
                    ui.radio_value(&mut proxy.kind, ProxyKind::Socks, t!("settings-proxy-socks"));
                });
                ui.add_space(4.0);
                labelled(ui, p, &t!("settings-proxy-host"), query, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut proxy.host).desired_width(FIELD_WIDTH))
                });
                labelled(ui, p, &t!("settings-proxy-port"), query, |ui| {
                    ui.add(egui::DragValue::new(&mut proxy.port).range(1..=65535))
                });
                ui.add_space(4.0);
                labelled(ui, p, &t!("settings-proxy-no-proxy-for"), query, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut proxy.no_proxy_for).desired_width(FIELD_WIDTH))
                });
                ui.label(egui::RichText::new(t!("settings-proxy-no-proxy-example")).color(p.text_secondary));
                ui.add_space(4.0);
                widgets::checkbox(ui, p, &mut proxy.authentication, &t!("settings-proxy-authentication"), query);
                ui.add_enabled_ui(proxy.authentication, |ui| {
                    labelled(ui, p, &t!("settings-proxy-login"), query, |ui| {
                        ui.add(egui::TextEdit::singleline(&mut proxy.login).desired_width(FIELD_WIDTH))
                    });
                    let password = self.proxy.password.get_or_insert_with(String::new);
                    labelled(ui, p, &t!("settings-proxy-password"), query, |ui| {
                        ui.add(egui::TextEdit::singleline(password).password(true).desired_width(FIELD_WIDTH))
                    });
                    if password.is_empty() {
                        self.proxy.password = None;
                    }
                    widgets::checkbox(ui, p, &mut proxy.remember, &t!("settings-proxy-remember"), query);
                });
            });
        });
        ui.add_space(12.0);
        let settings = proxy.clone();
        self.proxy_check_row(ui, p, settings);
    }

    fn proxy_check_row(&mut self, ui: &mut Ui, p: &IdePalette, settings: ProxySettings) {
        let page = &mut self.proxy;
        let mut start = None;
        if let Some(url) = &mut page.asking {
            let ask = t!("settings-proxy-check-ask");
            ui.label(&ask);
            let mut cancel = false;
            ui.horizontal(|ui| {
                let field = ui.add(egui::TextEdit::singleline(url).desired_width(FIELD_WIDTH));
                accessibility::name(&field, egui::Role::TextInput, &ask);
                if ui.button(t!("settings-ok")).clicked() {
                    start = Some(url.clone());
                }
                cancel = ui.button(t!("settings-cancel")).clicked();
            });
            if cancel || start.is_some() {
                page.asking = None;
            }
        } else if ui.add_enabled(page.running.is_none(), egui::Button::new(t!("settings-proxy-check"))).clicked() {
            page.asking = Some("https://".to_owned());
        }
        if let Some(url) = start {
            page.check(settings, url);
        }
        if page.running.is_some() {
            ui.spinner();
        }
        match &page.answer {
            Some(Ok(text)) => {
                ui.add(egui::Label::new(egui::RichText::new(text).color(p.text_secondary)).wrap());
            }
            Some(Err(text)) => {
                ui.add(egui::Label::new(egui::RichText::new(text).color(p.error_text)).wrap());
            }
            None => {}
        }
    }

    pub(super) fn keep_proxy_password(&mut self) {
        if let (Some(password), Some(draft)) = (self.proxy.password.take(), self.draft.as_ref()) {
            zen_workbench::network::keep_password(&draft.proxy, password);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_running_check_shows_its_answer_and_a_refused_script_says_so() {
        let mut page = ProxyPage::default();
        let settings = ProxySettings {
            automatic_configuration: true,
            automatic_configuration_url: "https://example.com/wpad.pac".to_owned(),
            ..ProxySettings::default()
        };
        page.check(settings, "http://127.0.0.1:9/".to_owned());
        while page.running.is_some() {
            page.poll();
            std::thread::yield_now();
        }
        let shown = page.answer.unwrap().unwrap_err();
        assert!(shown.contains("wpad.pac"), "{shown}");
    }
}
