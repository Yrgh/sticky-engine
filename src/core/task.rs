//! Async and multithreading utilities
//!
//! Because Components and the [`World`] run on the main thread, an async task
//! cannot interact with the rest of the engine without a channel. However, the
//! [`join_main`] and [`dispatch_main`] functions allow your task or thread to
//! join the main loop and gain access to the `World`.

use std::any::Any;

use futures::channel::oneshot;

use crate::core::{
    engine_sync::EngineSync, main_loop::{MainClosedError, MainJob}, world::World,
};

/// Runs a closure on the main thread with access to the [`World`], returning
/// the result.
///
/// Components, and therefore [`Level`](crate::core::level::Level)s and the
/// `World`, are [`!Send`](Send) + [`!Sync`](Sync), due to being stored in
/// [`RefCell`](std::cell::RefCell)s. In a task, you may want to update
/// something in a Component. You could use a channel, or you could use this.
/// 
/// If you don't want to wait on the result, or you can't use `.await`, use
/// [`dispatch_main`] instead.
///
/// # Cancel safety
/// If the returned future is canceled, the work will not be run.
///
/// # Panics
/// If the function is called outside of the main loop.
/// 
/// # Example
/// 
/// ```rust,no_run
/// # use sticky_engine::{prelude::*, core};
/// # use std::sync::Arc;
/// # struct Mesh {}
/// # impl core::asset::AutoAsset for Mesh {}
/// async fn place_mesh(engine: &EngineSync, level: LevelId) -> anyhow::Result<()> {
///     let mesh: Asset<Mesh> = engine.asset_manager.get_asset_async("mesh.glb").await?;
/// 
///     join_main(move |world| -> anyhow::Result<()> {
///         world
///             .get_level(level)
///             .ok_or_else(|| anyhow::anyhow!("level not found: {level:?}"))?
///             // ...
/// # ;
///         Ok(())
///     }).await??;
/// 
///     Ok(())
/// }
/// ```
pub async fn join_main<T: Any + Send + Sync, F: FnOnce(&World) -> T + Send + Sync + 'static>(
    engine: &EngineSync,
    f: F,
) -> Result<T, MainClosedError>  {
    let (tx, rx) = oneshot::channel();

    engine.queue_job_async(MainJob::ExecAsync {
        work: Box::new(|w| Box::new(f(w))),
        send: tx
    }).await?;

    let result = rx.await.map_err(|_| MainClosedError)?;

    let Ok(result) = result.downcast() else {
        unreachable!()
    };

    Ok(*result)
}

/// Runs a closure on the main thread with access to the [`World`] without
/// waiting for a result.
///
/// Unlike [`join_main`], `dispatch_main` does not return a future. This means
/// it can be run outside of async contexts, but the updates will not be
/// reflected after the function call. It can also be used to create an
/// arbitrary deferred action.
/// 
/// Note: the main loop only runs jobs like those produced by this function for
/// a short amount of time between actions like events. Calling a blocking
/// function can starve other jobs, and even prevent the engine from quitting.
///
/// # Panics
/// If the function is called outside of the main loop.
pub fn dispatch_main<F: FnOnce(&World) + Send + 'static>(engine: &EngineSync, f: F) -> Result<(), MainClosedError> {
    engine.queue_job_sync(MainJob::ExecSilent {
        work: Box::new(f)
    })?;

    Ok(())
}
