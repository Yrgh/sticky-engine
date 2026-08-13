//! Important imports such as macros, traits, and the founations of the engine.

pub use super::engine::{
    world::World,
    component::{ComponentId, ComponentParent, DynComponentId, IComponent, ISlotId},
    level::LevelIndex,
    main_loop::{run_main_loop, queue_quit},
    task::{join_main, spawn},
};
pub use super::{comp_def, slot_def, slot_impl};
