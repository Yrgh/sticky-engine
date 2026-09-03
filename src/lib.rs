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
//! The engine supports async throughout and is executor-agnostic. See
//! [`core::task`] for utilities. The engine's entry point is
//! [`run_main_loop`](prelude::run_main_loop), which must be called on the main
//! thread exactly once.
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
//! functions may fail (see [`GetError`](core::GetError)) or
//! unexpectedly panic unless you respect the borrowing rules.
//!
//! # Dependencies and feature flags
//!
//! The engine has the following mandatory dependencies:
//!
//! - `glamx`
//! - `linkme` (reexported in [`core::relations`])
//! - `tracing`
//! - `winit`
//!
//! Note that it is up to the you to set a [`Subscriber`](tracing::Subscriber)
//! in your application.
//!
//! The following feature flags are available:
//!
//! - **`gpu-vulkan`**: adds a built-in GPU API based on `vulkano`. Enabled by
//!   default.
//!
//! - **`vulkan-renderer`**: adds a default renderer. Requires `gpu-vulkan`.
//!   Enabled by default.
//!
//! - **`simple-asset-impls`**: adds tools for quick integration of the
//!   [`AssetManager`](core::asset::AssetManager). Enabled by default.
//!
//! - **`serde`**: adds a `serde` impl for [`Asset`](core::asset::Asset).
//!
//! - **`ron`**: adds asset loaders and savers for `ron`. Requires `serde`.
//!
//! - **`wincode`**: adds asset loaders and a schema for `Asset`
//!
//! ## Writing a Component
//!
//! Writing a Component by hand is difficult and error-prone, so you should use
//! [`comp_def`] instead. See its documentation for a more detailed overview.
//!
//! ```rust, ignore
//! comp_def! {
//!     struct CExample {
//!         components {
//!             // your child components here...
//!
//!             static rel: /* Component */,
//!             dyn rel2: /* Component or dyn Slot */,
//!             dyn? opt_rel: /* Component or dyn Slot; optional */,
//!             dyn* many: /* Component or dyn Slot; any number */,
//!         }
//!         variables {
//!             // your variables here...
//!         }
//!         behaviors {
//!             // your functions, special or otherwise, here...
//!
//!             #[init] // Mandatory
//!             fn init(
//!                 world: &World,
//!                 parent: ComponentParent,
//!                 self_id: ComponentId<Self>,
//!                 _: () // <- This can be any type you want. It will be made into a parameter on IComponent::spawn
//!             ) -> CExampleInit {
//!                 CExampleInit {
//!                     // Initialize your variables and child Components
//!                 }
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! ## Writing a Slot
//!
//! Slots, too, are complicated and error-prone to write, especially for impls.
//! To define one, add [`#[slot_def]`](slot_def) above your trait. Your trait
//! must be dyn-compatible, but is otherwise unrestricted.
//!
//! ```rust,ignore
//! #[slot_def]
//! pub trait SExample {}
//! ```
//!
//! You will get an error if you try to implement a Slot. You need to add
//! [`#[slot_impl]`](slot_impl) above the impl.
//!
//! ```rust,ignore
//! #[slot_impl]
//! impl SExample for CExample {}
//! ```

#![warn(clippy::all)]
#![allow(clippy::type_complexity)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_fun_call,
    clippy::todo,
    missing_docs
)]

pub use glamx;
pub use tracing;
#[cfg(feature = "gpu-vulkan")]
pub use vulkano;
pub use winit;

pub use sticky_engine_macros::{comp_def, slot_def, slot_impl};

pub mod builtin;
pub mod core;
pub mod prelude;

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod test;
