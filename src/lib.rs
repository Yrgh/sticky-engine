//! Sticky game engine
//! 
//! Sticky offers a Rust-based *strictly-typed tree*. In most engines with a
//! tree, any node can be the child of any other. In Sticky, a node decides both
//! the types and number of nodes it has as children. Furthermore, Sticky allows
//! you to iterate by type like an ECS if you wish.
//! 
//! Sticky works off the idea of a *"Component"*, a single node in the tree.
//! Each Component type defines what types and how many children it has.
//! Components all implement the [`IComponent`](core::component::IComponent)
//! trait and are created using the [`comp_def`] function-like macro.
//! 
//! Components can be grouped by *"Slots"*, traits with a few special
//! properties. Each Slot comes with an ID type that can be used instead of
//! [`DynComponentId`](core::component::DynComponentId) and
//! [`ComponentId<C>`](core::component::ComponentId). Slots are defined with the
//! [`slot_def`] attribute macro on the trait definition. Implementing a Slot
//! for a Component requires the [`slot_impl`] attribute macro on the
//! non-generic impl.
//! 
//! Components are grouped into [`Level`](core::level::Level)s, which are all
//! contained by the [`World`](core::world::World). The `World` holds the entire
//! state of the engine, but is not shareable across threads. The engine relies
//! heavily on [`Cell`](std::cell::Cell) and [`RefCell`](std::cell::RefCell) for
//! handing data around without blocking.
//! 
//! The engine supports async based on [`tokio`]. See [`core::task`] for
//! utilities. The engine's entry point is
//! [`run_main_loop`](prelude::run_main_loop), which must be called on the main
//! thread exactly once and provides a runtime. Do not add the [`tokio::main`]
//! attribute to your `main` function.
//! 
//! Rendering happens through [`Window`](core::window)s. Each window owns a
//! [`Level`](core::level::Level). On-screen windows are represented by
//! [`RootWindow`](core::window::RootWindow)
//! 
//! # Reading the documentation
//! 
//! Many functions will have a "Borrows" section that describes which Components
//! or other [`RefCell`](std::cell::RefCell)s the function will or may borrow,
//! and whether the borrow is immutable or mutable. These are warnings, as some
//! functions may fail (see [`ComponentGetError`](core::ComponentGetError)) or
//! unexpectedly panic unless you respect the borrowing rules.

#![warn(clippy::all)]
#![deny(clippy::unwrap_used, clippy::expect_fun_call, clippy::todo, missing_docs)]

pub use rapier3d;

pub use sticky_engine_macros::{comp_def, slot_def, slot_impl};

pub mod builtin;
pub mod core;
pub mod logging;
pub mod prelude;

