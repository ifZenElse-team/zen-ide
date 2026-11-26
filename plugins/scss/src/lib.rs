#[path = "../../css/src/abi.rs"]
mod abi;
#[path = "../../css/src/color.rs"]
mod color;
#[path = "../../css/src/complete.rs"]
mod complete;
#[path = "../../css/src/data.rs"]
mod data;
#[path = "../../css/src/highlight.rs"]
mod highlight;
#[path = "../../css/src/json.rs"]
mod json;
#[path = "../../css/src/structure.rs"]
mod structure;
#[path = "../../css/src/token.rs"]
mod token;
#[path = "../../css/src/tree.rs"]
mod tree;

pub use abi::{
    colors_request, complete_request, encode, encode_folds, zen_abi, zen_alloc, zen_colors, zen_complete, zen_fold,
    zen_highlight, zen_outline,
};
pub use complete::{Completions, Item, complete};
pub use highlight::{Kind, Span, highlight};
pub use json::{completions_json, outline_json};
pub use structure::{Fold, Symbol, fold, outline};

pub(crate) fn dialect() -> token::Dialect {
    token::Dialect::Scss
}
