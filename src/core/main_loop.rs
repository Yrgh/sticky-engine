//! The engine entry point
//!
//! The engine is driven by the main loop, and Components can only be accessed
//! through it. Async code must [join](crate::core::task::join_main) the main loop in order to
//! interact with the [`World`].
//!
//! See [`run_main_loop`].

use std::{
    any::Any,
    sync::OnceLock,
    time::{Duration, Instant},
};

use anyhow::Result as AResult;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
};

use futures::channel::{mpsc, oneshot};

use crate::core::{
    component::ISlotId,
    world::{World, WorldBuilder},
};

pub(crate) type Abstract = Box<dyn Any + Send + Sync>;
type InitFn = Box<dyn FnOnce(&World) + 'static>;

pub(crate) enum MainJob {
    ExecAsync {
        work: Box<dyn FnOnce(&World) -> Abstract + Send + 'static>,
        send: oneshot::Sender<Abstract>,
    },
    ExecSilent {
        work: Box<dyn FnOnce(&World) + Send + 'static>,
    },
    Quit,
}

pub(crate) static MAIN_QUEUE_ASYNC: OnceLock<mpsc::Sender<MainJob>> = OnceLock::new();
pub(crate) static MAIN_QUEUE_SYNC: OnceLock<std::sync::mpsc::Sender<MainJob>> = OnceLock::new();

struct MainLoop {
    jobs_async: mpsc::Receiver<MainJob>,
    jobs_sync: std::sync::mpsc::Receiver<MainJob>,

    world: World,
    world_builder: WorldBuilder,
    init_fn: Option<InitFn>,

    stable_accumulator: Instant,

    last_idle_time: Instant,
}

impl MainLoop {
    fn idle(&mut self, timeout: Option<Duration>) -> bool {
        let start = Instant::now();
        while timeout.is_none_or(|t| start.elapsed() < t) {
            // Clear out sync first, since that is unbounded
            let job = match self.jobs_sync.try_recv() {
                Ok(j) => j,
                // If sync is empty, work on async.
                Err(std::sync::mpsc::TryRecvError::Empty) => match self.jobs_async.try_recv() {
                    Ok(j) => j,
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Closed) => unreachable!(),
                },
                Err(std::sync::mpsc::TryRecvError::Disconnected) => unreachable!(),
            };

            match job {
                MainJob::ExecAsync { work, send } => {
                    // No work if future is dropped.
                    if !send.is_canceled() {
                        // Fails if the future is dropped while work is processing.
                        let _ = send.send(work(&self.world));
                    }
                }
                MainJob::ExecSilent { work } => work(&self.world),
                MainJob::Quit => return true,
            }
        }
        false
    }

    fn render_idle(&mut self) {
        let _span = tracing::debug_span!("engine_render").entered();

        todo!()
    }

    fn physics_handling(&mut self) -> bool {
        let mut iters = 0;
        while iters < STABLE_SPIRAL_LIMIT && Instant::now() > self.stable_accumulator {
            let span = tracing::debug_span!("stable_iteration", iteration = iters);

            for level in self.world.iter_levels().filter(|l| l.is_active()) {
                for id in level.iter_top_level() {
                    let mut comp = id
                        .get_mut(&self.world)
                        .expect("component was just acquired");
                    comp.pre_phys(&self.world, self.world.get_stable_tick_rate().as_secs_f32());
                }
            }

            tracing::trace_span!(parent: &span, "physics_iteration", iteration = iters).in_scope(
                || {
                    // TODO: Physics
                },
            );

            for level in self.world.iter_levels().filter(|l| l.is_active()) {
                for id in level.iter_top_level() {
                    let mut comp = id
                        .get_mut(&self.world)
                        .expect("component was just acquired");
                    comp.post_phys(&self.world, self.world.get_stable_tick_rate().as_secs_f32());
                }
            }

            self.stable_accumulator += self.world.get_stable_tick_rate();
            iters += 1;

            if self.idle(Some(Duration::from_micros(250))) {
                return true;
            }
        }

        false
    }
}

const STABLE_SPIRAL_LIMIT: usize = 8;

// --------------- READ ME ---------------
//
// World::set_active_event_loop *must* be paired with an
// unset_active_event_loop. Every possible return statement needs one.

impl ApplicationHandler for MainLoop {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: winit::event::StartCause) {
        unsafe { self.world.set_active_event_loop(event_loop) };

        if cause == winit::event::StartCause::Init {
            let _span = tracing::info_span!("engine_init").entered();

            match self.world.complete_init(&mut self.world_builder) {
                Ok(()) => {}
                Err(e) => {
                    tracing::error!("failed to initialize the engine: {e}");
                    self.world.unset_active_event_loop();
                    event_loop.exit();
                    return;
                }
            }

            if let Some(init) = self.init_fn.take() {
                let _span = tracing::debug_span!("user_init").entered();
                init(&self.world);
            }
        }

        self.world.unset_active_event_loop();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        unsafe { self.world.set_active_event_loop(event_loop) };

