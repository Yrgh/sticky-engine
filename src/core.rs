//! Engine features
//!
//! These are the *core* components of the engine, including the main loop,
//! Components, and reflection, amongst others. Unless it has to touch a macro
//! or is used internally by the engine, any built-in Components or Slots must
//! be separated from the `engine` module.

use thiserror::Error;

pub mod component;
pub mod gpu_api;
pub mod input;
pub mod level;
pub mod main_loop;
pub mod math;
pub mod relations;
pub mod rendering;
pub mod task;
pub mod trans;
pub mod util;
pub mod window;
pub mod world;

/// Error returned when a Component is acquired immutably from an ID.
#[derive(Error, Debug)]
pub enum ComponentGetError {
    /// When the ID is invalid or out of date.
    #[error("component not found")]
    NotFound,
    /// When the ID was valid, but the Component couldn't be borrowed
    #[error("component borrowed mutably")]
    BorrowError(#[from] std::cell::BorrowError),
}

/// Error returned when a Component is acquired mutably from an ID.
#[derive(Error, Debug)]
pub enum ComponentGetMutError {
    /// When the ID is invalid or out of date.
    #[error("component not found")]
    NotFound,
    /// When the ID was valid, but the Component couldn't be borrowed
    #[error("component borrowed mutably")]
    BorrowMutError(#[from] std::cell::BorrowMutError),
}
