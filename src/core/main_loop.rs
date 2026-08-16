//! The engine entry point
//!
//! The engine is driven by the main loop, and Components can only be accessed
//! through it. Async code must [join](crate::core::task::join_main) the main loop in order to
//! interact with the [`World`].
//!
//! See [`run_main_loop`].

use std::{
    any::Any,
    collections::{HashMap, VecDeque},
    sync::OnceLock,
    time::{Duration, Instant},
};

use anyhow::{Result as AResult, bail};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::WindowAttributes,
};

use tokio::sync::{mpsc, oneshot};

use crate::{
    core::{component::ISlotId, level::LevelIndex, world::World},
    log,
};

type Abstract = Box<dyn Any + Send + Sync>;
type InitFn = Box<dyn FnOnce(&World) + 'static>;

pub(crate) enum MainJob {
    Exec {
        work: Box<dyn FnOnce(&World) -> Abstract + Send + Sync + 'static>,
        send: oneshot::Sender<Abstract>,
    },
    Quit,
}

pub(crate) static RT_HANDLE: OnceLock<tokio::runtime::Handle> = OnceLock::new();
pub(crate) static MAIN_QUEUE: OnceLock<mpsc::Sender<MainJob>> = OnceLock::new();

struct MainLoop {
    tokio_rt: tokio::runtime::Runtime,
    jobs: mpsc::Receiver<MainJob>,

    world: World,

    init_fn: Option<InitFn>,
    headless: bool,

    stable_accumulator: Instant,

    #[expect(unused)]
    last_idle_time: Instant,
}

impl MainLoop {
    fn idle(&mut self, timeout: Option<Duration>) -> bool {
        let start = Instant::now();
        while timeout.is_none_or(|t| start.elapsed() < t) {
            let job = match self.jobs.try_recv() {
                Ok(j) => j,
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    panic!("something assigned to MAIN_QUEUE")
                }
            };

            match job {
                MainJob::Exec { work, send } => {
                    // No work if future is dropped.
                    if !send.is_closed() {
                        // Fails if the future is dropped while work is processing.
                        let _ = send.send(work(&mut self.world));
                    }
                }
                MainJob::Quit => return true,
            }
        }
        false
    }

    fn render_idle(&mut self) -> AResult<()> {
        for id in self.world.iter_window_ids() {
            let mut win = self
                .world
                .get_window_mut(id)
                .expect("window was just acquired");
            win.before_draw();
        }

        let mut render_tasks =
            self.world
                .iter_levels()
                .try_fold(Vec::new(), |mut acc, level| -> AResult<_> {
                    let rq = level.update_rendering_queue();
                    let deps = rq.search_dependencies();
                    let (idle_commands, prq) = rq.build(self.world.get_vk().expect("vulkan is not initialized"))?;

                    // TODO: Populate render_queue

                    acc.push((
                        level.id(),
                        deps,
                        idle_commands,
                        prq
                    ));
                    Ok(acc)
                })?;

        {
            // Kahn's
            let n = render_tasks.len();
            let mut in_degree = HashMap::with_capacity(n);
            let mut dependents: HashMap<LevelIndex, Vec<LevelIndex>> = HashMap::with_capacity(n);

            for (level, dependencies, _, _) in &render_tasks {
                in_degree.insert(*level, dependencies.len());
                for j in dependencies {
                    dependents.entry(*j).or_default().push(*level);
                }
            }

            let mut queue = VecDeque::from_iter(
                in_degree
                    .iter()
                    .filter_map(|(l, d)| (*d == 0).then_some(*l)),
            );

            let mut order = Vec::with_capacity(n);

            while let Some(level) = queue.pop_front() {
                order.push(level);

                for dependent in &dependents[&level] {
                    let Some(indeg) = in_degree.get_mut(dependent) else {
                        continue;
                    };

                    *indeg -= 1;
                    if *indeg == 0 {
                        queue.push_back(*dependent);
                    }
                }
            }

            if order.len() != n {
                bail!("cycle dependency detected between levels");
            }

            render_tasks.sort_by_cached_key(|(lidx, _, _, _)| {
                order
                    .iter()
                    .position(|lidx2| lidx2 == lidx)
                    .unwrap_or(usize::MAX)
            });
        }

        

        Ok(())
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
            // First init code goes here

            // Create main window
            if !self.headless {
                if let Err(e) = self.world.init_vk(super::vk::InitializationOptions {
                    event_loop: Some(event_loop),
                }) {
                    log!(err: "failed to initialize vulkano: {e}");
                    event_loop.exit();

                    self.world.unset_active_event_loop();
                    return;
                }

                // TODO: User inputs these
                let attrs = WindowAttributes::default()
                    .with_title("Sticky Engine")
                    .with_inner_size(PhysicalSize::new(1280, 720));

                match self.world.create_root_window(attrs) {
                    Ok(owned) => {
                        let id = owned.handle();
                        let level = self
                            .world
                            .get_window(id)
                            .expect("window was just created")
                            .level();
                        self.world.set_main_window(id, level);
                        owned.leak();
                    }
                    Err(e) => {
                        log!(err: "failed to create main window: {e}");
                        event_loop.exit();

                        self.world.unset_active_event_loop();
                        return;
                    }
                }
            }

            if let Some(init) = self.init_fn.take() {
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
            if matches!(&event, WindowEvent::CloseRequested) {
                if self.world.is_main_window(id) {
                    event_loop.exit();
                    self.world.unset_active_event_loop();
                    return;
                }
                log!(dbg: "ignoring close request for a non-main window");
            }
            self.world.handle_window_event(id, &event);
        } else {
            log!(wrn: "event from unknown window {window_id:?}");
        }

        if self.idle(Some(Duration::from_micros(250))) {
            event_loop.exit();
        }

        self.world.unset_active_event_loop();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        unsafe { self.world.set_active_event_loop(event_loop) };

        self.world.flush_actions();

        let mut iters = 0;
        while iters < STABLE_SPIRAL_LIMIT && Instant::now() > self.stable_accumulator {
            for level in self.world.iter_levels() {
                for id in level.iter_top_level() {
                    let mut comp = id
                        .get_mut(&self.world)
                        .expect("component was just acquired");
                    comp.pre_phys_hook(
                        &self.world,
                        self.world.get_stable_tick_rate().as_secs_f32(),
                    );
                }
            }

            // TODO: Physics

            for level in self.world.iter_levels() {
                for id in level.iter_top_level() {
                    let mut comp = id
                        .get_mut(&self.world)
                        .expect("component was just acquired");
                    comp.post_phys_hook(
                        &self.world,
                        self.world.get_stable_tick_rate().as_secs_f32(),
                    );
                }
            }

            self.stable_accumulator += self.world.get_stable_tick_rate();
            iters += 1;

            if self.idle(Some(Duration::from_micros(250))) {
                event_loop.exit();
                self.world.unset_active_event_loop();
                return;
            }
        }

        let delta = self.world.get_stable_tick_rate().as_secs_f32();
        for level in self.world.iter_levels() {
            for id in level.iter_top_level() {
                id.get_mut(&self.world)
                    .expect("just acquired")
                    .idle_hook(&self.world, delta);
            }
        }

        // Only do rendering work when it is at all possible.
        if self.world.get_vk().is_some() {
            self.render_idle().expect("failed to render levels");
        }

        if self.idle(None) {
            event_loop.exit();
        }

        self.world.unset_active_event_loop();
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        unsafe { self.world.set_active_event_loop(event_loop) };

        for mut window in self.world.iter_windows_mut() {
            window.resume();
        }

        self.world.unset_active_event_loop();
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        unsafe { self.world.set_active_event_loop(event_loop) };

        for mut window in self.world.iter_windows_mut() {
            window.suspend();
        }

        self.world.unset_active_event_loop();
    }
}

