mod dispatcher;
mod headless;
mod keyboard;
mod platform;
mod system_notifications;
#[cfg(any(feature = "wayland", feature = "x11"))]
mod text_system;
#[cfg(any(feature = "wayland", feature = "x11"))]
mod texture_window;
#[cfg(feature = "wayland")]
mod wayland;
#[cfg(feature = "x11")]
mod x11;

#[cfg(any(feature = "wayland", feature = "x11"))]
mod xdg_desktop_portal;

pub use dispatcher::*;
pub(crate) use headless::*;
pub(crate) use keyboard::*;
pub(crate) use platform::*;
#[cfg(any(feature = "wayland", feature = "x11"))]
pub(crate) use text_system::*;
#[cfg(any(feature = "wayland", feature = "x11"))]
pub(crate) use texture_window::*;
#[cfg(feature = "wayland")]
pub(crate) use wayland::*;
#[cfg(feature = "x11")]
pub(crate) use x11::*;

#[cfg(any(feature = "wayland", feature = "x11"))]
use std::rc::Rc;

/// Returns a renderer that draws into an offscreen texture, sharing one
/// headless GPU context per thread so test windows and texture windows on
/// the same thread share a device.
#[cfg(any(feature = "wayland", feature = "x11"))]
pub fn current_headless_renderer() -> Option<Box<dyn gpui::PlatformHeadlessRenderer>> {
    use anyhow::Context as _;
    use gpui_util::ResultExt as _;

    // Never dropped: by the time this thread-local's destructor runs, wgpu's
    // own thread-locals may already be gone, and dropping a Queue then aborts.
    thread_local! {
        static HEADLESS_GPU_CONTEXT: std::mem::ManuallyDrop<gpui_wgpu::GpuContext> =
            std::mem::ManuallyDrop::new(gpui_wgpu::GpuContext::new(Default::default()));
    }

    let gpu_context = HEADLESS_GPU_CONTEXT.with(|context| gpui_wgpu::GpuContext::clone(context));
    let renderer = gpui_wgpu::WgpuHeadlessRenderer::new(gpu_context)
        .context("Failed to create headless wgpu renderer")
        .log_err()?;
    Some(Box::new(renderer))
}

#[cfg(not(any(feature = "wayland", feature = "x11")))]
pub fn current_headless_renderer() -> Option<Box<dyn gpui::PlatformHeadlessRenderer>> {
    None
}

/// Returns the default platform implementation for the current OS.
pub fn current_platform(headless: bool) -> Rc<dyn gpui::Platform> {
    #[cfg(feature = "x11")]
    use anyhow::Context as _;

    if headless {
        return Rc::new(LinuxPlatform {
            inner: HeadlessClient::new(),
        });
    }

    #[cfg(any(feature = "wayland", feature = "x11"))]
    let gpu_context = gpui_wgpu::GpuContext::new(Default::default());

    match gpui::guess_compositor() {
        #[cfg(feature = "wayland")]
        "Wayland" => Rc::new(LinuxPlatform {
            inner: WaylandClient::new(gpu_context),
        }),

        #[cfg(feature = "x11")]
        "X11" => Rc::new(LinuxPlatform {
            inner: X11Client::new(gpu_context)
                .context("Failed to initialize X11 client.")
                .unwrap(),
        }),

        "Headless" => Rc::new(LinuxPlatform {
            inner: HeadlessClient::new(),
        }),
        _ => unreachable!(),
    }
}

/// Returns a platform that shares the given wgpu resources with its renderer.
///
/// Takes a [`gpui_wgpu::GpuContext`] handle: it may be empty (the first
/// window's renderer then fills it, with an instance bound to the platform's
/// own display connection — required, since surfaces must be created from
/// the same connection their window belongs to) or prefilled by the caller.
/// Either way the embedder holds the same handle and can create textures on
/// the shared device.
#[cfg(any(feature = "wayland", feature = "x11"))]
pub fn current_platform_with_gpu(gpu_context: gpui_wgpu::GpuContext) -> Rc<dyn gpui::Platform> {
    #[cfg(feature = "x11")]
    use anyhow::Context as _;

    match gpui::guess_compositor() {
        #[cfg(feature = "wayland")]
        "Wayland" => Rc::new(LinuxPlatform {
            inner: WaylandClient::new(gpu_context),
        }),

        #[cfg(feature = "x11")]
        "X11" => Rc::new(LinuxPlatform {
            inner: X11Client::new(gpu_context)
                .context("Failed to initialize X11 client.")
                .unwrap(),
        }),

        "Headless" => Rc::new(LinuxPlatform {
            inner: HeadlessClient::new(),
        }),
        _ => unreachable!(
            r#"At least one of the "wayland" or "x11" features must be enabled on gpui_linux or gpui_platform."#
        ),
    }
}
