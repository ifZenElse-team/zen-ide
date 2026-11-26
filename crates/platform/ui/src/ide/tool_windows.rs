use std::ops::RangeInclusive;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Side {
    LeftTop,
    LeftBottom,
    BottomLeft,
    BottomRight,
    RightTop,
    RightBottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Anchor {
    Left,
    Bottom,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum ViewMode {
    #[default]
    DockPinned,
    DockUnpinned,
    Undock,
    Float,
    Window,
}

impl ViewMode {
    pub(super) const ALL: [ViewMode; 5] =
        [ViewMode::DockPinned, ViewMode::DockUnpinned, ViewMode::Undock, ViewMode::Float, ViewMode::Window];

    pub(super) fn name(self) -> &'static str {
        match self {
            ViewMode::DockPinned => "dockPinned",
            ViewMode::DockUnpinned => "dockUnpinned",
            ViewMode::Undock => "undock",
            ViewMode::Float => "float",
            ViewMode::Window => "window",
        }
    }

    pub(super) fn from_name(name: &str) -> Option<ViewMode> {
        ViewMode::ALL.into_iter().find(|mode| mode.name() == name)
    }

    pub(super) fn action(self) -> &'static str {
        match self {
            ViewMode::DockPinned => "view.toolWindow.viewMode/dockPinned",
            ViewMode::DockUnpinned => "view.toolWindow.viewMode/dockUnpinned",
            ViewMode::Undock => "view.toolWindow.viewMode/undock",
            ViewMode::Float => "view.toolWindow.viewMode/float",
            ViewMode::Window => "view.toolWindow.viewMode/window",
        }
    }

    pub(super) fn of_action(action: &str) -> Option<ViewMode> {
        ViewMode::ALL.into_iter().find(|mode| mode.action() == action)
    }

    pub(super) fn docked(self) -> bool {
        matches!(self, ViewMode::DockPinned | ViewMode::DockUnpinned)
    }

    pub(super) fn on_side(self) -> bool {
        matches!(self, ViewMode::DockPinned | ViewMode::DockUnpinned | ViewMode::Undock)
    }

    pub(super) fn detached(self) -> bool {
        matches!(self, ViewMode::Float | ViewMode::Window)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Bounds {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) width: f32,
    pub(super) height: f32,
}

impl Bounds {
    pub(super) const MIN: [f32; 2] = [200.0, 120.0];

    pub(super) fn sanitized(self) -> Option<Bounds> {
        if ![self.x, self.y, self.width, self.height].iter().all(|value| value.is_finite()) {
            return None;
        }
        Some(Bounds {
            width: self.width.clamp(Self::MIN[0], 10_000.0),
            height: self.height.clamp(Self::MIN[1], 10_000.0),
            ..self
        })
    }
}

impl Side {
    pub(super) const ALL: [Side; 6] =
        [Side::LeftTop, Side::LeftBottom, Side::BottomLeft, Side::BottomRight, Side::RightTop, Side::RightBottom];

    pub(super) fn anchor(self) -> Anchor {
        match self {
            Side::LeftTop | Side::LeftBottom => Anchor::Left,
            Side::BottomLeft | Side::BottomRight => Anchor::Bottom,
            Side::RightTop | Side::RightBottom => Anchor::Right,
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Side::LeftTop => "leftTop",
            Side::LeftBottom => "leftBottom",
            Side::BottomLeft => "bottomLeft",
            Side::BottomRight => "bottomRight",
            Side::RightTop => "rightTop",
            Side::RightBottom => "rightBottom",
        }
    }

    pub(super) fn from_name(name: &str) -> Option<Side> {
        match name {
            "left" => Some(Side::LeftTop),
            "right" => Some(Side::RightTop),
            "bottom" => Some(Side::BottomLeft),
            _ => Side::ALL.into_iter().find(|side| side.name() == name),
        }
    }

    pub(super) fn message(self) -> &'static str {
        match self {
            Side::LeftTop => "ide-tool-side-left-top",
            Side::LeftBottom => "ide-tool-side-left-bottom",
            Side::BottomLeft => "ide-tool-side-bottom-left",
            Side::BottomRight => "ide-tool-side-bottom-right",
            Side::RightTop => "ide-tool-side-right-top",
            Side::RightBottom => "ide-tool-side-right-bottom",
        }
    }
}

impl Anchor {
    pub(super) fn sides(self) -> [Side; 2] {
        match self {
            Anchor::Left => [Side::LeftTop, Side::LeftBottom],
            Anchor::Bottom => [Side::BottomLeft, Side::BottomRight],
            Anchor::Right => [Side::RightTop, Side::RightBottom],
        }
    }

    pub(super) fn default_size(self) -> f32 {
        match self {
            Anchor::Left | Anchor::Right => 330.0,
            Anchor::Bottom => 280.0,
        }
    }

