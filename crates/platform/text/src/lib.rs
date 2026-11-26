mod buffer;
mod edit;
mod error;
pub mod line_separator;
pub mod search;
mod text;

pub use buffer::{Buffer, EditKind};
pub use edit::Edit;
pub use error::TextError;
pub use line_separator::LineSeparator;
pub use text::{Point, Text};
