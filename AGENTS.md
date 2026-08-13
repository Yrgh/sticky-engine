# AGENTS.md

Instructions for working on the `component-engine` crate.

## Crate organization

- `src/engine/` — core engine logic (main loop, components, levels, world, tasks, reflection). Everything in here is internal engine machinery.
- `src/engine/component/` — component definitions, IDs (`ids.rs`), and properties (`props.rs`).
- `src/builtin.rs` — built-in, non-core features. Any Component/Slot the engine ships goes here, *not* in `engine/`. These may be gated behind feature flags and are reimplementable by users.
- `src/logging.rs` — the `log!` macro and logging utilities.
- `src/prelude.rs` — important re-exports (macros, traits, `World`, etc.).
- `macros/` — proc-macro workspace member (`comp_def!`, `slot_def!`, `slot_impl!`).
- `src/main.rs` — binary entry point / usage example.

Unless a built-in Component/Slot touches a macro or is used internally by the engine, it must be separated from the `engine` module.

## Conventions

- Edition 2024. The library crate has `#![deny(clippy::unwrap_used, clippy::expect_fun_call, clippy::todo, missing_docs)]` — never use `unwrap()`/`expect()` in library code, and document all public items.
- The `World` is `!Send + !Sync` (interior mutability via `RefCell`/`Cell`); interact with it from the main loop or through `task::join_main`.
- Use `log!` (`err:`, `wrn:`, `dbg:`) for logging instead of `println!`/`eprintln!` except in examples.

## Rules

- **Use crates**: Don't write complex code if a crate can do it better, but don't add depenedencies without permission.
