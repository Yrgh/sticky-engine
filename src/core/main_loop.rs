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
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use anyhow::{Result as AResult, bail};
use vulkano::{command_buffer::PrimaryCommandBufferAbstract, sync::GpuFuture};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::WindowAttributes,
};

use tokio::sync::{mpsc, oneshot};

use crate::{
    core::{
        component::ISlotId,
        level::LevelIndex,
        world::{WORLD_PTR, World},
    },
    log,
};

type Abstract = Box<dyn Any + Send + Sync>;
type InitFn = Box<dyn FnOnce() + 'static>;

pub(crate) enum MainJob {
    Exec {
        work: Box<dyn FnOnce() -> Abstract + Send + Sync + 'static>,
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
                        let _ = send.send(work());
                    }
                }
                MainJob::Quit => return true,
            }
        }
        false
    }

    fn render_idle(&mut self) -> AResult<()> {
        // Collect things to render

        let vk_ctx = self.world.get_vk().expect("vulkan is not initialized");

        let mut render_tasks =
            self.world
                .iter_levels()
                .try_fold(Vec::new(), |mut acc, level| -> AResult<_> {
                    let rq = level.update_rendering_queue();

                    let deps = rq.search_dependencies();
                    let (idle_commands, prq) = rq.build(vk_ctx.clone())?;

                    acc.push((level.id(), deps, idle_commands, prq));
                    Ok(acc)
                })?;

        // Kahn's
        {
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

                for dependent in dependents.get(&level).into_iter().flatten() {
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

        let mut prqs = HashMap::new();

        let queue = vk_ctx.queues[0].clone();

        render_tasks
            .into_iter()
            .try_fold(
                vulkano::sync::now(vk_ctx.device.clone()).boxed_send_sync(),
                |acc, (lidx, _deps, cb, prq)| -> AResult<_> {
                    let exec = cb.execute_after(acc, queue.clone())?;

                    Ok(if let Some(mut prq) = prq {
                        let fence = Arc::new(exec.then_signal_fence());
                        prq.exec_after = Some(fence.clone());
                        prqs.insert(lidx, prq);
                        fence.boxed_send_sync()
                    } else {
                        exec.boxed_send_sync()
                    })
                },
            )?
            .flush()?;

        for mut window in self.world.iter_windows_mut_int() {
            let lidx = window.level();
            window.set_prq(prqs.remove(&lidx).expect("window missing a prq"));

            if let Some(os) = window.as_os() {
                os.request_redraw();
            }
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
                init();
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
                    log!(dbg: "ignoring close request for a non-main window");
                }
                WindowEvent::RedrawRequested => {
                    window.draw();
                }
                WindowEvent::Resized(new_size) => {
                    window.on_resize(&self.world, new_size);
                }
                event => {
                    // Drop window since handle_window_event creates a new borrow
                    drop(window);
                    self.world.handle_window_event(id, &event)
                }
            }
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
                    let mut comp = id.get_mut().expect("component was just acquired");
                    comp.pre_phys_hook(self.world.get_stable_tick_rate().as_secs_f32());
                }
            }

            // TODO: Physics

            for level in self.world.iter_levels() {
                for id in level.iter_top_level() {
                    let mut comp = id.get_mut().expect("component was just acquired");
                    comp.post_phys_hook(self.world.get_stable_tick_rate().as_secs_f32());
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

        if self.last_idle_time.elapsed() >= self.world.get_idle_min_delay() {
            let delta = self.last_idle_time.elapsed().as_secs_f32();
            self.last_idle_time = Instant::now();

            for level in self.world.iter_levels() {
                for id in level.iter_top_level() {
                    id.get_mut().expect("just acquired").idle_hook(delta);
                }
            }

            // Only do rendering work when it is at all possible.
            if self.world.get_vk().is_some() {
                self.render_idle().expect("failed to render levels");
            }
        }

        if self.idle(None) {
            event_loop.exit();
        }

        self.world.unset_active_event_loop();
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        unsafe { self.world.set_active_event_loop(event_loop) };

        for mut window in self.world.iter_windows_mut_int() {
            window.resume();
        }

        self.world.unset_active_event_loop();
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        unsafe { self.world.set_active_event_loop(event_loop) };

        for mut window in self.world.iter_windows_mut_int() {
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
pub unsafe fn run_main_loop(init_fn: impl FnOnce() + 'static, headless: bool) -> AResult<()> {
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

    WORLD_PTR.set(&main_loop.world);

    RT_HANDLE
        .set(main_loop.tokio_rt.handle().clone())
        .expect("no other sources should set RT_HANDLE");

    MAIN_QUEUE
        .set(job_tx)
        .expect("no other sources should set MAIN_QUEUE");

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
    event_loop.run_app(&mut main_loop)?;

    WORLD_PTR.set(std::ptr::null());

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
