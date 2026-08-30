# AGENTS.md

Instructions for working on the `sticky-engine` crate.

## Crate organization

- `src/core/` — core engine logic (main loop, components, levels, world, tasks, reflection). Everything in here is internal engine machinery.
- `src/core/component/` — component definitions, IDs (`ids.rs`), and properties (`props.rs`).
- `src/builtin.rs` — built-in, non-core features. Most Components/Slots the engine ships goes here, *not* in `core/`. These may be gated behind feature flags and are reimplementable by users..
- `macros/` — proc-macro workspace member (`comp_def!`, `slot_def!`, `slot_impl!`).
- `examples/*` — usage examples,.

Unless a built-in Component/Slot touches a macro or is used internally by the engine, it must be separated from the `core` module.

## Conventions

- Edition 2024. The library crate has `#![deny(clippy::unwrap_used, clippy::expect_fun_call, clippy::todo, missing_docs)]` — never use `unwrap()`, avoid `expect()` in library code, and document all public items.
- The `World` is `!Send + !Sync` (interior mutability via `RefCell`/`Cell`); interact with it from the main loop or through `task::join_main`.
- Use `tracing` features for logging and spans.

# Decision-making

When approaching a non-trivial problem, choose ONE of these two modes explicitly before writing code or a plan. Do not blend them.

### Mode 1: Commit and Backtrack
- Pick the approach you have the most evidence for. State it in one sentence ("Going with X because Y").
- Execute it **fully**, or until you hit a concrete blocker (a failing test, a contradiction, a borrowing problem, missing capability).
- If you hit a blocker: stop, state what failed and why in 1-2 sentences, then commit to the next approach. This is ONE backtrack, not a running internal debate.
- Budget: at most 2-3 backtracks per task. If you're on a 4th, **stop** and give a brief explanation to the user.

### Mode 2: Branch and Compare
- If the approach genuinely isn't clear, explicitly enumerate a few candidate approaches up front, each in a labeled section (e.g. "Option A: ...", "Option B: ...").
- Refine each approach until it either fails or a clear comparison can be made.
- Pick a winner with a one-line justification and proceed in Mode 1 from here. Don't keep re-litigating rejected options later.

### What NOT to do
- **Do not** use "Hmm, but wait..." / "Actually..." / "Let me reconsider..." as a substitute for either mode. These phrases signal an *unstructured* reversal — the same option being second-guessed without new information.
- A correction is only warranted when you've hit new information (a test result, an error, an aliasing conflict, a spec detail you missed). Re-deriving the same conclusion from the same information is not a valid reason to backtrack.
- Rule of thumb: if you can't name the specific new fact that changed your mind, you haven't earned a backtrack — keep going.
- Target: *fewer than 5 reconsiderations per request*. If you notice yourself exceeding that, stop, name the actual uncertainty in one sentence, and either commit or branch formally per the modes above.

## Rules

- **Use crates**: Before writing something complex, check if there is a crate that does it better. If there is a crate you want to add, ask *immediately*.

