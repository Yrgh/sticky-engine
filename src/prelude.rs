//! Important imports such as macros, traits, and the foundations of the engine.

pub use super::core::{
    component::{ComponentId, ComponentParent, DynComponentId, IComponent, ISlotId},
    level::LevelId,
    main_loop::{queue_quit, run_main_loop},
    task::{join_main, dispatch_main},
    trans::*,
    world::World,
    asset::{Asset, AssetManager},
    engine_sync::EngineSync,
};
pub use super::{comp_def, slot_def, slot_impl};
