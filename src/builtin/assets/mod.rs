//! Asset builtins, including simple accessors and cachers and loaders/savers
//! for various crates.

#[cfg(feature = "simple-asset-impls")]
pub mod simple_impl;

#[cfg(feature = "ron")]
pub mod ron;

#[cfg(feature = "wincode")]
pub mod wincode;