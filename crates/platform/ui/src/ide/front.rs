use zen_core::Session;

use super::IdeView;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FrontKind {
    Diff,
    Folder,
    Bundle,
}

impl FrontKind {
    fn stands_alone(self) -> bool {
        matches!(self, FrontKind::Folder | FrontKind::Bundle)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Front {
    pub(super) kind: FrontKind,
    pub(super) over: (Option<usize>, u64),
}

impl IdeView {
    pub(super) fn front_kind_is(&self, kind: FrontKind) -> bool {
        self.front.is_some_and(|front| front.kind == kind)
    }

    pub(super) fn bring_front(&mut self, kind: FrontKind, session: &Session) {
        if kind.stands_alone() {
            self.behind = None;
        } else if let Some(front) = self.front.filter(|front| front.kind.stands_alone()) {
            self.behind = Some(front);
        }
        self.front = Some(Front { kind, over: Self::front_document(session) });
    }

    pub(super) fn drop_front(&mut self, kind: FrontKind) -> bool {
        if self.front_kind_is(kind) {
            self.front = self.behind.take();
            return true;
        }
        if self.behind.is_some_and(|behind| behind.kind == kind) {
            self.behind = None;
        }
        false
    }

    pub(super) fn is_in_front(&self, kind: FrontKind, session: &Session) -> bool {
        self.front_kind_is(kind)
            && self.front.is_some_and(|front| front.over == Self::front_document(session))
            && match kind {
                FrontKind::Diff => self.diff.is_some() && !self.diff_from_workspace,
                FrontKind::Folder => self.folder_diff.is_some(),
                FrontKind::Bundle => self.bundle.is_some(),
            }
    }

    pub(super) fn settle_front(&mut self, session: &Session, covered: bool) {
        let document = Self::front_document(session);
        if self.behind.is_some_and(|behind| behind.over != document) {
            self.behind = None;
        }
        let Some(front) = self.front else { return };
        let stale = if front.kind.stands_alone() { front.over != document } else { !covered };
        if stale && !self.is_in_front(front.kind, session) {
            self.front = self.behind.take();
        }
    }

    pub(super) fn move_front(&mut self, before: (Option<usize>, u64), now: (Option<usize>, u64)) {
        for front in [&mut self.front, &mut self.behind].into_iter().flatten() {
            if front.over == before && !front.kind.stands_alone() {
                front.over = now;
            }
        }
    }
}
