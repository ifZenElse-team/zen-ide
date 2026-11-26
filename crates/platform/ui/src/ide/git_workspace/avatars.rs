use std::collections::{BTreeSet, HashMap};

use egui::{Color32, Rect, Vec2, pos2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_workbench::vcs::avatars::{AVATAR_SIZE, AVATARS, AVATARS_BY_DEFAULT, AvatarInfo, AvatarStatus};

use super::super::IdePalette;

const ASK_EVERY: f64 = 0.3;

#[derive(Default)]
pub(super) struct AvatarView {
    known: HashMap<String, Option<egui::TextureHandle>>,
    wanted: BTreeSet<String>,
    asked_at: f64,
    on: bool,
    decoded: Option<Decoded>,
}

struct Decoded {
    sender: std::sync::mpsc::Sender<(String, Option<Face>)>,
    receiver: std::sync::mpsc::Receiver<(String, Option<Face>)>,
}

struct Face {
    name: String,
    size: usize,
    rgba: Vec<u8>,
}

impl AvatarView {
    pub(super) fn picture(&mut self, email: &str) -> Option<egui::TextureHandle> {
        if !self.on || email.trim().is_empty() {
            return None;
        }
        let key = email.trim().to_lowercase();
        match self.known.get(&key) {
            Some(known) => known.clone(),
            None => {
                self.wanted.insert(key);
                None
            }
        }
    }

    fn decode_behind(&mut self, ctx: &egui::Context, key: String, file: std::path::PathBuf) {
        let decoded = self.decoded.get_or_insert_with(|| {
            let (sender, receiver) = std::sync::mpsc::channel();
            Decoded { sender, receiver }
        });
        let (sender, ctx) = (decoded.sender.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = sender.send((key, decode(&file)));
            ctx.request_repaint();
        });
    }

    fn take_decoded(&mut self, ctx: &egui::Context) {
        let Some(decoded) = &self.decoded else { return };
        for (key, face) in decoded.receiver.try_iter() {
            let texture = face.map(|Face { name, size, rgba }| {
                let image = egui::ColorImage::from_rgba_unmultiplied([size, size], &rgba);
                ctx.load_texture(name, image, egui::TextureOptions::LINEAR)
            });
            self.known.insert(key, texture);
        }
    }

    pub(super) fn follow(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session) {
        self.take_decoded(ctx);
        let on = session.settings().get::<bool>(AVATARS).unwrap_or(AVATARS_BY_DEFAULT);
        if on != self.on {
            self.on = on;
            self.known.clear();
            self.wanted.clear();
            ctx.request_repaint();
        }
        let now = ctx.input(|input| input.time);
        if !on || self.wanted.is_empty() || now - self.asked_at < ASK_EVERY {
            if !self.wanted.is_empty() {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(ASK_EVERY));
            }
            return;
        }
        self.asked_at = now;
        let emails: Vec<String> = std::mem::take(&mut self.wanted).into_iter().collect();
        let answer = registry.dispatch(session, "vcs.avatar.get", json!({ "emails": emails }));
        let Some(list) = answer.ok().and_then(|value| serde_json::from_value::<Vec<AvatarInfo>>(value).ok()) else {
            return;
        };
        let mut pending = false;
        for info in list {
            let key = info.email.trim().to_lowercase();
            match (info.status, info.file) {
                (AvatarStatus::Found, Some(file)) => {
                    self.known.insert(key.clone(), None);
                    self.decode_behind(ctx, key, file);
                }
                (AvatarStatus::Pending, _) => pending = true,
                (AvatarStatus::Off, _) => {}
                _ => {
                    self.known.insert(key, None);
                }
            }
        }
        if pending {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(ASK_EVERY));
        }
    }
}

fn decode(file: &std::path::Path) -> Option<Face> {
    let bytes = std::fs::read(file).ok()?;
    let picture = zen_image::decode(std::path::Path::new("avatar.img"), &bytes).ok()?;
    let (size, rgba) = round(&picture.shrunk_to(AVATAR_SIZE as usize * 2));
    Some(Face { name: format!("gw-avatar-{}", file.display()), size, rgba })
}

pub(super) fn round(picture: &zen_image::Picture) -> (usize, Vec<u8>) {
    let side = picture.width.min(picture.height).max(1);
    let (left, top) = ((picture.width - side.min(picture.width)) / 2, (picture.height - side.min(picture.height)) / 2);
    let radius = side as f32 / 2.0;
    let mut rgba = Vec::with_capacity(side * side * 4);
    for y in 0..side {
        for x in 0..side {
            let at = ((top + y) * picture.width + left + x) * 4;
            let pixel = picture.rgba.get(at..at + 4).unwrap_or(&[0, 0, 0, 0]);
            let (dx, dy) = (x as f32 + 0.5 - radius, y as f32 + 0.5 - radius);
            let inside = (radius - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0);
            rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], (f32::from(pixel[3]) * inside).round() as u8]);
        }
    }
    (side, rgba)
}

pub(super) fn draw_face(
    painter: &egui::Painter,
    p: &IdePalette,
    (center, radius): (egui::Pos2, f32),
    name: &str,
    size: f32,
    picture: Option<&egui::TextureHandle>,
) {
    match picture {
        Some(texture) => {
            let rect = Rect::from_center_size(center, Vec2::splat(radius * 2.0));
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(texture.id(), rect, uv, Color32::WHITE);
        }
        None => super::draw_avatar(painter, p, center, radius, name, size),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_avatar_is_decoded_behind_the_frame_and_arrives_on_a_later_one() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("broken");
        std::fs::write(&file, "not a picture").unwrap();
        let ctx = egui::Context::default();
        let mut view = AvatarView::default();
        view.decode_behind(&ctx, "new@example.com".to_owned(), file);
        while !view.known.contains_key("new@example.com") {
            view.take_decoded(&ctx);
            std::thread::yield_now();
        }
        assert!(view.known["new@example.com"].is_none(), "a broken picture leaves no texture");
    }

    #[test]
    fn a_picture_is_cut_round_from_its_middle_square() {
        let picture = zen_image::Picture {
            width: 4,
            height: 2,
            rgba: (0..8).flat_map(|index| [index as u8, 0, 0, 255]).collect(),
            format: "PNG",
            bits_per_pixel: None,
            vector_size: None,
        };
        let (side, rgba) = round(&picture);
        assert_eq!((side, rgba.len()), (2, 16));
        assert_eq!(rgba[0], 1, "the square starts at the second column");
        assert!(rgba.chunks(4).all(|pixel| pixel[3] < 255 && pixel[3] > 0), "a 2-pixel circle fades every pixel");
        let big = zen_image::Picture { width: 20, height: 20, rgba: vec![255; 20 * 20 * 4], ..picture };
        let (side, rgba) = round(&big);
        let alpha = |x: usize, y: usize| rgba[(y * side + x) * 4 + 3];
        assert_eq!((alpha(10, 10), alpha(0, 0), alpha(19, 19)), (255, 0, 0));
    }
}
