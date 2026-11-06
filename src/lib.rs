//! Convert files to Markdown for LLM and text pipelines.
//!
//! This is an independent Rust port of the MarkItDown idea. It is not a
//! Microsoft project and it does not use Microsoft trademarks. Office documents
//! are read with [`undoc`](https://docs.rs/undoc) and PDFs with
//! [`unpdf`](https://docs.rs/unpdf). Output for those formats follows those
//! libraries.
//!
//! # Example
//!
//! ```
//! use markitdown::{MarkItDown, StreamInfo};
//!
//! let result = MarkItDown::new()
//!     .convert_bytes(b"name,city\nAda,London\n", StreamInfo::new().with_extension("csv"))
//!     .unwrap();
//! assert!(result.markdown.contains("Ada"));
//! ```
//!
//! The library does not sandbox filesystem paths. A path you pass to
//! [`MarkItDown::convert_path`] is opened with the process's own permissions.
//! HTTP fetches reject loopback, private, link-local, and metadata targets.
//! See the README for the exact rules.

mod charset;
mod convert;
mod converters;
mod detect;
mod engine;
mod error;
mod fetch;
mod normalize;
mod save;
mod stream_info;

pub use convert::{
    ConvertContext, Converter, DocumentConverterResult, Options, PRIORITY_GENERIC,
    PRIORITY_SPECIFIC,
};
pub use engine::MarkItDown;
pub use error::{Error, FailedAttempt, Result};
pub use save::{beside_source_path, save_markdown, OutputDestination, SaveError, SavedMarkdown};
pub use stream_info::StreamInfo;

/// Crate version, from `Cargo.toml`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
