use crate::{Error, Exit, GridPoint, HostedSession, Options, Screen, SelectionKind, Size};

#[derive(Debug)]
pub struct Session {
    _never: std::convert::Infallible,
}

impl Session {
    pub fn spawn(_options: Options, _waker: impl Fn() + Send + Sync + 'static) -> Result<Session, Error> {
        Err(Error::Unsupported)
    }

    pub fn open(
        _host: &crate::Host,
        _options: Options,
        _waker: impl Fn() + Send + Sync + 'static,
    ) -> Result<Session, Error> {
        Err(Error::Unsupported)
    }

    pub fn attach(
        _host: &crate::Host,
        _hosted: HostedSession,
        _size: Size,
        _waker: impl Fn() + Send + Sync + 'static,
    ) -> Result<Session, Error> {
        Err(Error::Unsupported)
    }

    pub fn elsewhere(
        _host: &crate::Host,
        _hosted: HostedSession,
        _size: Size,
        _text: &str,
        _waker: impl Fn() + Send + Sync + 'static,
    ) -> Result<Session, Error> {
        Err(Error::Unsupported)
    }

    pub fn hosted(&self) -> Option<HostedSession> {
        match self._never {}
    }

    pub fn is_elsewhere(&self) -> bool {
        match self._never {}
    }

    pub fn close(&self) {}

    pub fn set_keep(&self, _keep: bool) {}

    pub fn lost_host(&self) -> bool {
        match self._never {}
    }

    pub fn write(&self, _bytes: impl Into<Vec<u8>>) {}

    pub fn resize(&self, _size: Size) {}

    pub fn size(&self) -> Size {
        match self._never {}
    }

    pub fn scroll(&self, _lines: i32) {}

    pub fn scroll_to_bottom(&self) {}

    pub fn snapshot(&self) -> Screen {
        match self._never {}
    }

    pub fn start_selection(&self, _kind: SelectionKind, _at: GridPoint) {}

    pub fn extend_selection(&self, _to: GridPoint) {}

    pub fn clear_selection(&self) {}

    pub fn select_all(&self) {}

    pub fn has_selection(&self) -> bool {
        match self._never {}
    }

    pub fn selection_text(&self) -> Option<String> {
        match self._never {}
    }

    pub fn line_count(&self) -> usize {
        match self._never {}
    }

    pub fn lines_text(&self, _from: usize, _to: usize) -> String {
        match self._never {}
    }

    pub fn title(&self) -> Option<String> {
        match self._never {}
    }

    pub fn exit(&self) -> Option<Exit> {
        match self._never {}
    }

    pub fn has_exited(&self) -> bool {
        match self._never {}
    }
}
