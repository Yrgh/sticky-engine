//! The engine entry point
//! 
//! The engine is driven by the main loop, and Components can only be accessed
//! through it. Async code must [join](crate::engine::task::join_main) the main loop in order to
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
    event_loop::{ActiveEventLoop, EventLoop},
};

use tokio::sync::{mpsc, oneshot};

use crate::engine::{component::ISlotId, world::World};

type Abstract = Box<dyn Any + Send + Sync>;

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
}

const STABLE_SPIRAL_LIMIT: usize = 8;

impl ApplicationHandler for MainLoop {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let _ = event_loop;
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        let _ = event_loop;
        let _ = window_id;
        match event {
            _ => {}
        }

        if self.idle(Some(Duration::from_micros(250))) {
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let _ = event_loop;

        self.world.flush_actions();

        let mut iters = 0;
        while iters < STABLE_SPIRAL_LIMIT && Instant::now() > self.stable_accumulator {
            for level in self.world.iter_levels() {
                for id in level.iter_top_level() {
                    let mut comp = id
                        .get_mut(&self.world)
                        .expect("component was just acquired");
                    comp.pre_phys_hook(&self.world, self.world.get_stable_tick_rate().as_secs_f32());
                }
            }

            // TODO: Physics

            for level in self.world.iter_levels() {
                for id in level.iter_top_level() {
                    let mut comp = id
                        .get_mut(&self.world)
                        .expect("component was just acquired");
                    comp.post_phys_hook(&self.world, self.world.get_stable_tick_rate().as_secs_f32());
                }
            }

            self.stable_accumulator += self.world.get_stable_tick_rate();
            iters += 1;

            if self.idle(Some(Duration::from_micros(250))) {
                event_loop.exit();
                return;
            }
        }

        if self.idle(None) {
            event_loop.exit();
        }
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        let _ = event_loop;
    }
}

const MAIN_QUEUE_LEN: usize = 16;

/// Entry point for the engine.
/// 
/// `init_fn` is called when the main loop has initialized.
/// 
/// The main loop is what controls the whole engine, from the [`World`] to the
/// renderer to all stable-tick logic. The [`World`], is not shareable across
/// threads, so all operations with Components must be routed through the main loop.
/// 
/// See [`task`](crate::engine::task) for working with async code.
/// 
/// To exit the main loop, call [`queue_quit`].
/// 
/// # Safety
/// 
/// This function must be called exactly once on the main thread. The main loop provides a [`tokio`]
/// runtime, so do not use `#[tokio::main]`.
pub unsafe fn run_main_loop(init_fn: impl FnOnce(&World)) -> AResult<()> {
    let (job_tx, job_rx) = mpsc::channel(MAIN_QUEUE_LEN);
    let mut main_loop = MainLoop {
        tokio_rt: tokio::runtime::Runtime::new()?,
        jobs: job_rx,

        world: World::new(),

        stable_accumulator: Instant::now(),

        last_idle_time: Instant::now(),
    };

    RT_HANDLE
        .set(main_loop.tokio_rt.handle().clone())
        .expect("no other sources should set RT_HANDLE");

    MAIN_QUEUE
        .set(job_tx)
        .expect("no other sources should set MAIN_QUEUE");

    init_fn(&main_loop.world);

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
