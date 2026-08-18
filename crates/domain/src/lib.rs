//! Core domain values shared across Rill modules.

mod source;
mod user;

pub use source::{
    CollectionEntryCandidate, ItemShape, NormalizedDocument, RawMedia, RawSourceItem, SourceKind,
};
pub use user::{Role, User};
