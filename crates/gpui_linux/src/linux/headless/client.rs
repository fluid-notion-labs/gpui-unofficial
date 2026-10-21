use std::cell::RefCell;
use std::rc::Rc;

use calloop::{EventLoop, LoopHandle};
use gpui_util::ResultExt;

use crate::linux::headless::window::{HeadlessDisplay, HeadlessWindow};
use crate::linux::{LinuxClient, LinuxCommon, LinuxKeyboardLayout};
#[cfg(any(feature = "wayland", feature = "x11"))]
use gpui::WindowAppearance;
use gpui::{
    AnyWindowHandle, CursorStyle, DisplayId, PlatformDisplay, PlatformKeyboardLayout,
    PlatformWindow, TextureWindowOptions, WindowParams,
};
#[cfg(any(feature = "wayland", feature = "x11"))]
use gpui_wgpu::GpuContext;

pub struct HeadlessClientState {
    pub(crate) _loop_handle: LoopHandle<'static, HeadlessClient>,
    pub(crate) event_loop: Option<calloop::EventLoop<'static, HeadlessClient>>,
    pub(crate) common: LinuxCommon,
    pub(crate) display: Rc<dyn PlatformDisplay>,
    /// GPU context for texture windows (offscreen wgpu rendering). Created
    /// lazily when the first texture window opens if the embedder did not
    /// hand one over.
    #[cfg(any(feature = "wayland", feature = "x11"))]
    pub(crate) gpu_context: Option<GpuContext>,
}

#[derive(Clone)]
pub(crate) struct HeadlessClient(Rc<RefCell<HeadlessClientState>>);

impl HeadlessClient {
    pub(crate) fn new() -> Self {
        let event_loop = EventLoop::try_new().unwrap();

        let (common, main_receiver, wake_receiver) = LinuxCommon::new(event_loop.get_signal());

        let handle = event_loop.handle();

        handle
            .insert_source(main_receiver, |event, _, _: &mut HeadlessClient| {
                if let calloop::channel::Event::Msg(runnable) = event {
                    runnable.run();
                }
            })
            .ok();

        handle
            .insert_source(wake_receiver, |event, _, client: &mut HeadlessClient| {
                if let calloop::channel::Event::Msg(()) = event {
                    client.with_common(|common| common.handle_system_wake());
                }
            })
            .ok();

        HeadlessClient(Rc::new(RefCell::new(HeadlessClientState {
            event_loop: Some(event_loop),
            _loop_handle: handle,
            common,
            display: Rc::new(HeadlessDisplay::new()),
            #[cfg(any(feature = "wayland", feature = "x11"))]
            gpu_context: None,
        })))
    }

    /// A headless client that renders texture windows on the embedder's GPU
    /// context instead of creating one lazily. No display server is touched:
    /// this client never opens a compositor connection or a real window.
    #[cfg(any(feature = "wayland", feature = "x11"))]
    pub(crate) fn with_gpu(gpu_context: GpuContext) -> Self {
        let client = Self::new();
        client.0.borrow_mut().gpu_context = Some(gpu_context);
        client
    }
}

impl LinuxClient for HeadlessClient {
    fn with_common<R>(&self, f: impl FnOnce(&mut LinuxCommon) -> R) -> R {
        f(&mut self.0.borrow_mut().common)
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(LinuxKeyboardLayout::new("unknown".into()))
    }

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        vec![self.0.borrow().display.clone()]
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(self.0.borrow().display.clone())
    }

    fn display(&self, id: DisplayId) -> Option<Rc<dyn PlatformDisplay>> {
        let display = self.0.borrow().display.clone();
        (display.id() == id).then_some(display)
    }

    #[cfg(feature = "screen-capture")]
    fn screen_capture_sources(
        &self,
    ) -> futures::channel::oneshot::Receiver<anyhow::Result<Vec<Rc<dyn gpui::ScreenCaptureSource>>>>
    {
        let (tx, rx) = futures::channel::oneshot::channel();
        tx.send(Err(anyhow::anyhow!(
            "Headless mode does not support screen capture."
        )))
        .ok();
        rx
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        None
    }

    fn window_stack(&self) -> Option<Vec<AnyWindowHandle>> {
        None
    }

    fn open_window(
        &self,
        _handle: AnyWindowHandle,
        params: WindowParams,
    ) -> anyhow::Result<Box<dyn PlatformWindow>> {
        Ok(Box::new(HeadlessWindow::new(
            params,
            self.0.borrow().display.clone(),
        )))
    }

    fn compositor_name(&self) -> &'static str {
        "headless"
    }

    /// Texture windows are the ONLY window type a headless client can open —
    /// there is no compositor to attach a real window to. They render into an
    /// offscreen wgpu texture via [`crate::linux::TextureWindow`], sharing the
    /// embedder's GPU context when one was provided (see [`Self::with_gpu`]).
    #[cfg(any(feature = "wayland", feature = "x11"))]
    fn open_texture_window(
        &self,
        _handle: AnyWindowHandle,
        options: TextureWindowOptions,
    ) -> anyhow::Result<Box<dyn PlatformWindow>> {
        let gpu_context = self
            .0
            .borrow_mut()
            .gpu_context
            .get_or_insert_with(|| GpuContext::new(Default::default()))
            .clone();
        let window =
            crate::linux::TextureWindow::new(gpu_context, None, options, WindowAppearance::Dark)?;
        Ok(Box::new(window))
    }

    fn set_cursor_style(&self, _style: CursorStyle) {}

    fn open_uri(&self, _uri: &str) {}

    fn reveal_path(&self, _path: std::path::PathBuf) {}

    fn write_to_primary(&self, _item: gpui::ClipboardItem) {}

    fn write_to_clipboard(&self, _item: gpui::ClipboardItem) {}

    fn read_from_primary(&self) -> Option<gpui::ClipboardItem> {
        None
    }

    fn read_from_clipboard(&self) -> Option<gpui::ClipboardItem> {
        None
    }

    fn run(&self) {
        let mut event_loop = self
            .0
            .borrow_mut()
            .event_loop
            .take()
            .expect("App is already running");

        event_loop.run(None, &mut self.clone(), |_| {}).log_err();
    }
}
