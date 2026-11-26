use std::hash::{Hash, Hasher};

use egui::{Color32, Id, Painter, Rect, Ui, emath::easing};

use crate::accessibility;

const GLIDE: f64 = 0.18;

const LIFT: f32 = 0.15;

const HYSTERESIS: f32 = 0.2;

const SETTLE_FRAMES: u8 = 4;

pub(super) fn key(value: impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn crossed(starts: &[f32], widths: &[f32], dragged: usize, at: f32, lean: f32) -> usize {
    (0..starts.len()).filter(|index| *index != dragged && starts[*index] + widths[*index] * (0.5 + lean) < at).count()
}

pub(super) fn drop_place(
    starts: &[f32],
    widths: &[f32],
    dragged: usize,
    at: f32,
    pinned: &[bool],
    previous: Option<usize>,
) -> usize {
    let low = crossed(starts, widths, dragged, at, HYSTERESIS);
    let high = crossed(starts, widths, dragged, at, -HYSTERESIS);
    let place = match previous {
        Some(previous) => previous.clamp(low, high),
        None => crossed(starts, widths, dragged, at, 0.0),
    };
    let is_pinned = |index: usize| pinned.get(index).copied().unwrap_or(false);
    let pinned_others = (0..starts.len()).filter(|index| *index != dragged && is_pinned(*index)).count();
    if is_pinned(dragged) { place.min(pinned_others) } else { place.max(pinned_others) }
}

pub(super) fn drag_targets(
    starts: &[f32],
    widths: &[f32],
    gap: f32,
    dragged: usize,
    place: usize,
    dragged_at: Option<f32>,
) -> Vec<f32> {
    let mut order: Vec<usize> = (0..starts.len()).filter(|index| *index != dragged).collect();
    order.insert(place.min(order.len()), dragged);
    let mut targets = vec![0.0; starts.len()];
    let mut along = starts.first().copied().unwrap_or_default();
    for index in order {
        targets[index] = along;
        along += widths[index] + gap;
    }
    if let (Some(target), Some(at)) = (targets.get_mut(dragged), dragged_at) {
        *target = at;
    }
    targets
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Glide {
    from: f32,
    to: f32,
    start: f64,
}

impl Glide {
    pub(super) fn at(value: f32, now: f64) -> Self {
        Self { from: value, to: value, start: now }
    }

    pub(super) fn value(&self, now: f64) -> f32 {
        let done = ((now - self.start) / GLIDE).clamp(0.0, 1.0) as f32;
        self.from + (self.to - self.from) * easing::cubic_out(done)
    }

    pub(super) fn moving(&self, now: f64) -> bool {
        now - self.start < GLIDE && self.from != self.to
    }

    pub(super) fn toward(self, target: f32, now: f64, follow: bool) -> Self {
        if follow {
            return Self::at(target, now);
        }
        if target == self.to {
            return self;
        }
        Self { from: self.value(now), to: target, start: now }
    }
}

pub(super) fn glide(ui: &Ui, id: Id, target: f32, follow: bool) -> f32 {
    let now = ui.input(|input| input.time);
    let follow = follow || accessibility::reduce_motion();
    let glide = ui.ctx().data_mut(|data| {
        let glide = data
            .get_temp::<Glide>(id)
            .map_or_else(|| Glide::at(target, now), |glide| glide.toward(target, now, follow));
        data.insert_temp(id, glide);
        glide
    });
    if glide.moving(now) {
        ui.ctx().request_repaint();
    }
    glide.value(now)
}

pub(super) fn lift(ui: &Ui, id: Id, on: bool) -> f32 {
    let time = if accessibility::reduce_motion() { 0.0 } else { LIFT };
    ui.ctx().animate_bool_with_time_and_easing(id, on, time, easing::cubic_out)
}

pub(super) fn paint_lift(painter: &Painter, rect: Rect, shadow: Color32, lift: f32) {
    if lift <= 0.0 {
        return;
    }
    let shadow = egui::epaint::Shadow {
        offset: [0, (3.0 * lift).round() as i8],
        blur: (12.0 * lift).round() as u8,
        spread: 0,
        color: shadow.gamma_multiply(lift),
    };
    painter.add(shadow.as_shape(rect, 7));
}

#[derive(Clone, Debug, Default)]
struct Settle {
    keys: Vec<u64>,
    targets: Vec<f32>,
    frames: u8,
}

pub(super) fn begin_settle(ui: &Ui, scope: Id, keys: &[u64], targets: &[f32]) {
    let settle = Settle { keys: keys.to_vec(), targets: targets.to_vec(), frames: SETTLE_FRAMES };
    ui.ctx().data_mut(|data| data.insert_temp(scope.with("settle"), settle));
}

pub(super) fn settled(ui: &Ui, scope: Id, keys: &[u64]) -> Option<Vec<f32>> {
    let id = scope.with("settle");
    ui.ctx().data_mut(|data| {
        let mut settle = data.get_temp::<Settle>(id)?;
        if settle.frames == 0 || settle.keys != keys {
            data.remove::<Settle>(id);
            return None;
        }
        settle.frames -= 1;
        let targets = settle.targets.clone();
        data.insert_temp(id, settle);
        Some(targets)
    })
}

#[cfg(test)]
mod tests {
    use super::{GLIDE, Glide, drag_targets, drop_place};

    #[test]
    fn the_place_changes_only_past_the_neighbours_middle_and_back() {
        let starts = [0.0, 100.0, 200.0];
        let widths = [100.0; 3];
        assert_eq!(drop_place(&starts, &widths, 0, 155.0, &[], None), 1, "past the middle of the second");
        assert_eq!(drop_place(&starts, &widths, 0, 155.0, &[], Some(0)), 0, "not far enough past it yet");
        assert_eq!(drop_place(&starts, &widths, 0, 175.0, &[], Some(0)), 1);
        assert_eq!(drop_place(&starts, &widths, 0, 140.0, &[], Some(1)), 1, "a step back keeps the place");
        assert_eq!(drop_place(&starts, &widths, 0, 125.0, &[], Some(1)), 0, "far enough back gives it up");
        assert_eq!(drop_place(&starts, &widths, 2, 10.0, &[true, false, false], None), 1, "never before a pin");
        assert_eq!(drop_place(&starts, &widths, 0, 290.0, &[true, false, false], None), 0, "a pin stays first");
    }

    #[test]
    fn the_neighbours_open_the_place_and_the_dragged_tab_sticks_to_the_pointer() {
        let starts = [8.0, 112.0, 216.0];
        let widths = [100.0, 60.0, 100.0];
        assert_eq!(drag_targets(&starts, &widths, 4.0, 0, 2, Some(190.0)), [190.0, 8.0, 72.0]);
        assert_eq!(drag_targets(&starts, &widths, 4.0, 0, 2, None), [176.0, 8.0, 72.0], "where it settles");
        assert_eq!(drag_targets(&starts, &widths, 4.0, 2, 0, Some(20.0)), [112.0, 216.0, 20.0]);
    }

    #[test]
    fn a_dropped_tab_eases_into_place_without_a_jump() {
        let frame = 1.0 / 60.0;
        let mut glide = Glide::at(0.0, 0.0);
        let mut samples = Vec::new();
        for step in 0..30 {
            let now = f64::from(step) * frame;
            let pointer = 3.0 * step as f32;
            if step < 10 {
                glide = glide.toward(pointer, now, true);
            } else {
                glide = glide.toward(176.0, now, false);
            }
            samples.push(glide.value(now));
        }
        assert_eq!(samples[9], 27.0, "while dragged it is where the pointer is");
        assert!((samples[10] - samples[9]).abs() < 1e-3, "no jump at the drop: {:?}", &samples[8..12]);
        let after: Vec<f32> = samples[10..].to_vec();
        assert!(after.windows(2).all(|pair| pair[1] >= pair[0]), "it only moves forward: {after:?}");
        let steps: Vec<f32> = after.windows(2).map(|pair| pair[1] - pair[0]).collect();
        assert!(steps[0] > steps[5] && steps[5] > steps[9], "fast first, slow at the end: {steps:?}");
        let settled = (GLIDE / frame).ceil() as usize + 10;
        assert!((samples[settled.min(29)] - 176.0).abs() < 1e-3, "in place after the glide: {samples:?}");
    }

    #[test]
    fn a_new_target_starts_from_where_the_tab_is() {
        let glide = Glide::at(0.0, 0.0).toward(100.0, 0.0, false);
        let middle = glide.value(GLIDE / 2.0);
        let turned = glide.toward(0.0, GLIDE / 2.0, false);
        assert!((turned.value(GLIDE / 2.0) - middle).abs() < 1e-3, "no jump when it turns back");
    }
}