const MAIN_QUEUE_LEN: usize = 16;

/// Entry point for the engine.
///
/// `init_fn` is called when the main loop has initialized.
///
/// If `headless` is set, a main level will be created, but no main window. It
/// is still possible to open other windows later. Additionally, you will have
/// to manually request [`vulkano`] to be initialized. If `headless` is false, a
/// main window will be created that owns the main level.
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
/// This function must be called exactly once on the main thread. The main loop provides a [`tokio`]
/// runtime, so do not use `#[tokio::main]`.
pub unsafe fn run_main_loop(init_fn: impl FnOnce(&World) + 'static, headless: bool) -> AResult<()> {
    let (job_tx, job_rx) = mpsc::channel(MAIN_QUEUE_LEN);
    let mut main_loop = MainLoop {
        tokio_rt: tokio::runtime::Runtime::new()?,
        jobs: job_rx,

        world: if headless {
            World::new_headless()
        } else {
            World::new_empty()
        },

        init_fn: Some(Box::new(init_fn)),
        headless,

        stable_accumulator: Instant::now(),

        last_idle_time: Instant::now(),
    };

    RT_HANDLE
        .set(main_loop.tokio_rt.handle().clone())
        .expect("no other sources should set RT_HANDLE");

    MAIN_QUEUE
        .set(job_tx)
        .expect("no other sources should set MAIN_QUEUE");

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
    event_loop.run_app(&mut main_loop)?;

    Ok(())
}

/// Queues the main loop to quit.
pub fn queue_quit() {
    RT_HANDLE
        .get()
        .expect("main loop not running")
        .spawn(async {
            MAIN_QUEUE
                .get()
                .expect("main loop not running")
                .send(MainJob::Quit)
                .await
                .expect("main loop not running");
        });
}
