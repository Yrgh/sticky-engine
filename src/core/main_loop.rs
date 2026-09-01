//! The engine entry point
//!
//! The engine is driven by the main loop, and Components can only be accessed
//! through it. Async code must [join](crate::core::task::join_main) the main loop in order to
//! interact with the [`World`].
//! 
//! There are two ways to create a main loop:
//! 
//! - For applications: [`run_main_loop`].
//! 
//! - For tests: [`ManualDriver::new`].

use std::{
    any::Any,
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

use thiserror::Error;
use winit::{
    application::ApplicationHandler,
    error::{EventLoopError, OsError},
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
};

use futures::channel::{mpsc, oneshot};

use crate::core::{
    component::ISlotId, engine_sync::EngineSync, level::LevelId, world::{CompleteInitError, GpuCreateMode, MainMode, World, WorldBuilder},
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

struct MainLoop {
    driver: ManualDriver,

    world_builder: WorldBuilder,
    init_fn: Option<InitFn>,

    stable_accumulator: Instant,
    last_idle_time: Instant,
}

#[derive(Debug, Error)]
enum RenderError {
    #[error("renderer error: {0}")]
    RendererError(anyhow::Error),
    #[error("cyclic dependencies between Levels")]
    LevelDependencyCycle,
}

impl MainLoop {
    fn render(&mut self) -> Result<(), RenderError> {
        let _span = tracing::debug_span!("engine_render").entered();

        // Step 1: Figure out the order to render in.
        // Step 2: Collect the things to render
        // Step 3: Submit and distribute

        let Some(renderer) = self.driver.world.get_renderer() else {
            unreachable!()
        };

        let mut level_to_parts = self
            .driver
            .world
            .iter_levels()
            .filter(|l| l.is_active())
            .map(|l| -> Result<_, RenderError> {
                let rq = l.get_rendering_queue();
                let deps = rq.search_dependencies();
                let (lvl_instructions, win_instructions) = renderer
                    .render_level(&rq)
                    .map_err(RenderError::RendererError)?;
                Ok((l.id(), (deps, lvl_instructions, win_instructions)))
            })
            .collect::<Result<HashMap<_, _>, RenderError>>()?;

        // Kahn's algorithm
        let n = level_to_parts.len();
        let mut dependents: HashMap<LevelId, Vec<_>> = HashMap::with_capacity(n);
        let mut in_degrees = HashMap::with_capacity(n);

        for (lvl, (deps, _, _)) in &level_to_parts {
            in_degrees.insert(*lvl, deps.len());
            for dep in deps {
                dependents.entry(*dep).or_default().push(*lvl);
            }
        }

        let mut queue: VecDeque<_> = in_degrees
            .iter()
            .filter_map(|(lvl, d)| (*d == 0).then_some(*lvl))
            .collect();

        let mut order = Vec::with_capacity(n);

        while let Some(lvl) = queue.pop_front() {
            order.push(lvl);
            if let Some(dependents) = dependents.get(&lvl) {
                for dependent in dependents {
                    let Some(deg) = in_degrees.get_mut(dependent) else {
                        panic!("dependent should exist in in_degrees");
                    };

                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(*dependent);
                    }
                }
            }
        }

        if order.len() != n {
            return Err(RenderError::LevelDependencyCycle);
        }

        for lvl in &order {
            let Some(parts) = level_to_parts.get_mut(lvl) else {
                panic!("level should exist in level_to_parts");
            };

            if let Some(win_instructions) = parts.2.take() {
                let Some(level) = self.driver.world.get_level(*lvl) else {
                    panic!("level index should be valid because it came from iter_levels");
                };

                if let Some(win) = level.get_window() {
                    let Some(mut window) = self.driver.world.get_window_mut_int(win) else {
                        continue;
                    };

                    window.set_instructions(win_instructions);

                    if let Some(os) = window.as_os() {
                        os.request_redraw();
                    }
                }
            }
        }

        let mut level_instructions = order
            .into_iter()
            .filter_map(|lvl| Some(level_to_parts.remove(&lvl)?.1));

        renderer
            .submit_level_instructions(&mut level_instructions)
            .map_err(RenderError::RendererError)?;

        Ok(())
    }

    fn physics_handling(&mut self) -> bool {
        let mut iters = 0;
        while iters < STABLE_SPIRAL_LIMIT && Instant::now() > self.stable_accumulator {
            let tick_rate = self.driver.world.engine().get_stable_tick_rate();

            let _span = tracing::debug_span!("stable_iteration", iteration = iters);

            if self
                .driver
                .physics_internal(tick_rate, Some(Duration::from_micros(200)))
            {
                return true;
            }

            self.stable_accumulator += tick_rate;
            iters += 1;
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
        unsafe { self.driver.world.set_active_event_loop(event_loop) };

        if cause == winit::event::StartCause::Init {
            let _span = tracing::info_span!("engine_init").entered();

            match self
                .driver
                .world
                .complete_init(&mut self.world_builder)
            {
                Ok(()) => {}
                Err(e) => {
                    tracing::error!("failed to initialize the engine: {e}");
                    self.driver.world.unset_active_event_loop();
                    event_loop.exit();
                    return;
                }
            }

            if let Some(init) = self.init_fn.take() {
                let _span = tracing::debug_span!("user_init").entered();
                init(&self.driver.world);
            }
        }

        self.driver.world.unset_active_event_loop();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        unsafe { self.driver.world.set_active_event_loop(event_loop) };

        if let Some(id) = self.driver.world.window_by_os_id(window_id) {
            let mut window = self
                .driver
                .world
                .get_window_mut_int(id)
                .expect("just got the window ID from the World");

            match event {
                WindowEvent::CloseRequested => {
                    if self.driver.world.is_main_window(id) {
                        event_loop.exit();
                        drop(window);
                        self.driver.world.unset_active_event_loop();
                        return;
                    }

                    drop(window);
                    self.driver.world.handle_window_event(id, &event);
                }
                WindowEvent::RedrawRequested => match window.draw() {
                    Ok(()) => {}
                    Err(e) => tracing::error!(window_id = ?id, "failed to draw to window: {e}"),
                },
                WindowEvent::Resized(new_size) => {
                    window.on_resize(&self.driver.world, new_size);
                }
                event => {
                    drop(window);
                    self.driver.world.handle_window_event(id, &event);
                }
            }
        } else {
            tracing::warn!(?window_id, "winit event for unknown window");
        }

        if self.driver.downtime(Some(Duration::from_micros(150))) {
            event_loop.exit();
        }

        self.driver.world.unset_active_event_loop();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        unsafe { self.driver.world.set_active_event_loop(event_loop) };

        // Release any present fences the GPU has finished with, both before and
        // after doing frame work this cycle.
        if let Some(gpu_api) = self.driver.world.get_gpu_api() {
            gpu_api.cleanup_in_flight();
        }

        unsafe { self.driver.world.flush_actions() };

        if self.physics_handling() {
            self.driver.world.unset_active_event_loop();
            event_loop.exit();
            return;
        }

        if self.last_idle_time.elapsed() >= self.driver.world.engine().get_idle_min_delay()
            && self.driver.world.try_acquire_any_swapchain()
        {
            let delta = self.last_idle_time.elapsed().as_secs_f32();
            self.last_idle_time = Instant::now();

            if self
                .driver
                .idle_internal(delta, Some(Duration::from_micros(200)))
            {
                self.driver.world.unset_active_event_loop();
                event_loop.exit();
                return;
            }

            // Only do rendering work when it is at all possible.
            if self.driver.world.get_renderer().is_some() {
                match self.render() {
                    Ok(()) => {}
                    Err(e) => tracing::error!("rendering failed: {e}"),
                }
            }
        }

        if let Some(gpu_api) = self.driver.world.get_gpu_api() {
            gpu_api.cleanup_in_flight();
        }

        self.driver.world.unset_active_event_loop();
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let _span = tracing::debug_span!("application_resumed").entered();

        unsafe { self.driver.world.set_active_event_loop(event_loop) };

        for (_id, mut window) in self.driver.world.iter_windows_mut_int() {
            window.resume();
        }

        self.driver.world.unset_active_event_loop();
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        unsafe { self.driver.world.set_active_event_loop(event_loop) };

        for (_id, mut window) in self.driver.world.iter_windows_mut_int() {
            window.suspend();
        }

        self.driver.world.unset_active_event_loop();
    }
}

const MAIN_QUEUE_ASYNC_LEN: usize = 8;

#[derive(Debug, Error)]
#[allow(missing_docs)]
/// Error returned by [`run_main_loop`].
pub enum RunMainLoopError {
    #[error("winit error: {0}")]
    WinitOsError(#[from] OsError),
    #[error("event loop error: {0}")]
    EventLoopError(#[from] EventLoopError),
    #[error("no asset manager set in builder")]
    /// Returned if the [`WorldBuilder`] did not have an `AssetManager` set.
    NoAssetManager,
}

/// Entry point for the engine.
///
/// `init_fn` is called when the main loop has initialized fully.
///
/// The main loop is what controls the whole engine, from the [`World`] to the
/// renderer to all stable-tick logic. The `World`, is not shareable across
/// threads, so all operations with Components must be routed through the main loop.
///
/// See [`task`](crate::core::task) for working with async code.
///
/// To exit the main loop, call [`queue_quit`].
///
/// # Safety
///
/// This functions must be called **exactly** once from the main thread.
///
/// For `tokio` users, build a `Runtime` manually and dispatch through a handle.
/// Don't use `#[tokio::main]`.
pub unsafe fn run_main_loop(
    mut world_builder: WorldBuilder,
    init_fn: impl FnOnce(&World) + 'static,
) -> Result<(), RunMainLoopError> {
    if cfg!(test) {
        panic!("Cannot run the main loop during a test. Write an example");
    }

    let (job_async_tx, job_async_rx) = mpsc::channel(MAIN_QUEUE_ASYNC_LEN);
    let (job_sync_tx, job_sync_rx) = std::sync::mpsc::channel();

    let engine = EngineSync {
        stable_rate: world_builder.stable_rate.into(),
        min_idle_delay: world_builder.min_idle_delay.into(),

        asset_manager: world_builder.asset_manager.take().ok_or(RunMainLoopError::NoAssetManager)?,

        main_queue_async: job_async_tx,
        main_queue_sync: job_sync_tx,
    };

    let mut main_loop = MainLoop {
        driver: ManualDriver {
            jobs_async: job_async_rx,
            jobs_sync: job_sync_rx,

            world: world_builder.finish_ish(Arc::new(engine)),
        },
        
        world_builder,
        init_fn: Some(Box::new(init_fn)),

        stable_accumulator: Instant::now(),
        last_idle_time: Instant::now(),
    };

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
    let run_result = event_loop.run_app(&mut main_loop);

    // Ensure the main loop drops before the event_loop because of potential
    // errors on close.
    drop(main_loop);

    run_result?;

    Ok(())
}

#[derive(Debug, Error)]
#[error("main loop closed")]
/// Error returned when the main loop has ended or hasn't started.
pub struct MainClosedError;

/// Queues the main loop to quit.
///
/// If you can use `.await`, use [`queue_quit_async`] instead.
pub fn queue_quit(engine: &EngineSync) -> Result<(), MainClosedError> {
    engine.queue_job_sync(MainJob::Quit)
}

/// Queues the main loop to quit, but using the async queue.
///
/// This function is preferred in async contexts.
pub async fn queue_quit_async(engine: &EngineSync) -> Result<(), MainClosedError> {
    engine.queue_job_async(MainJob::Quit).await
}

/// Manual driver of the "main loop" for testing.
///
/// When writing tests, you don't want your code to be at the whim of `winit`
/// and the event loop, so this driver allows you to manually send out physics
/// ticks and idle ticks to test how your code works.
/// 
/// Create one for testing using [`ManualDriver::new`], just how you would with
/// [`run_main_loop`].
pub struct ManualDriver {
    jobs_async: mpsc::Receiver<MainJob>,
    jobs_sync: std::sync::mpsc::Receiver<MainJob>,

    world: World,
}

#[derive(Debug, Error)]
#[allow(missing_docs)]
/// Error returned by [`ManualDriver::new`].
pub enum ManualDriverNewError {
    #[error("winit error: {0}")]
    WinitOsError(#[from] OsError),
    #[error("event loop error: {0}")]
    EventLoopError(#[from] EventLoopError),
    #[error("no asset manager set in builder")]
    /// Returned if the [`WorldBuilder`] did not have an `AssetManager` set.
    NoAssetManager,
    #[error("attempted to create a GPU API")]
    GpuApiSet,
    #[error("attempted to create a main window")]
    MainWindow,
}

impl ManualDriver {
    fn downtime(&mut self, timeout: Option<Duration>) -> bool {
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

    fn physics_internal(
        &mut self,
        tick_rate: Duration,
        downtime_timeout: Option<Duration>,
    ) -> bool {
        for level in self.world.iter_levels().filter(|l| l.is_active()) {
            for id in level.iter_top_level() {
                let mut comp = id
                    .get_mut(&self.world)
                    .expect("component was just acquired");
                comp.pre_phys(&self.world, tick_rate.as_secs_f32());
            }
        }

        if self.downtime(downtime_timeout) {
            return true;
        }

        // TODO: Physics sim

        for level in self.world.iter_levels().filter(|l| l.is_active()) {
            for id in level.iter_top_level() {
                let mut comp = id
                    .get_mut(&self.world)
                    .expect("component was just acquired");
                comp.post_phys(&self.world, tick_rate.as_secs_f32());
            }
        }

        self.downtime(downtime_timeout)
    }

    fn idle_internal(&mut self, delta: f32, downtime_timeout: Option<Duration>) -> bool {
        for level in self.world.iter_levels().filter(|l| l.is_active()) {
            for id in level.iter_top_level() {
                let mut comp = id
                    .get_mut(&self.world)
                    .expect("component was just acquired");
                comp.idle(&self.world, delta);
            }
        }

        self.downtime(downtime_timeout)
    }
}

impl ManualDriver {
    /// Create a new `ManualDriver` for testing.
    /// 
    /// There are a few restrictions:
    /// 
    /// - `world_builder` cannot attempt to create a GPU API or renderer. This
    ///   will return an error.
    /// 
    /// - `world_builder` cannot attempt to create a main window.
    /// 
    /// - There is no active event loop, so no creating windows after
    ///   initialization.
    /// 
    /// There a few upsides:
    /// 
    /// - Complete, manual control over when things happen
    /// 
    /// - Complete automated testing
    /// 
    /// - All other functionality remains
    pub fn new(
        mut world_builder: WorldBuilder,
        init_fn: impl FnOnce(&World) + 'static,
    ) -> Result<Self, ManualDriverNewError> {
        if !matches!(world_builder.gpu_create_mode, GpuCreateMode::Dont) {
            return Err(ManualDriverNewError::GpuApiSet);
        }

        if matches!(world_builder.main_mode, MainMode::Window) {
            return Err(ManualDriverNewError::MainWindow);
        }
        
        let (job_async_tx, job_async_rx) = mpsc::channel(MAIN_QUEUE_ASYNC_LEN);
        let (job_sync_tx, job_sync_rx) = std::sync::mpsc::channel();

        let engine = EngineSync {
            stable_rate: world_builder.stable_rate.into(),
            min_idle_delay: world_builder.min_idle_delay.into(),
    
            asset_manager: world_builder.asset_manager.take().ok_or(ManualDriverNewError::NoAssetManager)?,
    
            main_queue_async: job_async_tx,
            main_queue_sync: job_sync_tx,
        };

        let mut self_ = Self {
            jobs_async: job_async_rx,
            jobs_sync: job_sync_rx,
        
            world: world_builder.finish_ish(Arc::new(engine)),
        };

        match self_.world.complete_init(&mut world_builder) {
            Ok(()) => {},
            Err(CompleteInitError::MainWindowError(_)) => unreachable!(),
            Err(CompleteInitError::RendererInitError(_)) => unreachable!(),
        }
        
        init_fn(&self_.world);

        Ok(self_)
    }

    /// Tick the physics simulation.
    /// 
    /// This will send `pre_phys` and `post_phys` events surrounding the physics
    /// sim, just how it would periodically through [`run_main_loop`]. It will
    /// use the [stable tick rate](EngineSync::get_stable_tick_rate) set in the
    /// [`EngineSync`] as the delta time.
    /// 
    /// Returns whether a quit was requested. You do not have to honor it, but
    /// it may have cancelled some actions.
    pub fn tick_physics(&mut self) -> bool {
        self.physics_internal(self.world.engine().get_stable_tick_rate(), None)
    }

    /// Emit an idle tick.
    /// 
    /// This will send `idle` events with the given `delta`. `Level`s will not
    /// collect render data.
    /// 
    /// Returns whether a quit was requested. You do not have to honor it, but
    /// it may have cancelled some actions.
    pub fn tick_idle(&mut self, delta: f32) -> bool {
        self.idle_internal(delta, None)
    }

    /// Returns the [`World`] this driver owns.
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Returns the [`EngineSync`] the [`World`] owns, pre-cloned.
    /// 
    /// This is a shortcut for `self.world().engine().clone()`.
    pub fn engine(&self) -> Arc<EngineSync> {
        self.world.engine().clone()
    }
}
