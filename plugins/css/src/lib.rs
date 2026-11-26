mod abi;
mod color;
mod complete;
mod data;
mod highlight;
mod json;
mod structure;
mod token;
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
    token::Dialect::Css
}
