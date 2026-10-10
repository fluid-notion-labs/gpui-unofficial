use std::cell::Cell;
use std::time::{Duration, Instant};

/// Detects whether a window is actively being resized by watching the stream
/// of [`winit::event::WindowEvent::Resized`] events.
///
/// While the user holds the resize handle, winit fires `Resized` continuously
/// (every frame on most platforms). Each call to [`on_resize_event`] extends a
/// short deadline. Once no `Resized` events arrive for [`IDLE_THRESHOLD`], the
/// detector considers the resize finished and [`is_resizing`] returns `false`.
///
/// This is purely event-driven and does **not** use mouse button state, so it
/// works correctly on macOS where the OS owns the window-drag and never delivers
/// a mouse-up event back to the application.
pub struct ResizeDetector {
    /// Absolute time after which the window is no longer considered resizing.
    /// `None` means no resize has been seen yet.
    deadline: Cell<Option<Instant>>,
}

/// How long after the last `Resized` event before we consider the resize done.
/// Kept short so the buffer unlocks quickly after the user releases the handle.
const IDLE_THRESHOLD: Duration = Duration::from_millis(150);

impl ResizeDetector {
    pub fn new() -> Self {
        Self {
            deadline: Cell::new(None),
        }
    }

    /// Must be called on every `WindowEvent::Resized` from the winit event loop.
    /// Extends the active-resize deadline by [`IDLE_THRESHOLD`] from now.
    pub fn on_resize_event(&self) {
        self.deadline.set(Some(Instant::now() + IDLE_THRESHOLD));
    }

    /// Returns `true` while resize events are still arriving or within the idle
    /// threshold after the last one. Returns `false` once the threshold expires.
    pub fn is_resizing(&self) -> bool {
        match self.deadline.get() {
            None => false,
            Some(deadline) => {
                if Instant::now() < deadline {
                    true
                } else {
                    // Deadline passed — clear so future checks short-circuit.
                    self.deadline.set(None);
                    false
                }
            }
        }
    }
}

impl Default for ResizeDetector {
    fn default() -> Self {
        Self::new()
    }
}
