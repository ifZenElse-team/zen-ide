use egui::Ui;
use zen_i18n::t;
use zen_workbench::caffeine::{Activity, Assertion, Length, MAX_MINUTES, PRESETS};

use super::{SettingsDialog, widgets};
use crate::ide::IdePalette;
use crate::ide::caffeine::{activity_name, assertion_name, time_left};

fn lengths(custom: u32) -> Vec<Length> {
    let mut lengths = vec![Length::Indefinitely];
    lengths.extend(PRESETS.iter().map(|minutes| Length::Minutes { minutes: *minutes }));
    if !PRESETS.contains(&custom) {
        lengths.push(Length::Minutes { minutes: custom });
    }
    lengths.push(Length::While);
    lengths
}

fn length_name(length: Length) -> String {
    match length {
        Length::Indefinitely | Length::Minutes { minutes: 0 } => t!("ide-caffeine-for-ever-item"),
        Length::Minutes { minutes } if PRESETS.contains(&minutes) => time_left(u64::from(minutes) * 60),
        Length::Minutes { minutes } => t!("settings-caffeine-length-custom", minutes = minutes),
        Length::While => t!("ide-caffeine-while-item"),
    }
}

fn note(ui: &mut Ui, p: &IdePalette, text: String) {
    ui.horizontal(|ui| {
        ui.add_space(24.0);
        ui.add(egui::Label::new(egui::RichText::new(text).color(p.text_secondary)).wrap());
    });
}

impl SettingsDialog {
    pub(super) fn caffeine_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        let caffeine = &mut draft.caffeine;
        ui.add(egui::Label::new(egui::RichText::new(t!("settings-caffeine-about")).color(p.text_secondary)).wrap());
        ui.add_space(4.0);
        ui.add(egui::Label::new(egui::RichText::new(t!("settings-caffeine-defaults")).color(p.text_secondary)).wrap());
        ui.add_space(12.0);

        widgets::group_title(ui, p, &t!("settings-caffeine-what"), query);
        for assertion in Assertion::ALL {
            let mut on = caffeine.what.contains(&assertion);
            if widgets::checkbox(ui, p, &mut on, &assertion_name(assertion), query).changed() {
                caffeine.what.retain(|kind| *kind != assertion);
                if on {
                    caffeine.what.push(assertion);
                }
                caffeine.what.sort();
            }
        }
        note(ui, p, t!("settings-caffeine-lid"));
        ui.add_space(12.0);

        widgets::group_title(ui, p, &t!("settings-caffeine-length"), query);
        ui.horizontal_wrapped(|ui| {
            let combo = egui::ComboBox::from_id_salt("settings-caffeine-length")
                .selected_text(length_name(caffeine.length))
                .width(widgets::field_width(ui, 140.0, 240.0))
                .show_ui(ui, |ui| {
                    for length in lengths(caffeine.custom_minutes) {
                        ui.selectable_value(&mut caffeine.length, length, length_name(length));
                    }
                });
            crate::accessibility::name(&combo.response, egui::Role::ComboBox, &t!("settings-caffeine-length"));
        });
        ui.add_space(4.0);
        let before = caffeine.custom_minutes;
        widgets::number_field(
            ui,
            p,
            &t!("settings-caffeine-custom-minutes"),
            query,
            &mut caffeine.custom_minutes,
            1..=MAX_MINUTES,
        );
        if caffeine.length == (Length::Minutes { minutes: before }) && !PRESETS.contains(&before) {
            caffeine.length = Length::Minutes { minutes: caffeine.custom_minutes };
        }
        ui.add_space(12.0);

        widgets::group_title(ui, p, &t!("settings-caffeine-while"), query);
        for activity in Activity::ALL {
            let mut on = caffeine.during.contains(&activity);
            if widgets::checkbox(ui, p, &mut on, &activity_name(activity), query).changed() {
                caffeine.during.retain(|kind| *kind != activity);
                if on {
                    caffeine.during.push(activity);
                }
                caffeine.during.sort();
            }
        }
        ui.add_space(12.0);

        widgets::group_title(ui, p, &t!("settings-caffeine-battery"), query);
        widgets::checkbox(ui, p, &mut caffeine.off_on_battery, &t!("ide-caffeine-off-on-battery"), query);
        ui.add_space(4.0);
        widgets::checkbox(ui, p, &mut caffeine.low_battery, &t!("ide-caffeine-low-battery"), query);
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.add_enabled_ui(caffeine.low_battery, |ui| {
                widgets::number_field(
                    ui,
                    p,
                    &t!("settings-caffeine-low-level"),
                    query,
                    &mut caffeine.low_battery_level,
                    5..=95,
                );
            });
        });
        ui.add_space(4.0);
        widgets::checkbox(ui, p, &mut caffeine.resume_on_charge, &t!("ide-caffeine-resume"), query);
        ui.add_space(12.0);

        widgets::group_title(ui, p, &t!("settings-caffeine-launch"), query);
        widgets::checkbox(ui, p, &mut caffeine.start_on_launch, &t!("settings-caffeine-start-on-launch"), query);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lengths_offer_the_presets_a_custom_time_and_while() {
        assert_eq!(lengths(15).len(), PRESETS.len() + 2, "15 minutes is a preset");
        let custom = lengths(90);
        assert_eq!(custom.len(), PRESETS.len() + 3);
        assert_eq!(custom.first(), Some(&Length::Indefinitely));
        assert_eq!(custom.last(), Some(&Length::While));
        assert!(custom.contains(&Length::Minutes { minutes: 90 }));
    }
}
