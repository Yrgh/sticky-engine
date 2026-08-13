//! TODO: Rename this thing
//! 
//! TODO: Top-level docs

#![warn(clippy::all)]
#![deny(clippy::unwrap_used, clippy::expect_fun_call, clippy::todo, missing_docs)]

pub use macros::{comp_def, slot_def, slot_impl};

pub mod builtin;
pub mod engine;
pub mod logging;
pub mod prelude;
