use egui::{Key, Stroke, Ui, vec2};
use zen_i18n::t;
use zen_terminal::links::LinkKind;
use zen_workbench::terminal::LinkAction;

use super::{IdePalette, dialog_frame};
use crate::terminal::LinkClick;
use crate::theme;

const WIDTH: f32 = 300.0;

const SHOWN_CHARS: usize = 56;

#[derive(Clone, Debug)]
pub(super) struct LinkDialog {
    pub(super) session: u64,
    pub(super) click: LinkClick,
    pub(super) remember: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Act { action: LinkAction, remember: bool },
    Cancel,
}

pub(super) fn action_label(action: LinkAction) -> String {
    match action {
        LinkAction::OpenInBrowser => t!("ide-terminal-link-open-in-browser"),
        LinkAction::CopyLink => t!("ide-terminal-link-copy-link"),
        LinkAction::OpenInEditor => t!("ide-terminal-link-open-in-editor"),
        LinkAction::RevealInProject => t!("ide-terminal-link-reveal-in-project"),
        LinkAction::OpenInFinder if cfg!(target_os = "macos") => t!("ide-terminal-link-open-in-finder"),
        LinkAction::OpenInFinder if cfg!(target_os = "windows") => t!("ide-terminal-link-open-in-explorer"),
        LinkAction::OpenInFinder => t!("ide-terminal-link-open-in-files"),
        LinkAction::CopyPath => t!("ide-terminal-link-copy-path"),
    }
}

fn shortened(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit {
        return text.to_owned();
    }
    let head = (limit - 1) * 2 / 3;
    let tail = limit - 1 - head;
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(count - tail).collect();
    format!("{start}…{end}")
}

fn shown_link(click: &LinkClick) -> String {
    shortened(&click.command_target(), SHOWN_CHARS)
}

fn button(ui: &mut Ui, p: &IdePalette, label: String, primary: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(WIDTH, 30.0), egui::Sense::click());
    let role = if primary { egui::Role::DefaultButton } else { egui::Role::Button };
    crate::accessibility::name(&response, role, &label);
    let painter = ui.painter();
    let text = if primary {
        painter.rect_filled(rect, 6, p.accent);
        p.accent_text
    } else {
        if response.hovered() {
            painter.rect_filled(rect, 6, p.popup_hover);
        }
        painter.rect_stroke(rect.shrink(0.5), 6, Stroke::new(1.0, p.button_border), egui::StrokeKind::Inside);
        p.text
    };
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, label, theme::regular(13.0), text);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

