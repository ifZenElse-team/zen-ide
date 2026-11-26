use egui::{Color32, Stroke, Ui, vec2};
use zen_i18n::t;
use zen_net::trust::Untrusted;

use super::{IdePalette, dialog_frame};
use crate::theme;

pub(super) fn show(ui: &mut Ui, p: &IdePalette, untrusted: &Untrusted) -> Option<bool> {
    let mut choice = None;
    let certificate = &untrusted.certificate;
    let modal =
        egui::Modal::new(ui.scope_id().with("untrusted-certificate")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(460.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            let title = egui::RichText::new(t!("ide-certificate-title")).font(theme::semibold(15.0)).color(p.text);
            ui.add(egui::Label::new(title).wrap());
            ui.add_space(10.0);
            let text = t!("ide-certificate-text", host = untrusted.host.as_str());
            ui.add(egui::Label::new(egui::RichText::new(text).font(theme::regular(13.0)).color(p.text)).wrap());
            ui.add_space(10.0);
            for (key, value) in [
                ("settings-certificates-issued-to", &certificate.subject),
                ("settings-certificates-issued-by", &certificate.issuer),
                ("settings-certificates-valid-from", &certificate.not_before),
                ("settings-certificates-valid-to", &certificate.not_after),
                ("settings-certificates-fingerprint", &certificate.fingerprint),
            ] {
                let line = format!("{} {value}", t!(key));
                ui.add(
                    egui::Label::new(egui::RichText::new(line).font(theme::regular(12.0)).color(p.text_secondary))
                        .wrap(),
                );
                ui.add_space(3.0);
            }
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let reject = egui::Button::new(
                    egui::RichText::new(t!("ide-certificate-reject")).font(theme::regular(13.0)).color(p.accent_text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(p.accent);
                if ui.add(reject).clicked() {
                    choice = Some(false);
                }
                let accept = egui::Button::new(
                    egui::RichText::new(t!("ide-certificate-accept")).font(theme::regular(13.0)).color(p.text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, p.button_border));
                if ui.add(accept).clicked() {
                    choice = Some(true);
                }
            });
        });
    choice.or_else(|| modal.should_close().then_some(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zen_net::trust::CertificateInfo;

    #[test]
    fn the_dialog_waits_for_an_answer_and_escape_rejects() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let untrusted = Untrusted {
            host: "example.com".to_owned(),
            certificate: CertificateInfo {
                subject: "CN=example.com".to_owned(),
                issuer: "CN=Corporate Root".to_owned(),
                not_before: "2026-01-01".to_owned(),
                not_after: "2027-01-01".to_owned(),
                fingerprint: "ab".repeat(32),
                host: None,
            },
        };
        let palette = IdePalette::zen_dark();
        let mut screen = crate::offscreen::Offscreen::new(vec2(800.0, 600.0), 1.0);
        let mut choice = None;
        for _ in 0..3 {
            screen.step(Vec::new(), |ui| choice = choice.or(show(ui, &palette, &untrusted)));
        }
        assert_eq!(choice, None);
        let escape = egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        screen.step(vec![escape], |ui| choice = choice.or(show(ui, &palette, &untrusted)));
        assert_eq!(choice, Some(false));
    }
}