    pub(super) fn sizes(self) -> RangeInclusive<f32> {
        match self {
            Anchor::Left | Anchor::Right => 180.0..=640.0,
            Anchor::Bottom => 100.0..=900.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Declared {
    pub(super) id: &'static str,
    pub(super) side: Side,
    pub(super) shown: bool,
    pub(super) reopens: bool,
}

pub(super) const DECLARED: &[Declared] = &[
    Declared { id: "project", side: Side::LeftTop, shown: true, reopens: true },
    Declared { id: "commit", side: Side::LeftTop, shown: false, reopens: true },
    Declared { id: "structure", side: Side::LeftTop, shown: false, reopens: true },
    Declared { id: "bookmarks", side: Side::LeftBottom, shown: false, reopens: true },
    Declared { id: "find", side: Side::BottomLeft, shown: false, reopens: false },
    Declared { id: "debug", side: Side::BottomLeft, shown: false, reopens: false },
    Declared { id: "run", side: Side::BottomLeft, shown: false, reopens: false },
    Declared { id: "terminal", side: Side::BottomLeft, shown: false, reopens: true },
    Declared { id: "problems", side: Side::BottomLeft, shown: false, reopens: true },
    Declared { id: "git", side: Side::BottomLeft, shown: false, reopens: true },
    Declared { id: "services", side: Side::BottomLeft, shown: false, reopens: false },
    Declared { id: "todo", side: Side::BottomLeft, shown: false, reopens: true },
    Declared { id: "build", side: Side::BottomLeft, shown: false, reopens: true },
    Declared { id: "endpoints", side: Side::BottomLeft, shown: false, reopens: true },
    Declared { id: "dependencies", side: Side::BottomLeft, shown: false, reopens: true },
    Declared { id: "caffeine", side: Side::RightTop, shown: false, reopens: true },
    Declared { id: "hierarchy", side: Side::RightTop, shown: false, reopens: false },
    Declared { id: "maven", side: Side::RightTop, shown: false, reopens: true },
    Declared { id: "gradle", side: Side::RightTop, shown: false, reopens: true },
    Declared { id: "ant", side: Side::RightTop, shown: false, reopens: true },
    Declared { id: "npm", side: Side::RightTop, shown: false, reopens: true },
    Declared { id: "coverage", side: Side::RightTop, shown: false, reopens: false },
    Declared { id: "notifications", side: Side::RightBottom, shown: false, reopens: false },
];

pub(super) fn declared(id: &str) -> Option<&'static Declared> {
    DECLARED.iter().find(|declared| declared.id == id).or_else(|| super::plugin_tool_window::declared(id))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ToolWindowState {
    pub(super) id: String,
    pub(super) side: Side,
    #[serde(default)]
    pub(super) size: f32,
    #[serde(default)]
    pub(super) visible: bool,
    #[serde(default)]
    pub(super) mode: ViewMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) bounds: Option<Bounds>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Splits {
    pub(super) left: f32,
    pub(super) bottom: f32,
    pub(super) right: f32,
}

impl Default for Splits {
    fn default() -> Self {
        Self { left: 0.5, bottom: 0.5, right: 0.5 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct ToolWindowLayout {
    pub(super) windows: Vec<ToolWindowState>,
    pub(super) splits: Splits,
    #[serde(skip)]
    pub(super) active: Option<String>,
}

impl Default for ToolWindowLayout {
    fn default() -> Self {
        Self {
            windows: DECLARED
                .iter()
                .map(|declared| ToolWindowState {
                    id: declared.id.to_owned(),
                    side: declared.side,
                    size: declared.side.anchor().default_size(),
                    visible: declared.shown,
                    mode: ViewMode::DockPinned,
                    bounds: None,
                })
                .collect(),
            splits: Splits::default(),
            active: None,
        }
    }
}

pub(super) const STRETCH_STEP: f32 = 20.0;

pub(super) const EDITOR_ROOM: f32 = 160.0;

impl ToolWindowLayout {
    pub(super) fn from_shown(shown: &[&str]) -> Self {
        let mut layout = Self::default();
        for window in &mut layout.windows {
            window.visible = false;
        }
        for id in shown {
            layout.show(id);
        }
        layout.active = None;
        layout
    }

    pub(super) fn reopened(saved: ToolWindowLayout) -> Self {
        Self::made_whole(saved, false)
    }

    pub(super) fn applied(saved: ToolWindowLayout) -> Self {
        Self::made_whole(saved, true)
    }

    fn made_whole(saved: ToolWindowLayout, results: bool) -> Self {
        let mut windows: Vec<ToolWindowState> = Vec::new();
        for window in saved.windows {
            if windows.iter().any(|known| known.id == window.id) {
                continue;
            }
            windows.push(window);
        }
        let mut layout = Self { windows, splits: saved.splits, active: None };
        for (index, declared) in DECLARED.iter().enumerate() {
            if layout.position(declared.id).is_some() {
                continue;
            }
            let at = DECLARED[..index]
                .iter()
                .rev()
                .find_map(|before| layout.position(before.id).filter(|&at| layout.windows[at].side == declared.side))
                .map_or_else(
                    || {
                        layout
                            .windows
                            .iter()
                            .position(|window| window.side == declared.side)
                            .unwrap_or(layout.windows.len())
                    },
                    |at| at + 1,
                );
            let size = declared.side.anchor().default_size();
            layout.windows.insert(
                at,
                ToolWindowState {
                    id: declared.id.to_owned(),
                    side: declared.side,
                    size,
                    visible: false,
                    mode: ViewMode::DockPinned,
                    bounds: None,
                },
            );
        }
        let mut shown: Vec<Side> = Vec::new();
        for window in &mut layout.windows {
            let range = window.side.anchor().sizes();
            window.size = if window.size.is_finite() && window.size > 0.0 {
                window.size.clamp(*range.start(), *range.end())
            } else {
                window.side.anchor().default_size()
            };
            window.bounds = window.bounds.and_then(Bounds::sanitized);
            let kept = declared(&window.id).is_some_and(|declared| declared.reopens || results);
            if !kept || (window.mode.on_side() && shown.contains(&window.side)) {
                window.visible = false;
            }
            if window.visible && window.mode.on_side() {
                shown.push(window.side);
            }
        }
        for split in [&mut layout.splits.left, &mut layout.splits.bottom, &mut layout.splits.right] {
            *split = if split.is_finite() { split.clamp(0.1, 0.9) } else { 0.5 };
        }
        layout
    }

    fn position(&self, id: &str) -> Option<usize> {
        self.windows.iter().position(|window| window.id == id)
    }

    pub(super) fn ensure(&mut self, id: &str, side: Side) {
        if self.position(id).is_none() {
            self.windows.push(ToolWindowState {
                id: id.to_owned(),
                side,
                size: side.anchor().default_size(),
                visible: false,
                mode: ViewMode::DockPinned,
                bounds: None,
            });
        }
    }

    pub(super) fn get(&self, id: &str) -> Option<&ToolWindowState> {
        self.windows.iter().find(|window| window.id == id)
    }

    pub(super) fn side_of(&self, id: &str) -> Option<Side> {
        self.get(id).map(|window| window.side)
    }

    pub(super) fn is_visible(&self, id: &str) -> bool {
        self.get(id).is_some_and(|window| window.visible)
    }

    pub(super) fn at(&self, side: Side) -> impl Iterator<Item = &ToolWindowState> {
        self.windows.iter().filter(move |window| window.side == side)
    }

    pub(super) fn visible_at(&self, side: Side) -> Option<&ToolWindowState> {
        self.at(side).find(|window| window.visible && window.mode.on_side())
    }

    pub(super) fn docked_at(&self, side: Side) -> Option<&ToolWindowState> {
        self.at(side).find(|window| window.visible && window.mode.docked())
    }

    pub(super) fn undocked(&self) -> impl Iterator<Item = &ToolWindowState> {
        self.windows.iter().filter(|window| window.visible && window.mode == ViewMode::Undock)
    }

    pub(super) fn detached(&self) -> impl Iterator<Item = &ToolWindowState> {
        self.windows.iter().filter(|window| window.visible && window.mode.detached())
    }

    pub(super) fn mode_of(&self, id: &str) -> Option<ViewMode> {
        self.get(id).map(|window| window.mode)
    }

    pub(super) fn show(&mut self, id: &str) -> bool {
        let Some(shown) = self.get(id) else { return false };
        let (side, on_side) = (shown.side, shown.mode.on_side());
        for window in &mut self.windows {
            if window.id == id {
                window.visible = true;
            } else if window.visible
                && ((on_side && window.side == side && window.mode.on_side())
                    || matches!(window.mode, ViewMode::DockUnpinned | ViewMode::Undock))
            {
                window.visible = false;
            }
        }
        self.active = Some(id.to_owned());
        true
    }

    pub(super) fn lose_focus(&mut self, keep: Option<&str>) -> bool {
        let mut hidden = false;
        for window in &mut self.windows {
            if window.visible && window.mode == ViewMode::DockUnpinned && Some(window.id.as_str()) != keep {
                window.visible = false;
                hidden = true;
            }
        }
        if hidden && self.active.as_deref().is_some_and(|id| !self.is_visible(id)) {
            self.active = None;
        }
        hidden
    }

    pub(super) fn set_mode(&mut self, id: &str, mode: ViewMode) -> bool {
        let Some(at) = self.position(id) else { return false };
        if self.windows[at].mode == mode {
            return false;
        }
        self.windows[at].mode = mode;
        let (side, visible) = (self.windows[at].side, self.windows[at].visible);
        if visible && mode.on_side() {
            for window in &mut self.windows {
                if window.id != id && window.side == side && window.visible && window.mode.on_side() {
                    window.visible = false;
                }
            }
        }
        true
    }

    pub(super) fn set_bounds(&mut self, id: &str, bounds: Bounds) -> bool {
        let Some(at) = self.position(id) else { return false };
        let bounds = bounds.sanitized();
        if bounds.is_none() || self.windows[at].bounds == bounds {
            return false;
        }
        self.windows[at].bounds = bounds;
        true
    }

    pub(super) fn hide(&mut self, id: &str) -> bool {
        let Some(at) = self.position(id) else { return false };
        self.windows[at].visible = false;
        if self.active.as_deref() == Some(id) {
            self.active = None;
        }
        true
    }

    pub(super) fn toggle(&mut self, id: &str) -> bool {
        if self.is_visible(id) { self.hide(id) } else { self.show(id) }
    }

    pub(super) fn activate(&mut self, id: &str) {
        if self.is_visible(id) {
            self.active = Some(id.to_owned());
        }
    }

    pub(super) fn active(&self) -> Option<&str> {
        self.active.as_deref().filter(|id| self.is_visible(id))
    }

    pub(super) fn move_to(&mut self, id: &str, side: Side, index: Option<usize>) -> bool {
        let Some(from) = self.position(id) else { return false };
        let mut window = self.windows.remove(from);
        if window.side.anchor() != side.anchor() {
            window.size = self.size_at(side.anchor());
        }
        window.side = side;
        let positions: Vec<usize> =
            self.windows.iter().enumerate().filter(|(_, other)| other.side == side).map(|(at, _)| at).collect();
        let at = match index.and_then(|index| positions.get(index)) {
            Some(&at) => at,
            None => positions.last().map_or_else(|| self.default_position(side), |&last| last + 1),
        };
        let visible = window.visible;
        self.windows.insert(at, window);
        if visible {
            self.show(id);
        }
        true
    }

    fn default_position(&self, side: Side) -> usize {
        let order = |side: Side| Side::ALL.iter().position(|other| *other == side).unwrap_or(0);
        self.windows.iter().position(|window| order(window.side) > order(side)).unwrap_or(self.windows.len())
    }

    pub(super) fn size_at(&self, anchor: Anchor) -> f32 {
        let [first, second] = anchor.sides();
        let on_edge = |window: &&ToolWindowState| window.side == first || window.side == second;
        self.windows
            .iter()
            .filter(on_edge)
            .find(|window| window.visible)
            .or_else(|| self.windows.iter().find(on_edge))
            .map_or(anchor.default_size(), |window| window.size)
    }

    pub(super) fn set_size_at(&mut self, anchor: Anchor, size: f32) -> bool {
        let range = anchor.sizes();
        let size = size.clamp(*range.start(), *range.end());
        let before = self.size_at(anchor);
        for window in &mut self.windows {
            if window.side.anchor() == anchor {
                window.size = size;
            }
        }
        before != size
    }

    pub(super) fn split(&self, anchor: Anchor) -> f32 {
        match anchor {
            Anchor::Left => self.splits.left,
            Anchor::Bottom => self.splits.bottom,
            Anchor::Right => self.splits.right,
        }
    }

    pub(super) fn set_split(&mut self, anchor: Anchor, share: f32) {
        let share = share.clamp(0.1, 0.9);
        match anchor {
            Anchor::Left => self.splits.left = share,
            Anchor::Bottom => self.splits.bottom = share,
            Anchor::Right => self.splits.right = share,
        }
    }

    pub(super) fn hide_all(&mut self) -> ToolWindowLayout {
        let kept = self.clone();
        for window in &mut self.windows {
            window.visible = false;
        }
        self.active = None;
        kept
    }

    pub(super) fn restore(&mut self, kept: ToolWindowLayout) {
        for window in kept.windows.iter().filter(|window| window.visible) {
            self.show(&window.id);
        }
        self.active = kept.active.filter(|id| self.is_visible(id));
    }

    pub(super) fn fitted_sizes(&self, width: f32, height: f32) -> [f32; 3] {
        let shown = |anchor: Anchor| {
            let [first, second] = anchor.sides();
            if self.docked_at(first).is_some() || self.docked_at(second).is_some() { self.size_at(anchor) } else { 0.0 }
        };
        let (left, bottom, right) = (shown(Anchor::Left), shown(Anchor::Bottom), shown(Anchor::Right));
        let bottom = bottom.min((height - EDITOR_ROOM).max(0.0));
        let across = (width - EDITOR_ROOM).max(0.0);
        let scale = if left + right > across && left + right > 0.0 { across / (left + right) } else { 1.0 };
        [left * scale, bottom, right * scale]
    }

    pub(super) fn stretch(&mut self, id: &str, direction: Direction, by: f32, extent: f32) -> bool {
        let Some(window) = self.get(id).filter(|window| window.visible && window.mode.on_side()) else {
            return false;
        };
        let side = window.side;
        let anchor = side.anchor();
        let grow = match (anchor, direction) {
            (Anchor::Left, Direction::Right) | (Anchor::Right, Direction::Left) | (Anchor::Bottom, Direction::Up) => {
                Some(by)
            }
            (Anchor::Left, Direction::Left) | (Anchor::Right, Direction::Right) | (Anchor::Bottom, Direction::Down) => {
                Some(-by)
            }
            _ => None,
        };
        if let Some(grow) = grow {
            let size = self.size_at(anchor) + grow;
            return self.set_size_at(anchor, size);
        }
        let [first, second] = anchor.sides();
        if self.docked_at(first).is_none() || self.docked_at(second).is_none() || extent <= 0.0 {
            return false;
        }
        let step = by / extent;
        let share = self.split(anchor);
        let new = if matches!(direction, Direction::Up | Direction::Left) { share - step } else { share + step };
        self.set_split(anchor, new);
        self.split(anchor) != share
    }
}

pub(super) const DEFAULT_LAYOUT: &str = "default";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NamedLayout {
    pub(super) name: String,
    pub(super) layout: ToolWindowLayout,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct SavedLayouts {
    pub(super) layouts: Vec<NamedLayout>,
    pub(super) current: Option<String>,
}

impl SavedLayouts {
    pub(super) fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<SavedLayouts>(&text).ok())
            .map(|mut saved| {
                let mut names: Vec<String> = Vec::new();
                saved.layouts.retain(|layout| {
                    let fresh = !layout.name.trim().is_empty()
                        && !is_default_name(&layout.name)
                        && !names.iter().any(|name| name.eq_ignore_ascii_case(&layout.name));
                    names.push(layout.name.clone());
                    fresh
                });
                if saved.current.as_deref().is_some_and(|current| saved.get(current).is_none()) {
                    saved.current = None;
                }
                saved
            })
            .unwrap_or_default()
    }

    pub(super) fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)
    }

    pub(super) fn get(&self, name: &str) -> Option<&NamedLayout> {
        self.layouts.iter().find(|layout| layout.name.eq_ignore_ascii_case(name.trim()))
    }

    pub(super) fn save_as(&mut self, name: &str, layout: &ToolWindowLayout) -> Result<String, SaveError> {
        let name = self.free_name(name, None)?;
        let layout = ToolWindowLayout { active: None, ..layout.clone() };
        self.layouts.push(NamedLayout { name: name.clone(), layout });
        self.current = Some(name.clone());
        Ok(name)
    }

    pub(super) fn save_changes(&mut self, name: &str, layout: &ToolWindowLayout) -> bool {
        let Some(found) = self.layouts.iter_mut().find(|found| found.name.eq_ignore_ascii_case(name.trim())) else {
            return false;
        };
        found.layout = ToolWindowLayout { active: None, ..layout.clone() };
        true
    }

    pub(super) fn rename(&mut self, name: &str, new_name: &str) -> Option<Result<String, SaveError>> {
        let at = self.layouts.iter().position(|found| found.name.eq_ignore_ascii_case(name.trim()))?;
        let old = self.layouts[at].name.clone();
        let new_name = match self.free_name(new_name, Some(&old)) {
            Ok(new_name) => new_name,
            Err(error) => return Some(Err(error)),
        };
        if self.current.as_deref().is_some_and(|current| current.eq_ignore_ascii_case(&old)) {
            self.current = Some(new_name.clone());
        }
        self.layouts[at].name = new_name.clone();
        Some(Ok(new_name))
    }

    fn free_name(&self, name: &str, renamed: Option<&str>) -> Result<String, SaveError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(SaveError::Empty);
        }
        if is_default_name(name) {
            return Err(SaveError::Default);
        }
        if self.get(name).is_some_and(|found| renamed.is_none_or(|renamed| !found.name.eq_ignore_ascii_case(renamed))) {
            return Err(SaveError::Taken);
        }
        Ok(name.to_owned())
    }

    pub(super) fn delete(&mut self, name: &str) -> bool {
        let Some(at) = self.layouts.iter().position(|layout| layout.name.eq_ignore_ascii_case(name.trim())) else {
            return false;
        };
        let removed = self.layouts.remove(at);
        if self.current.as_deref().is_some_and(|current| current.eq_ignore_ascii_case(&removed.name)) {
            self.current = None;
        }
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SaveError {
    Empty,
    Default,
    Taken,
}

impl SaveError {
    pub(super) fn message(self) -> &'static str {
        match self {
            SaveError::Empty => "ide-layout-save-error-empty",
            SaveError::Default => "ide-layout-save-error-default",
            SaveError::Taken => "ide-layout-save-error-taken",
        }
    }
}

pub(super) fn is_default_name(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case(DEFAULT_LAYOUT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(layout: &ToolWindowLayout, side: Side) -> Vec<&str> {
        layout.at(side).map(|window| window.id.as_str()).collect()
    }

    #[test]
    fn the_default_layout_is_todays() {
        let layout = ToolWindowLayout::default();
        assert_eq!(ids(&layout, Side::LeftTop), ["project", "commit", "structure"]);
        assert_eq!(
            ids(&layout, Side::BottomLeft),
            [
                "find",
                "debug",
                "run",
                "terminal",
                "problems",
                "git",
                "services",
                "todo",
                "build",
                "endpoints",
                "dependencies"
            ]
        );
        assert_eq!(ids(&layout, Side::RightBottom), ["notifications"]);
        assert_eq!(ids(&layout, Side::LeftBottom), ["bookmarks"], "on the left stripe under the line");
        assert_eq!(
            ids(&layout, Side::RightTop),
            ["caffeine", "hierarchy", "maven", "gradle", "ant", "npm", "coverage"],
            "Caffeine, under Git on the right stripe; Maven, Gradle, Ant and npm, listed where they apply"
        );
        assert!(layout.is_visible("project"));
        assert_eq!(layout.windows.iter().filter(|window| window.visible).count(), 1);
        assert_eq!(layout.size_at(Anchor::Left), 330.0);
        assert_eq!(layout.size_at(Anchor::Bottom), 280.0);
    }

    #[test]
    fn one_tool_window_shows_on_a_side_and_the_halves_share_an_edge() {
        let mut layout = ToolWindowLayout::default();
        assert!(layout.show("structure"));
        assert!(!layout.is_visible("project"), "Structure takes the place of Project");
        layout.show("terminal");
        layout.show("git");
        assert!(!layout.is_visible("terminal"));
        assert_eq!(layout.active(), Some("git"));
        layout.move_to("project", Side::LeftBottom, None);
        layout.show("project");
        assert!(layout.is_visible("structure") && layout.is_visible("project"), "left top and left bottom both show");
        assert!(layout.toggle("project"));
        assert!(!layout.is_visible("project"));
        assert!(!layout.show("nothing"));
    }

    #[test]
    fn moving_keeps_the_order_and_the_visibility() {
        let mut layout = ToolWindowLayout::default();
        layout.show("terminal");
        assert!(layout.move_to("terminal", Side::RightTop, None));
        assert_eq!(
            ids(&layout, Side::RightTop),
            ["caffeine", "hierarchy", "maven", "gradle", "ant", "npm", "coverage", "terminal"]
        );
        assert!(layout.is_visible("terminal"), "a shown tool window stays shown where it goes");
        assert_eq!(layout.get("terminal").unwrap().size, 330.0, "it takes the width of a side");
        layout.move_to("git", Side::RightTop, Some(0));
        assert_eq!(
            ids(&layout, Side::RightTop),
            ["git", "caffeine", "hierarchy", "maven", "gradle", "ant", "npm", "coverage", "terminal"]
        );
        layout.move_to("structure", Side::RightTop, Some(1));
        assert_eq!(
            ids(&layout, Side::RightTop),
            ["git", "structure", "caffeine", "hierarchy", "maven", "gradle", "ant", "npm", "coverage", "terminal"]
        );
        layout.move_to("structure", Side::LeftTop, Some(0));
        assert_eq!(ids(&layout, Side::LeftTop), ["structure", "project", "commit"]);
        layout.move_to("project", Side::LeftTop, Some(9));
        assert_eq!(ids(&layout, Side::LeftTop), ["structure", "commit", "project"]);
        layout.show("run");
        layout.move_to("run", Side::BottomRight, None);
        layout.show("problems");
        assert!(layout.is_visible("run") && layout.is_visible("problems"), "bottom left and right both show");
        assert!(!layout.move_to("nothing", Side::LeftTop, None));
    }

    #[test]
    fn the_edges_share_a_size_within_range_and_stretch() {
        let mut layout = ToolWindowLayout::default();
        assert!(layout.set_size_at(Anchor::Left, 400.0));
        assert_eq!(layout.get("structure").unwrap().size, 400.0, "the tool windows of an edge share a size");
        layout.set_size_at(Anchor::Left, 5000.0);
        assert_eq!(layout.size_at(Anchor::Left), 640.0);
        layout.set_size_at(Anchor::Bottom, 10.0);
        assert_eq!(layout.size_at(Anchor::Bottom), 100.0);

        layout.set_size_at(Anchor::Left, 330.0);
        assert!(layout.stretch("project", Direction::Right, STRETCH_STEP, 800.0));
        assert_eq!(layout.size_at(Anchor::Left), 350.0);
        assert!(layout.stretch("project", Direction::Left, STRETCH_STEP, 800.0));
        assert_eq!(layout.size_at(Anchor::Left), 330.0);
        assert!(!layout.stretch("project", Direction::Up, STRETCH_STEP, 800.0), "one half shown: nothing to share");
        assert!(!layout.stretch("terminal", Direction::Up, STRETCH_STEP, 800.0), "a hidden tool window");
        layout.show("terminal");
        assert!(layout.stretch("terminal", Direction::Up, STRETCH_STEP, 800.0));
        assert_eq!(layout.size_at(Anchor::Bottom), 120.0);

        layout.move_to("structure", Side::LeftBottom, None);
        layout.show("structure");
        assert!(layout.stretch("structure", Direction::Up, 80.0, 800.0));
        assert!((layout.split(Anchor::Left) - 0.4).abs() < 1e-4, "the bottom half grows upwards");
        assert!(layout.stretch("project", Direction::Down, 80.0, 800.0));
        assert!((layout.split(Anchor::Left) - 0.5).abs() < 1e-4);
        layout.set_split(Anchor::Left, 2.0);
        assert_eq!(layout.split(Anchor::Left), 0.9);
    }

    #[test]
    fn the_layout_round_trips_and_a_file_is_made_whole() {
        let mut layout = ToolWindowLayout::default();
        layout.move_to("terminal", Side::RightBottom, None);
        layout.move_to("project", Side::RightTop, None);
        layout.show("terminal");
        layout.set_size_at(Anchor::Right, 420.0);
        layout.set_split(Anchor::Right, 0.3);
        let json = serde_json::to_value(&layout).unwrap();
        assert_eq!(json["windows"][0]["id"], "commit");
        assert!(json.get("active").is_none(), "the active tool window is not kept");
        let terminal = json["windows"].as_array().unwrap().iter().find(|window| window["id"] == "terminal").unwrap();
        assert_eq!(terminal["side"], "rightBottom");
        assert_eq!(terminal["size"], 420.0);
        assert_eq!(terminal["visible"], true);
        let read: ToolWindowLayout = serde_json::from_value(json).unwrap();
        let mut expected = layout.clone();
        expected.active = None;
        assert_eq!(read, expected);
        assert_eq!(ToolWindowLayout::reopened(read), expected);

        let partial: ToolWindowLayout = serde_json::from_str(
            r#"{ "windows": [
                { "id": "git", "side": "rightTop", "size": 99999, "visible": true },
                { "id": "problems", "side": "rightTop", "size": 300, "visible": true },
                { "id": "run", "side": "bottomLeft", "size": 300, "visible": true },
                { "id": "database", "side": "bottomRight", "size": 250, "visible": true },
                { "id": "git", "side": "leftTop", "size": 300 }
            ] }"#,
        )
        .unwrap();
        let whole = ToolWindowLayout::reopened(partial);
        assert_eq!(whole.splits, Splits::default());
        assert_eq!(
            ids(&whole, Side::RightTop),
            ["git", "caffeine", "hierarchy", "maven", "gradle", "ant", "npm", "coverage", "problems"],
            "a new tool window goes to its side"
        );
        assert_eq!(whole.get("git").unwrap().size, 640.0, "sizes are brought in range");
        assert!(whole.is_visible("git") && !whole.is_visible("problems"), "one shown tool window a side");
        assert!(!whole.is_visible("run"), "Run starts hidden");
        assert_eq!(ids(&whole, Side::BottomRight), ["database"], "a tool window this version lacks is kept");
        assert!(!whole.is_visible("database"), "hidden, with nothing to draw it");
        assert_eq!(ids(&whole, Side::LeftTop), ["project", "commit", "structure"]);
        assert_eq!(
            ids(&whole, Side::BottomLeft),
            ["find", "debug", "run", "terminal", "services", "todo", "build", "endpoints", "dependencies"]
        );
        assert!(!whole.is_visible("project"), "a tool window the file lacks starts hidden");
        assert_eq!(whole.windows.len(), DECLARED.len() + 1);
    }

    #[test]
    fn older_files_get_the_default_layout_with_their_panels() {
        let layout = ToolWindowLayout::from_shown(&["structure", "terminal"]);
        assert!(layout.is_visible("structure") && layout.is_visible("terminal") && !layout.is_visible("project"));
        assert_eq!(layout.active(), None);
        let none = ToolWindowLayout::from_shown(&[]);
        assert!(none.windows.iter().all(|window| !window.visible));
        let empty: ToolWindowLayout = serde_json::from_str(r#"{ "windows": [] }"#).unwrap();
        assert!(empty.windows.is_empty());
        assert_eq!(ToolWindowLayout::reopened(empty).windows.len(), DECLARED.len());
    }

    #[test]
    fn a_mode_hides_every_tool_window_and_puts_them_back() {
        let mut layout = ToolWindowLayout::default();
        layout.show("terminal");
        layout.move_to("git", Side::RightTop, None);
        layout.show("git");
        layout.set_size_at(Anchor::Bottom, 400.0);
        let before = layout.clone();
        let kept = layout.hide_all();
        assert_eq!(kept, before);
        assert!(layout.windows.iter().all(|window| !window.visible));
        assert_eq!(layout.active(), None);
        assert_eq!(layout.side_of("git"), Some(Side::RightTop), "hidden, the tool windows keep their places");
        assert_eq!(layout.size_at(Anchor::Bottom), 400.0);
        layout.restore(kept.clone());
        assert_eq!(layout, before);

        let kept = layout.hide_all();
        layout.move_to("terminal", Side::RightBottom, None);
        layout.restore(kept);
        assert!(layout.is_visible("terminal") && layout.is_visible("git") && layout.is_visible("project"));
        assert_eq!(layout.side_of("terminal"), Some(Side::RightBottom), "moved while hidden, it shows where it is");
        assert_eq!(layout.active(), Some("git"));
    }

    #[test]
    fn the_edges_fit_a_smaller_window() {
        let mut layout = ToolWindowLayout::default();
        layout.show("terminal");
        layout.move_to("git", Side::RightTop, None);
        layout.show("git");
        layout.set_size_at(Anchor::Bottom, 900.0);
        layout.set_size_at(Anchor::Left, 600.0);
        layout.set_size_at(Anchor::Right, 200.0);
        assert_eq!(layout.fitted_sizes(2000.0, 1200.0), [600.0, 900.0, 200.0], "a large window shows them as they are");
        let [left, bottom, right] = layout.fitted_sizes(960.0, 600.0);
        assert_eq!(bottom, 600.0 - EDITOR_ROOM);
        assert!((left + right - (960.0 - EDITOR_ROOM)).abs() < 1e-3 && (left / right - 3.0).abs() < 1e-3);
        assert_eq!(layout.size_at(Anchor::Bottom), 900.0, "the sizes kept stay for a larger window");
        layout.hide("git");
        assert_eq!(layout.fitted_sizes(960.0, 600.0)[2], 0.0, "an edge that shows nothing");
        assert_eq!(layout.fitted_sizes(100.0, 100.0), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn sides_have_names() {
        for side in Side::ALL {
            assert_eq!(Side::from_name(side.name()), Some(side));
            assert_eq!(serde_json::to_value(side).unwrap(), side.name());
        }
        assert_eq!(Side::from_name("left"), Some(Side::LeftTop));
        assert_eq!(Side::from_name("bottom"), Some(Side::BottomLeft));
        assert_eq!(Side::from_name("right"), Some(Side::RightTop));
        assert_eq!(Side::from_name("top"), None);
    }

    #[test]
    fn view_modes_have_names_and_actions() {
        for mode in ViewMode::ALL {
            assert_eq!(ViewMode::from_name(mode.name()), Some(mode));
            assert_eq!(ViewMode::of_action(mode.action()), Some(mode));
            assert_eq!(serde_json::to_value(mode).unwrap(), mode.name());
        }
        assert_eq!(ViewMode::default(), ViewMode::DockPinned);
        assert_eq!(ViewMode::from_name("floating"), None);
        let docked: Vec<bool> = ViewMode::ALL.iter().map(|mode| mode.docked()).collect();
        assert_eq!(docked, [true, true, false, false, false]);
        let detached: Vec<bool> = ViewMode::ALL.iter().map(|mode| mode.detached()).collect();
        assert_eq!(detached, [false, false, false, true, true]);
    }

    #[test]
    fn floating_and_windowed_tool_windows_leave_their_side_to_the_docked_one() {
        let mut layout = ToolWindowLayout::default();
        assert!(layout.set_mode("project", ViewMode::Float));
        assert!(!layout.set_mode("project", ViewMode::Float), "no change");
        assert!(layout.is_visible("project"), "a shown tool window stays shown in its new mode");
        assert!(layout.visible_at(Side::LeftTop).is_none(), "floating, it is not at its side");
        layout.show("structure");
        assert!(layout.is_visible("project") && layout.is_visible("structure"), "Structure docks beside it");
        assert_eq!(layout.detached().map(|window| window.id.as_str()).collect::<Vec<_>>(), ["project"]);
        assert_eq!(layout.fitted_sizes(2000.0, 1200.0)[0], 330.0, "the left edge holds Structure only");
        layout.set_mode("structure", ViewMode::Window);
        assert_eq!(layout.fitted_sizes(2000.0, 1200.0)[0], 0.0, "nothing docked at the left");
        assert!(!layout.stretch("structure", Direction::Right, STRETCH_STEP, 800.0), "a window is not stretched");
        layout.set_mode("project", ViewMode::DockPinned);
        assert!(layout.is_visible("project") && layout.is_visible("structure"));
        layout.set_mode("structure", ViewMode::DockPinned);
        assert!(layout.is_visible("structure") && !layout.is_visible("project"), "docked again, one a side");
    }

    #[test]
    fn unpinned_tool_windows_hide_on_focus_loss_and_undocked_ones_when_another_opens() {
        let mut layout = ToolWindowLayout::default();
        layout.set_mode("project", ViewMode::DockUnpinned);
        assert!(layout.is_visible("project"));
        assert!(!layout.lose_focus(Some("project")), "a click in it keeps it");
        assert!(layout.lose_focus(None), "a click elsewhere hides it");
        assert!(!layout.is_visible("project"));
        layout.set_mode("terminal", ViewMode::Undock);
        layout.show("terminal");
        assert!(layout.docked_at(Side::BottomLeft).is_none(), "undocked, it does not take the bottom");
        assert_eq!(layout.visible_at(Side::BottomLeft).map(|window| window.id.as_str()), Some("terminal"));
        assert_eq!(layout.fitted_sizes(2000.0, 1200.0)[1], 0.0, "the editor keeps the bottom");
        assert!(!layout.lose_focus(None));
        assert!(layout.is_visible("terminal"), "an undocked one stays while nothing else opens");
        layout.show("project");
        assert!(!layout.is_visible("terminal"), "another tool window opening hides the undocked one");
        layout.show("terminal");
        assert!(!layout.is_visible("project"), "and the unpinned one loses the focus to it");
        layout.set_mode("problems", ViewMode::Float);
        layout.show("problems");
        assert!(!layout.is_visible("terminal"), "a floating one opening takes the focus too");
        layout.set_mode("git", ViewMode::DockPinned);
        layout.show("git");
        assert!(layout.is_visible("problems"), "a pinned or floating one stays");
    }

    #[test]
    fn modes_and_bounds_round_trip_and_bad_bounds_are_dropped() {
        let mut layout = ToolWindowLayout::default();
        layout.set_mode("terminal", ViewMode::Window);
        assert!(layout.set_bounds("terminal", Bounds { x: -1700.0, y: 80.0, width: 640.0, height: 400.0 }));
        assert!(!layout.set_bounds("terminal", Bounds { x: -1700.0, y: 80.0, width: 640.0, height: 400.0 }));
        assert!(!layout.set_bounds("terminal", Bounds { x: f32::NAN, y: 0.0, width: 1.0, height: 1.0 }));
        layout.set_bounds("problems", Bounds { x: 2600.0, y: 10.0, width: 50.0, height: 50.0 });
        assert_eq!(layout.get("problems").unwrap().bounds.unwrap().width, Bounds::MIN[0], "at least the smallest size");
        let json = serde_json::to_value(&layout).unwrap();
        let terminal = json["windows"].as_array().unwrap().iter().find(|window| window["id"] == "terminal").unwrap();
        assert_eq!(terminal["mode"], "window");
        assert_eq!(terminal["bounds"], serde_json::json!({ "x": -1700.0, "y": 80.0, "width": 640.0, "height": 400.0 }));
        let project = json["windows"].as_array().unwrap().iter().find(|window| window["id"] == "project").unwrap();
        assert!(project.get("bounds").is_none());
        let read: ToolWindowLayout = serde_json::from_value(json).unwrap();
        assert_eq!(ToolWindowLayout::reopened(read.clone()).mode_of("terminal"), Some(ViewMode::Window));
        assert_eq!(read.get("terminal").unwrap().bounds, layout.get("terminal").unwrap().bounds);

        let older: ToolWindowLayout = serde_json::from_str(
            r#"{ "windows": [
                { "id": "project", "side": "leftTop", "size": 300, "visible": true },
                { "id": "structure", "side": "leftTop", "size": 300, "visible": true, "mode": "float",
                  "bounds": { "x": 1e40, "y": 0, "width": 300, "height": 300 } }
            ] }"#,
        )
        .unwrap();
        let whole = ToolWindowLayout::reopened(older);
        assert_eq!(whole.mode_of("project"), Some(ViewMode::DockPinned), "a file without modes docks and pins");
        assert!(whole.is_visible("project") && whole.is_visible("structure"), "floating, Structure keeps showing");
        assert_eq!(whole.get("structure").unwrap().bounds, None, "bounds out of range are dropped");
    }

    #[test]
    fn a_saved_layout_shows_its_results_tool_windows() {
        let mut layout = ToolWindowLayout::default();
        layout.set_mode("run", ViewMode::Undock);
        layout.show("run");
        assert!(!ToolWindowLayout::reopened(layout.clone()).is_visible("run"), "a project opens without results");
        let applied = ToolWindowLayout::applied(layout);
        assert!(applied.is_visible("run"), "a layout applied shows them as saved");
        assert_eq!(applied.mode_of("run"), Some(ViewMode::Undock));
    }

    #[test]
    fn layouts_are_saved_under_free_names_and_kept_in_a_file() {
        let mut saved = SavedLayouts::default();
        let mut layout = ToolWindowLayout::default();
        layout.set_mode("project", ViewMode::Float);
        layout.active = Some("project".to_owned());
        assert_eq!(saved.save_as("  Review  ", &layout), Ok("Review".to_owned()));
        assert_eq!(saved.current.as_deref(), Some("Review"));
        assert_eq!(saved.get("review").unwrap().layout.active, None, "the active tool window is not saved");
        assert_eq!(saved.save_as("REVIEW", &layout), Err(SaveError::Taken));
        assert_eq!(saved.save_as("Default", &layout), Err(SaveError::Default));
        assert_eq!(saved.save_as("  ", &layout), Err(SaveError::Empty));
        saved.save_as("Debugging", &ToolWindowLayout::default()).unwrap();
        assert_eq!(saved.current.as_deref(), Some("Debugging"));

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("window-layouts.json");
        saved.save(&file).unwrap();
        let read = SavedLayouts::load(&file);
        assert_eq!(read, saved);
        assert_eq!(read.get("Review").unwrap().layout.mode_of("project"), Some(ViewMode::Float));

        let mut changed = ToolWindowLayout::default();
        changed.set_mode("git", ViewMode::Window);
        assert!(saved.save_changes("review", &changed));
        assert_eq!(saved.get("Review").unwrap().layout.mode_of("git"), Some(ViewMode::Window), "Save Changes");
        assert!(!saved.save_changes("Gone", &changed));
        assert_eq!(saved.rename("Review", "debugging"), Some(Err(SaveError::Taken)));
        assert_eq!(saved.rename("Review", "default"), Some(Err(SaveError::Default)));
        assert_eq!(saved.rename("Gone", "Other"), None);
        assert_eq!(saved.rename("review", " REVIEW "), Some(Ok("REVIEW".to_owned())), "its own name in another case");
        assert_eq!(saved.rename("Debugging", "Debug"), Some(Ok("Debug".to_owned())));
        assert_eq!(saved.current.as_deref(), Some("Debug"), "the current layout keeps being current");
        assert!(saved.delete("debug"));
        assert_eq!(saved.current, None, "deleting the current layout makes the default one current");
        assert!(!saved.delete("debug"));

        std::fs::write(
            &file,
            r#"{ "layouts": [
                { "name": "A", "layout": { "windows": [] } },
                { "name": "a", "layout": { "windows": [] } },
                { "name": "default", "layout": { "windows": [] } },
                { "name": "", "layout": { "windows": [] } }
            ], "current": "Gone" }"#,
        )
        .unwrap();
        let read = SavedLayouts::load(&file);
        assert_eq!(read.layouts.iter().map(|layout| layout.name.as_str()).collect::<Vec<_>>(), ["A"]);
        assert_eq!(read.current, None);
        std::fs::write(&file, "not json").unwrap();
        assert_eq!(SavedLayouts::load(&file), SavedLayouts::default());
        assert_eq!(SavedLayouts::load(&dir.path().join("missing.json")), SavedLayouts::default());
    }
}
