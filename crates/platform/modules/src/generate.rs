use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub struct GenerateContext<'a> {
    pub path: &'a Path,
    pub text: &'a str,
    pub offset: usize,
    pub indent: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub id: String,
    pub title: String,
    pub chooser: bool,
    pub action: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Insertion {
    pub at: usize,
    pub end: Option<usize>,
    pub text: String,
    pub also: Vec<(std::ops::Range<usize>, String)>,
}

pub trait CodeGenerator: Send + Sync {
    fn handles(&self, file_name: &str) -> bool;

    fn offers(&self, context: &GenerateContext<'_>) -> Vec<Offer>;

    fn pending(&self, _context: &GenerateContext<'_>, _id: &str) -> bool {
        false
    }

    fn chooser(&self, context: &GenerateContext<'_>, id: &str) -> Option<Chooser>;

    fn generate(&self, context: &GenerateContext<'_>, id: &str, chosen: &Chosen) -> Result<Insertion, String>;
}

/// What a generator asks the person to choose before it writes code: the members it can use and its options.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Chooser {
    /// The dialog's title, e.g. `Choose Fields to Initialize by Constructor`.
    pub title: String,
    /// The fields or methods to choose from, in the language's order.
    pub members: Vec<Member>,
    /// The generator's own choices, e.g. whether to insert `@Override`.
    pub options: Vec<ChooserOption>,
    /// Whether the code can be generated with no member chosen.
    pub allow_empty: bool,
    /// Whether the person picks one member from a long list by searching, instead of checking members.
    #[serde(default)]
    pub search: bool,
}

/// A field or method the chooser offers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    /// What `editor.generate.apply` takes in `members` to choose it.
    pub id: String,
    /// The member's name as the tree shows it, e.g. `name` or `equals(Object)`.
    pub name: String,
    /// What is shown after the name, usually its type.
    #[serde(default)]
    pub detail: Option<String>,
    /// The type or class it belongs to; the tree shows members under it.
    #[serde(default)]
    pub group: Option<String>,
    /// Whether it is chosen when the chooser opens.
    #[serde(default)]
    pub selected: bool,
}

/// One of a generator's own choices, a check box under the members.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChooserOption {
    /// What `editor.generate.apply` takes in `options`.
    pub id: String,
    /// The check box's text.
    pub label: String,
    /// Whether it is on when the chooser opens.
    pub value: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Chosen {
    pub members: Vec<String>,
    pub options: BTreeMap<String, bool>,
}

impl Chosen {
    pub fn option(&self, chooser: &Chooser, id: &str) -> bool {
        self.options
            .get(id)
            .copied()
            .unwrap_or_else(|| chooser.options.iter().any(|option| option.id == id && option.value))
    }
}

#[derive(Clone, Default)]
pub struct CodeGenerators(Vec<Arc<dyn CodeGenerator>>);

impl CodeGenerators {
    pub fn add(&mut self, generator: Arc<dyn CodeGenerator>) {
        self.0.push(generator);
    }

    pub fn for_file(&self, file_name: &str) -> Vec<Arc<dyn CodeGenerator>> {
        self.0.iter().filter(|generator| generator.handles(file_name)).cloned().collect()
    }
}
