//! Engine features
//!
//! These are the *core* components of the engine, including the main loop,
//! Components, and reflection, amongst others. Unless it has to touch a macro
//! or is used internally by the engine, any built-in Components or Slots must
//! be separated from the `engine` module.

pub mod component;
pub mod input;
pub mod level;
pub mod main_loop;
pub mod relations;
pub mod renderer;
pub mod task;
pub mod trans;
pub mod vk;
pub mod window;
pub mod world;
