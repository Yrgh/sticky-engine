//! Important imports such as macros, traits, and the foundations of the engine.

pub use super::core::{
    component::{ComponentId, ComponentParent, DynComponentId, IComponent, ISlotId},
    level::LevelIndex,
    main_loop::{queue_quit, run_main_loop},
    task::{join_main},
    trans::*,
    world::World,
};
pub use super::{comp_def, slot_def, slot_impl};