pub(super) fn show(ui: &mut Ui, p: &IdePalette, product: (&str, &str), dialog: &mut LinkDialog) -> Option<Choice> {
    let mut choice = None;
    let actions = LinkAction::offered(dialog.click.kind);
    let modal = egui::Modal::new(ui.scope_id().with("terminal-link"))
        .frame(dialog_frame(p))
        .backdrop_color(p.popup_shadow)
        .show(ui.ctx(), |ui| {
            ui.set_width(WIDTH);
            ui.spacing_mut().item_spacing.y = 0.0;
            let (mark, _) = ui.allocate_exact_size(vec2(40.0, 40.0), egui::Sense::hover());
            crate::icons::product_mark(ui, mark, product.0, product.1, false);
            ui.add_space(12.0);
            let (title, about) = match dialog.click.kind {
                LinkKind::Url => {
                    (t!("ide-terminal-link-url-title"), t!("ide-terminal-link-url-about", product = product.1))
                }
                LinkKind::File => (t!("ide-terminal-link-file-title"), t!("ide-terminal-link-file-about")),
            };
            ui.add(egui::Label::new(egui::RichText::new(title).font(theme::bold(15.0)).color(p.text)).wrap());
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(egui::RichText::new(about).font(theme::regular(13.0)).color(p.text_secondary)).wrap(),
            );
            ui.add_space(4.0);
            let link = ui.add(
                egui::Label::new(
                    egui::RichText::new(shown_link(&dialog.click)).font(theme::regular(12.0)).color(p.text),
                )
                .wrap(),
            );
            link.on_hover_text(dialog.click.command_target());
            ui.add_space(12.0);
            ui.checkbox(
                &mut dialog.remember,
                egui::RichText::new(t!("ide-terminal-link-remember")).font(theme::regular(13.0)).color(p.text),
            );
            ui.add_space(14.0);
            for (index, action) in actions.iter().enumerate() {
                if index > 0 {
                    ui.add_space(8.0);
                }
                if button(ui, p, action_label(*action), index == 0).clicked() {
                    choice = Some(Choice::Act { action: *action, remember: dialog.remember });
                }
            }
            ui.add_space(8.0);
            if button(ui, p, t!("settings-cancel"), false).clicked() {
                choice = Some(Choice::Cancel);
            }
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                choice = Some(Choice::Act { action: actions[0], remember: dialog.remember });
            }
        });
    choice.or_else(|| modal.should_close().then_some(Choice::Cancel))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(kind: LinkKind, target: &str, line: Option<u32>, column: Option<u32>) -> LinkClick {
        LinkClick { kind, text: target.to_owned(), target: target.to_owned(), line, column, at: egui::Pos2::ZERO }
    }

    fn frame(ctx: &egui::Context, dialog: &mut LinkDialog, events: Vec<egui::Event>) -> Option<Choice> {
        let mut out = None;
        let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            out = show(ui, &IdePalette::zen_dark(), ("rustronin", "RustRonin"), dialog);
        });
        output.textures_delta.clear();
        out
    }

    fn key(key: Key) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    #[test]
    fn long_links_lose_their_middle() {
        assert_eq!(shortened("short", 10), "short");
        let cut = shortened(&"a".repeat(30).replace("aaaaaaaaaa", "0123456789"), 10);
        assert_eq!(cut.chars().count(), 10);
        assert!(cut.starts_with("012345") && cut.ends_with("789") && cut.contains('…'), "{cut}");
        let file = click(LinkKind::File, "/p/src/main.rs", Some(12), Some(5));
        assert_eq!(file.command_target(), "/p/src/main.rs:12:5");
        assert_eq!(click(LinkKind::File, "/p/a.rs", Some(3), None).command_target(), "/p/a.rs:3");
        assert_eq!(click(LinkKind::Url, "https://x.y", None, None).command_target(), "https://x.y");
    }

    #[test]
    fn enter_takes_the_default_action_and_escape_cancels() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut url =
            LinkDialog { session: 1, click: click(LinkKind::Url, "https://x.y", None, None), remember: false };
        assert_eq!(frame(&ctx, &mut url, Vec::new()), None, "it waits for the person");
        assert_eq!(
            frame(&ctx, &mut url, vec![key(Key::Enter)]),
            Some(Choice::Act { action: LinkAction::OpenInBrowser, remember: false })
        );
        let mut file =
            LinkDialog { session: 1, click: click(LinkKind::File, "/p/main.rs", Some(1), None), remember: true };
        assert_eq!(
            frame(&ctx, &mut file, vec![key(Key::Enter)]),
            Some(Choice::Act { action: LinkAction::OpenInEditor, remember: true })
        );
        frame(&ctx, &mut file, Vec::new());
        assert_eq!(frame(&ctx, &mut file, vec![key(Key::Escape)]), Some(Choice::Cancel));
    }

    #[test]
    fn every_action_has_a_label() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let labels: Vec<String> = [LinkKind::Url, LinkKind::File]
            .into_iter()
            .flat_map(|kind| LinkAction::offered(kind).iter().copied())
            .map(action_label)
            .collect();
        assert_eq!(labels.len(), 6);
        assert!(labels.iter().all(|label| !label.is_empty() && !label.starts_with("ide-")), "{labels:?}");
        assert!(labels.contains(&"Open in Browser".to_owned()) && labels.contains(&"Copy Path".to_owned()));
    }

    #[test]
    fn the_dialog_draws_over_the_terminal_in_light_and_dark() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(std::path::PathBuf::from);
        let lines = [
            "$ cargo build",
            "error[E0425]: cannot find value `x` in this scope",
            "  --> src/main.rs:12:5",
            "$ open https://doc.rust-lang.org/book/ch03-01-variables-and-mutability.html",
        ];
        for (theme_name, theme) in
            [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
        {
            let palette = IdePalette::from_theme(&theme);
            let terminal = crate::terminal::TerminalPalette::from_theme(&theme);
            for (shot, kind, target, line, column) in [
                ("terminal-link-url", LinkKind::Url, lines[3].trim_start_matches("$ open "), None, None),
                ("terminal-link-file", LinkKind::File, "/Users/me/hello/src/main.rs", Some(12), Some(5)),
            ] {
                let mut screen = crate::offscreen::Offscreen::new(vec2(720.0, 520.0), 2.0);
                palette.apply(screen.context());
                let mut dialog = LinkDialog { session: 1, click: click(kind, target, line, column), remember: false };
                let mut choice = None;
                let mut draw = |ui: &mut Ui| {
                    ui.painter().rect_filled(ui.max_rect(), 0, terminal.background);
                    for (row, text) in lines.iter().enumerate() {
                        let at = egui::pos2(12.0, 12.0 + row as f32 * 18.0);
                        ui.painter().text(
                            at,
                            egui::Align2::LEFT_TOP,
                            *text,
                            egui::FontId::monospace(13.0),
                            terminal.foreground,
                        );
                    }
                    choice = show(ui, &palette, ("rustronin", "RustRonin"), &mut dialog);
                };
                screen.render(&mut draw);
                screen.step(Vec::new(), &mut draw);
                let image = screen.render(&mut draw);
                drop(draw);
                assert_eq!(choice, None);
                let corner = image.image()[(4, 1000)];
                assert_ne!(corner, terminal.background, "the window behind the dialog is dimmed");
                if let Some(dir) = &snapshots {
                    crate::offscreen::write_png(&dir.join(format!("{shot}-{theme_name}.png")), image.image()).unwrap();
                }
            }
        }
    }
}