        if let Some(id) = self.world.window_by_os_id(window_id) {
            let mut window = self
                .world
                .get_window_mut_int(id)
                .expect("just got the window ID from the World");

            match event {
                WindowEvent::CloseRequested => {
                    if self.world.is_main_window(id) {
                        event_loop.exit();
                        drop(window);
                        self.world.unset_active_event_loop();
                        return;
                    }

                    drop(window);
                    self.world.handle_window_event(id, &event);
                }
                WindowEvent::RedrawRequested => match window.draw() {
                    Ok(()) => {}
                    Err(e) => tracing::error!(window_id = ?id, "failed to draw to window: {e}"),
                },
                WindowEvent::Resized(new_size) => {
                    window.on_resize(&self.world, new_size);
                }
                event => {
                    drop(window);
                    self.world.handle_window_event(id, &event);
                }
            }
        } else {
            tracing::warn!(?window_id, "winit event for unknown window");
        }

        if self.idle(Some(Duration::from_micros(250))) {
            event_loop.exit();
        }

        self.world.unset_active_event_loop();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        unsafe { self.world.set_active_event_loop(event_loop) };

        // Release any present fences the GPU has finished with, both before and
        // after doing frame work this cycle.
        if let Some(gpu_api) = self.world.get_gpu_api() {
            gpu_api.cleanup_in_flight();
        }

        unsafe { self.world.flush_actions() };

        if self.physics_handling() {
            self.world.unset_active_event_loop();
            event_loop.exit();
            return;
        }

        if self.last_idle_time.elapsed() >= self.world.get_idle_min_delay()
            && self.world.try_acquire_any_swapchain()
        {
            let delta = self.last_idle_time.elapsed().as_secs_f32();
            self.last_idle_time = Instant::now();

            for level in self.world.iter_levels() {
                for id in level.iter_top_level() {
                    id.get_mut(&self.world)
                        .expect("just acquired")
                        .idle(&self.world, delta);
                }
            }

            // Only do rendering work when it is at all possible.
            if self.world.get_renderer().is_some() {
                self.render_idle();
            }
        }

        if self.idle(None) {
            self.world.unset_active_event_loop();
            event_loop.exit();
            return;
        }

        if let Some(gpu_api) = self.world.get_gpu_api() {
            gpu_api.cleanup_in_flight();
        }

        self.world.unset_active_event_loop();
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let _span = tracing::debug_span!("application_resumed").entered();

        unsafe { self.world.set_active_event_loop(event_loop) };

        for (_id, mut window) in self.world.iter_windows_mut_int() {
            window.resume();
        }

        self.world.unset_active_event_loop();
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        unsafe { self.world.set_active_event_loop(event_loop) };

        for (_id, mut window) in self.world.iter_windows_mut_int() {
            window.suspend();
        }

        self.world.unset_active_event_loop();
    }
}

const MAIN_QUEUE_ASYNC_LEN: usize = 8;

/// Entry point for the engine.
///
/// `init_fn` is called when the main loop has initialized fully.
///
/// The main loop is what controls the whole engine, from the [`World`] to the
/// renderer to all stable-tick logic. The [`World`], is not shareable across
/// threads, so all operations with Components must be routed through the main loop.
///
/// See [`task`](crate::core::task) for working with async code.
///
/// To exit the main loop, call [`queue_quit`].
///
/// # Safety
///
/// This function must be called exactly once on the main thread. The main loop
/// provides a [`tokio`] runtime, so do not use `#[tokio::main]`.
pub unsafe fn run_main_loop(
    world_builder: WorldBuilder,
    init_fn: impl FnOnce(&World) + 'static,
) -> AResult<()> {
    if cfg!(test) {
        panic!("Cannot run the main loop during a test. Write an example");
    }

    let (job_async_tx, job_async_rx) = mpsc::channel(MAIN_QUEUE_ASYNC_LEN);
    let (job_sync_tx, job_sync_rx) = std::sync::mpsc::channel();

    let mut main_loop = MainLoop {
        jobs_async: job_async_rx,
        jobs_sync: job_sync_rx,

        world: world_builder.finish_ish(),
        world_builder,

        init_fn: Some(Box::new(init_fn)),

        stable_accumulator: Instant::now(),

        last_idle_time: Instant::now(),
    };

    MAIN_QUEUE_ASYNC
        .set(job_async_tx)
        .expect("no other sources should set MAIN_QUEUE_ASYNC");

    MAIN_QUEUE_SYNC
        .set(job_sync_tx)
        .expect("no other sources should set MAIN_QUEUE_SYNC");

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
    event_loop.run_app(&mut main_loop)?;

    Ok(())
}

/// Error returned when the main loop has ended or hasn't started.
pub struct MainClosedError;

pub(crate) fn queue(job: MainJob) -> Result<(), MainClosedError> {
    let Some(mq) = MAIN_QUEUE_SYNC.get() else {
        return Err(MainClosedError);
    };

    mq.send(job).map_err(|_| MainClosedError)?;

    Ok(())
}

pub(crate) async fn queue_async(job: MainJob) -> Result<(), MainClosedError> {
    use futures::SinkExt;
    let Some(mut mq) = MAIN_QUEUE_ASYNC.get().cloned() else {
        return Err(MainClosedError);
    };

    mq.send(job).await.map_err(|_| MainClosedError)?;

    Ok(())
}

/// Queues the main loop to quit.
///
/// If you can use `.await`, use [`queue_quit_async`] instead.
pub fn queue_quit() -> Result<(), MainClosedError> {
    queue(MainJob::Quit)
}

/// Queues the main loop to quit, but using the async queue.
///
/// This function is preferred in async contexts.
pub async fn queue_quit_async() -> Result<(), MainClosedError> {
    queue_async(MainJob::Quit).await
}
