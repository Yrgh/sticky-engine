//! Vulkan utilities

use std::sync::{Arc, Mutex};

use anyhow::{Context, Result as AResult, bail};

use vulkano::{
    VulkanLibrary,
    command_buffer::allocator::{CommandBufferAllocator, StandardCommandBufferAllocator},
    device::{
        Device, DeviceCreateInfo, DeviceExtensions, Queue, QueueCreateInfo, QueueFlags,
        physical::{PhysicalDevice, PhysicalDeviceType},
    },
    image::{Image, ImageUsage},
    instance::{Instance, InstanceCreateFlags, InstanceCreateInfo},
    memory::allocator::{MemoryAllocator, StandardMemoryAllocator},
    swapchain::{Surface, Swapchain, SwapchainCreateInfo},
    sync::GpuFuture,
};
use winit::{dpi::PhysicalSize, raw_window_handle::HasDisplayHandle};

use crate::core::renderer::FinalPresentFuture;

/// Initialization options for the Vulkan context.
pub struct InitializationOptions<'a> {
    /// A reference to the event loop, if presenting to surfaces is desired. If
    /// not, this can be left as `None`, even if an event loop exists.
    pub event_loop: Option<&'a dyn HasDisplayHandle>,
}

/// The Vulkan context the engine relies on.
pub struct VkContext {
    /// The Vulkan instance
    pub instance: Arc<Instance>,
    /// The selected physical device
    pub physical_device: Arc<PhysicalDevice>,
    /// The logical device
    pub device: Arc<Device>,
    /// The selected queues
    pub queues: Vec<Arc<Queue>>,
    /// The memory allocator for *buffers*
    pub buffer_allocator: Arc<dyn MemoryAllocator>,
    /// The memory allocator for *command buffers*
    pub command_buffer_allocator: Arc<dyn CommandBufferAllocator>,
    /// Every present fence that is still in flight.
    ///
    /// Present fences must never be dropped directly while un-cleaned (dropping would block the
    /// thread until the GPU finishes). Instead, windows push their final present fence here and the
    /// main loop periodically calls [`cleanup_in_flight_futures`](Self::cleanup_in_flight_futures),
    /// which releases any fence whose GPU work has finished and removes it from the list.
    in_flight_futures: Mutex<Vec<Arc<FinalPresentFuture>>>,
}

impl VkContext {
    pub(crate) fn new(init_opts: InitializationOptions<'_>) -> AResult<Self> {
        let library = VulkanLibrary::new().context("Failed to create vulkan library")?;

        let surface_extensions = init_opts
            .event_loop
            .as_ref()
            .map(Surface::required_extensions)
            .transpose()
            .context("querying surface extensions")?
            .unwrap_or_default();

        let instance = Instance::new(
            library,
            InstanceCreateInfo {
                flags: InstanceCreateFlags::ENUMERATE_PORTABILITY,
                enabled_extensions: surface_extensions,
                ..Default::default()
            },
        )
        .context("Failed to create vulkan instance")?;

        // TODO: User-defined extensions and queue flags
        let device_extensions = DeviceExtensions {
            khr_swapchain: true,
            ..Default::default()
        };

        let Some((physical_device, queue_family, _queue_count)) = instance
            .enumerate_physical_devices()?
            // Needs to have relevant queues and extensions
            .filter_map(|p| {
                if !p.supported_extensions().contains(&device_extensions) {
                    return None;
                }
                let (qf, qn) =
                    p.queue_family_properties()
                        .iter()
                        .enumerate()
                        .find_map(|(id, qf)| {
                            (qf.queue_flags
                                .contains(QueueFlags::GRAPHICS | QueueFlags::COMPUTE)
                                && init_opts.event_loop.as_ref().is_some_and(|el| {
                                    p.presentation_support(id as u32, el)
                                        .expect("failed to query presentation support")
                                }))
                            .then_some((id as u32, qf.queue_count))
                        })?;

                Some((p, qf, qn))
            })
            // TODO: Collect all devices and allow device switching
            // Pick the best device that fits
            .min_by_key(|(p, _qf, _qn)| match p.properties().device_type {
                PhysicalDeviceType::DiscreteGpu => 0,
                PhysicalDeviceType::IntegratedGpu => 1,
                PhysicalDeviceType::VirtualGpu => 2,
                PhysicalDeviceType::Cpu => 3,
                _ => 4,
            })
        else {
            bail!("no valid physical devices");
        };

        let (device, queues) = Device::new(
            physical_device.clone(),
            DeviceCreateInfo {
                queue_create_infos: vec![QueueCreateInfo {
                    queue_family_index: queue_family,
                    ..Default::default()
                }],
                enabled_extensions: device_extensions,
                ..Default::default()
            },
        )
        .context("Failed to create logical device")?;

        let queues: Vec<_> = queues.collect();

        let buffer_allocator = Arc::new(StandardMemoryAllocator::new_default(device.clone()));

        let command_buffer_allocator = Arc::new(StandardCommandBufferAllocator::new(
            device.clone(),
            Default::default(),
        ));

        Ok(Self {
            instance,
            physical_device,
            device,
            queues,
            buffer_allocator,
            command_buffer_allocator,
            in_flight_futures: Mutex::new(Vec::new()),
        })
    }

