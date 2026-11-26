use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::Session;
use zen_syntax::SymbolKind;

pub trait SearchContributor: Send + Sync {
    fn id(&self) -> &str;

    fn title(&self) -> &str;

    fn has_tab(&self) -> bool {
        true
    }

    fn weight(&self) -> i32;

    fn primary(&self) -> bool {
        true
    }

    fn fallback(&self) -> bool {
        false
    }

    fn commands(&self) -> bool {
        false
    }

    fn search(&self, session: &mut Session, query: &SearchQuery, sink: &mut SearchSink) -> SearchProgress;
}

#[derive(Clone, Debug)]
pub struct SearchQuery {
    pub text: String,
    pub all: bool,
    pub include_non_project: bool,
    pub limit: usize,
    pub scope: Option<String>,
    pub text_options: TextOptions,
    pub cancel: CancelToken,
}

/// How the Text tab matches: its Cc, W and .* switches and its file mask.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TextOptions {
    /// Match case (Cc).
    pub match_case: bool,
    /// Whole words only (W).
    pub whole_words: bool,
    /// A regular expression (.*).
    pub regex: bool,
    /// File masks, comma-separated (`*.go, !*_test.go`); every file when empty.
    pub mask: String,
}

#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Debug)]
pub struct SearchSink {
    items: Vec<SearchItem>,
    limit: usize,
    cancel: CancelToken,
}

impl SearchSink {
    pub fn new(limit: usize, cancel: CancelToken) -> Self {
        Self { items: Vec::new(), limit, cancel }
    }

    pub fn push(&mut self, item: SearchItem) -> bool {
        if self.is_closed() {
            return false;
        }
        self.items.push(item);
        !self.is_closed()
    }

    pub fn is_closed(&self) -> bool {
        self.items.len() >= self.limit || self.cancel.is_cancelled()
    }

    pub fn into_items(self) -> Vec<SearchItem> {
        self.items
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchProgress {
    Done,
    Pending(&'static str),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchItem {
    pub name: String,
    pub detail: String,
    pub hint: Option<String>,
    pub ranges: Vec<[usize; 2]>,
    pub detail_ranges: Vec<[usize; 2]>,
    pub in_project: bool,
    pub available: bool,
    pub target: SearchTarget,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SearchTarget {
    File {
        path: PathBuf,
        #[serde(skip_serializing_if = "Option::is_none")]
        line: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        column: Option<u32>,
    },
    Directory {
        path: PathBuf,
    },
    Place {
        path: PathBuf,
        offset: usize,
        line: u32,
        symbol: SymbolKind,
    },
    Action {
        action: String,
        command: String,
        opens: Option<String>,
        params: serde_json::Value,
    },
    Settings {
        page: String,
    },
    Switch {
        on: bool,
        command: String,
        params: serde_json::Value,
    },
    Value {
        value: String,
    },
    Run {
        command: String,
        params: serde_json::Value,
        icon: String,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TypeNoun {
    #[default]
    Class,
    Type,
}

#[derive(Clone, Default)]
pub struct SearchContributors(Vec<Arc<dyn SearchContributor>>);

impl SearchContributors {
    pub fn add(&mut self, contributor: Arc<dyn SearchContributor>) {
        if !self.0.iter().any(|other| other.id() == contributor.id()) {
            self.0.push(contributor);
        }
    }

    pub fn list(&self) -> &[Arc<dyn SearchContributor>] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str) -> SearchItem {
        SearchItem {
            name: name.to_owned(),
            detail: String::new(),
            hint: None,
            ranges: Vec::new(),
            detail_ranges: Vec::new(),
            in_project: true,
            available: true,
            target: SearchTarget::File { path: PathBuf::from(name), line: None, column: None },
        }
    }

    #[test]
    fn a_sink_takes_up_to_its_limit_and_nothing_once_cancelled() {
        let cancel = CancelToken::default();
        let mut sink = SearchSink::new(2, cancel.clone());
        assert!(sink.push(item("a")));
        assert!(!sink.push(item("b")), "full after the second");
        assert!(!sink.push(item("c")));
        assert_eq!(sink.into_items().len(), 2);
        let mut sink = SearchSink::new(5, cancel.clone());
        cancel.cancel();
        assert!(!sink.push(item("a")));
        assert!(sink.into_items().is_empty());
    }

    #[test]
    fn items_serialise_with_their_target_kind() {
        let value = serde_json::to_value(item("main.go")).unwrap();
        assert_eq!(value["target"]["kind"], "file");
        assert_eq!(value["inProject"], true);
    }
}
