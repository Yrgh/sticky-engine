//! Engine features
//! 
//! These are the *core* components of the engine, including the main loop,
//! Components, and reflection, amongst others. Unless it has to touch a macro
//! or is used internally by the engine, any built-in Components or Slots must
//! be separated from the `engine` module.

pub mod component;
pub mod level;
pub mod main_loop;
pub mod relations;
pub mod task;
pub mod world;
pub mod trans;
