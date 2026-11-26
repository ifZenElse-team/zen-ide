pub mod brackets;
mod carets;
mod editing;
pub mod expand_word;
pub mod folding;
pub mod lines;
mod movement;
pub mod occurrences;
pub mod regions;
mod selection;
pub mod soft_wrap;
mod structure;
pub mod unwrap;

pub use brackets::{BRACKETS, BracketMatch, bracket_pairs, matching_bracket};
pub use carets::{Direction, column_head, column_selection};
pub use editing::{EditOp, edit};
pub use folding::{Fold, FoldKind, doc_comment_names, doc_comment_names_in, folds};
pub use movement::{Movement, double_click_word, hump_at};
pub use occurrences::occurrences;
pub use regions::{
    CustomRegion, RegionComments, RegionMarker, RegionStyle, custom_regions, custom_regions_where, region_marker,
    with_custom_regions,
};
pub use selection::{Selection, Selections};
pub use soft_wrap::LineWrap;
pub use structure::{Structure, StructureOptions, Token, TokenKind};
pub use unwrap::{UnwrapKind, UnwrapOption, unwrap_options};
pub use zen_text::search::{
    Query, SearchContext, escape as escape_regex, find_all, replacement, replacements, replacements_within,
};