    /// Registers a present fence for lifetime tracking.
    ///
    /// The future is kept alive here (instead of being dropped, which could
    /// block) until the GPU finishes with it and
    /// [`cleanup_in_flight_futures`](Self::cleanup_in_flight_futures) removes
    /// it.
    pub(crate) fn push_in_flight_future(&self, fut: Arc<FinalPresentFuture>) {
        self.in_flight_futures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(fut);
    }

    /// Releases every in-flight present fence whose GPU work has finished, and
    /// removes it from the list.
    ///
    /// This is non-blocking: each fence is first given a chance to clean up via
    /// [`GpuFuture::cleanup_finished`], then kept only if its fence is not yet
    /// signalled.
    pub(crate) fn cleanup_in_flight_futures(&self) {
        let mut list = self
            .in_flight_futures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        list.retain_mut(|fut| {
            fut.cleanup_finished();
            !fut.is_signaled().unwrap_or(true)
        });
    }

    /// Only one swapchain can exist per surface. Use
    /// [`recreate_swapchain`](Self::recreate_swapchain) for resizes. Use this
    /// for resumes.
    pub(crate) fn create_swapchain(
        &self,
        surface: Arc<Surface>,
        size: PhysicalSize<u32>,
    ) -> AResult<(Arc<Swapchain>, Vec<Arc<Image>>)> {
        let caps = self
            .physical_device
            .surface_capabilities(&surface, Default::default())?;

        let composite_alpha = caps
            .supported_composite_alpha
            .into_iter()
            .next()
            .expect("no composite alphas");
        let image_format = self
            .physical_device
            .surface_formats(&surface, Default::default())?[0]
            .0;

        Ok(Swapchain::new(
            self.device.clone(),
            surface.clone(),
            SwapchainCreateInfo {
                min_image_count: caps.min_image_count + 1,
                image_format,
                image_extent: size.into(),
                image_usage: ImageUsage::COLOR_ATTACHMENT,
                composite_alpha,
                ..Default::default()
            },
        )?)
    }

    pub(crate) fn recreate_swapchain(
        &self,
        original: Arc<Swapchain>,
        size: PhysicalSize<u32>,
    ) -> AResult<(Arc<Swapchain>, Vec<Arc<Image>>)> {
        let caps = self
            .physical_device
            .surface_capabilities(original.surface(), Default::default())?;

        let composite_alpha = caps
            .supported_composite_alpha
            .into_iter()
            .next()
            .expect("no composite alphas");
        let image_format = self
            .physical_device
            .surface_formats(original.surface(), Default::default())?[0]
            .0;

        Ok(Swapchain::recreate(
            &original,
            SwapchainCreateInfo {
                min_image_count: caps.min_image_count + 1,
                image_format,
                image_extent: size.into(),
                image_usage: ImageUsage::COLOR_ATTACHMENT,
                composite_alpha,
                ..Default::default()
            },
        )?)
    }
}
