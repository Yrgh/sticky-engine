# Sticky Engine

Sticky has a few properties that make it a good choice for many games:

1. **Modular**. Everything besides the very fundamentals of the engine uses a plug-in system. If you don't like a builtin feature, switch it.
2. **Compositional**. The engine doesn't try and add inheritance to Rust. Instead, you make each Component (see below) individually and piece them together.
3. **Strongly-type tree**. Similar to game engines like Godot, Sticky features a node tree, however, each Component defines exactly what types and how many children it has, meaning users make fewer mistakes and can rely on statically-typed guarantees.
4. **Type/trait-based iteration**. Despite having a tree, you can iterate over Components based on type or trait.
5. **Rust**. Rust's type system and memory safety grants you rapid development with fewer chances for errors.

## Usage

Add the following to your `Cargo.toml` or run `cargo add sticky-engine`:
```toml
sticky-engine = "0.1.0"
tracing_subscriber = "0.3.23"
```

Then write your main function:
```rust
use sticky_engine::{
    builtin::{
        assets::simple_impl::{FalseAsyncFs, FsAccessor},
        renderer_vk::VkRenderer,
    },
    core::asset::AssetManager,
    prelude::*,
};
use tracing_subscriber::fmt::format::FmtSpan;

fn main() {
    // Set the subscriber
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_span_events(FmtSpan::NONE)
            .with_target(true)
            .finish(),
    )
    .expect("failed to set global subscriber");

    // Build the asset manager
    let mut builder = AssetManager::builder();
    builder
        // FalseAsyncFs is only to remove the need for a runtime here.
        .with_accessor(FsAccessor::<FalseAsyncFs>::new("./"));

    let asset_manager = builder.build();

    // Build the World
    let mut builder = World::builder();
    builder
        .with_renderer::<VkRenderer>(())
        .with_window()
        .with_asset_manager(asset_manager);

    // Run the app
    unsafe {
        run_main_loop(builder, |_| {})
    }
    .expect("main loop failed");
}
```