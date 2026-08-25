//! Async utilities
//! 
//! While [`tokio::spawn`] is normally a great tool for creating tasks, it
//! doesn't work outside of a runtime. [`spawn`] works regardless of where it is
//! called, as long as the main loop is still running.
//! 
//! Because Components and the [`World`](crate::core::world::World) run on the
//! main thread, an async task cannot interact with the rest of the engine
//! without a channel. However, the [`join_main`] function allows your task to
//! join the main loop and gain access to the `World`.

use std::any::Any;

use tokio::sync::oneshot;

use crate::core::{
    main_loop::{MAIN_QUEUE, MainJob, RT_HANDLE},
};

/// Runs a closure on the main thread with access to the [`World`](crate::core::world::World).
///
/// Components, and therefore [`Level`](crate::core::level::Level)s and the
/// `World`, are [`!Send`](Send) + [`!Sync`](Sync), due to being stored in
/// [`RefCell`](std::cell::RefCell)s. In a task, you may want to update
/// something in a Component. You could use a channel, or you could use this.
///
/// # Panics
/// If the function is called outside of the main loop.
pub async fn join_main<T: Any + Send + Sync, F: FnOnce() -> T + Send + Sync + 'static>(
    f: F,
) -> T {
    let mq = MAIN_QUEUE.get().expect("main loop not initialized yet");
    let (tx, rx) = oneshot::channel();
    mq.send(MainJob::Exec {
        work: Box::new(|| Box::new(f())),
        send: tx,
    })
    .await
    .expect("main loop no longer initialized");

    let result = rx.await.expect("main loop ended while in flight");

    let Ok(result) = result.downcast() else {
        unreachable!()
    };

    *result
}

/// Spawns a future on the runtime.
///
/// This works identically to [`tokio::spawn`], except it doesn't require the function be called
/// from inside the context of a [`Runtime`](tokio::runtime::Runtime).
///
/// # Panics
/// If the function is called outside of the main loop.
pub fn spawn<F: Future<Output: Send + 'static> + Send + 'static>(
    f: F,
) -> tokio::task::JoinHandle<F::Output> {
    let handle = RT_HANDLE.get().expect("main loop not initialized yet");
    handle.spawn(f)
}
