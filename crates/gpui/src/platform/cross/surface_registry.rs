use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::Mutex;

/// An opaque identifier for a registered WGPU surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SurfaceId(pub(crate) u64);

/// Triple-buffered surface for lock-free rendering.
///
/// Uses three buffers with atomic index swaps:
/// - `rendering`: Currently being rendered by external thread
/// - `ready`: Latest complete frame, ready to display
/// - `display`: Currently being composited by GPUI
///
/// This allows external thread and compositor to run independently without blocking.
struct TripleBuffer {
    textures: [wgpu::Texture; 3],
    views: [wgpu::TextureView; 3],

    // Atomic buffer indices (0, 1, or 2)
    rendering_idx: AtomicU8,  // Back buffer for external rendering
    ready_idx: AtomicU8,      // Mailbox buffer - latest complete frame
    display_idx: AtomicU8,    // Front buffer for compositor

    // Redraw coalescing: prevents flooding compositor with thousands of requests/sec
    redraw_pending: std::sync::atomic::AtomicBool,

    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
}

/// Thread-safe registry of all active WGPU surfaces.
/// Maps `SurfaceId` to triple-buffered texture sets.
pub struct SurfaceRegistry {
    surfaces: Mutex<HashMap<SurfaceId, TripleBuffer>>,
    next_id: AtomicU64,
}

impl SurfaceRegistry {
    pub fn new() -> Self {
        Self {
            surfaces: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    /// Create a new triple-buffered surface. Returns its `SurfaceId`.
    pub fn create(
        &self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat
    ) -> SurfaceId {
        let id = SurfaceId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let tb = Self::create_triple_buffer(device, width, height, format);
        self.surfaces.lock().unwrap().insert(id, tb);
        id
    }

    /// Atomically swap rendering and ready buffers (called by external thread after rendering).
    ///
    /// This is the "present" operation - it makes the newly rendered frame available
    /// to the compositor and gives the external thread a recycled buffer to render into.
    ///
    /// Returns immediately without blocking.
    pub fn swap_rendering_ready(&self, id: SurfaceId) {
        if let Some(tb) = self.surfaces.lock().unwrap().get(&id) {
            // Atomically swap: rendering becomes ready, old ready becomes rendering
            let old_rendering = tb.rendering_idx.load(Ordering::Relaxed);
            let old_ready = tb.ready_idx.swap(old_rendering, Ordering::Relaxed);
            tb.rendering_idx.store(old_ready, Ordering::Relaxed);
        }
    }

    /// Atomically swap ready and display buffers (called by compositor before sampling).
    ///
    /// This updates the compositor's view to the latest complete frame and returns
    /// the old display buffer to the ready pool.
    ///
    /// Returns immediately without blocking.
    pub fn swap_ready_display(&self, id: SurfaceId) {
        if let Some(tb) = self.surfaces.lock().unwrap().get(&id) {
            // Atomically swap: ready becomes display, old display becomes ready
            let old_ready = tb.ready_idx.load(Ordering::Relaxed);
            let old_display = tb.display_idx.swap(old_ready, Ordering::Relaxed);
            tb.ready_idx.store(old_display, Ordering::Relaxed);
        }
    }

    /// Get the rendering buffer's `TextureView` (what external code renders into).
    pub fn back_view(&self, id: SurfaceId) -> Option<wgpu::TextureView> {
        let surfaces = self.surfaces.lock().unwrap();
        surfaces.get(&id).map(|tb| {
            let idx = tb.rendering_idx.load(Ordering::Relaxed) as usize;
            tb.views[idx].clone()
        })
    }

    /// Get the display buffer's `TextureView` (what the compositor reads from).
    pub fn front_view(&self, id: SurfaceId) -> Option<wgpu::TextureView> {
        let surfaces = self.surfaces.lock().unwrap();
        surfaces.get(&id).map(|tb| {
            let idx = tb.display_idx.load(Ordering::Relaxed) as usize;
            tb.views[idx].clone()
        })
    }

    /// Atomically retrieve both the rendering view and the corresponding texture
    /// dimensions. This is useful when a caller needs to create auxiliary
    /// resources (e.g. a depth buffer) that must exactly match the view's size.
    pub fn lock_and_get_back_with_size(
        &self,
        id: SurfaceId
    ) -> Option<(wgpu::TextureView, (u32, u32))> {
        let surfaces = self.surfaces.lock().unwrap();
        surfaces.get(&id).map(|tb| {
            let idx = tb.rendering_idx.load(Ordering::Relaxed) as usize;
            (tb.views[idx].clone(), (tb.width, tb.height))
        })
    }

    /// Resize all three buffers, creating new textures.
    ///
    /// SAFETY: Skips resize if a compositor pass is pending to avoid invalidating
    /// views currently in use. The element will retry resize on next frame.
    pub fn resize(&self, device: &wgpu::Device, id: SurfaceId, width: u32, height: u32) {
        let mut surfaces = self.surfaces.lock().unwrap();
        if let Some(tb) = surfaces.get_mut(&id) {
            if tb.width == width && tb.height == height {
                return;
            }

            // CRITICAL: Don't resize while compositor is rendering this surface!
            // If redraw_pending is true, compositor is using the buffers.
            // Skip resize - the element will retry on next frame.
            if tb.redraw_pending.load(Ordering::Relaxed) {
                return;
            }

            let new_tb = Self::create_triple_buffer(device, width, height, tb.format);
            *tb = new_tb;
        }
    }

    /// Get the current size of a surface.
    pub fn size(&self, id: SurfaceId) -> Option<(u32, u32)> {
        let surfaces = self.surfaces.lock().unwrap();
        surfaces.get(&id).map(|tb| (tb.width, tb.height))
    }

    /// Get the texture format for a surface.
    pub fn format(&self, id: SurfaceId) -> Option<wgpu::TextureFormat> {
        let surfaces = self.surfaces.lock().unwrap();
        surfaces.get(&id).map(|tb| tb.format)
    }

    /// Remove a surface from the registry.
    pub fn remove(&self, id: SurfaceId) {
        self.surfaces.lock().unwrap().remove(&id);
    }

    /// Set the redraw pending flag, returning the previous value.
    /// Used by present() to coalesce multiple redraw requests.
    pub fn set_redraw_pending(&self, id: SurfaceId) -> bool {
        if let Some(tb) = self.surfaces.lock().unwrap().get(&id) {
            tb.redraw_pending.swap(true, Ordering::Relaxed)
        } else {
            false
        }
    }

    /// Clear the redraw pending flag.
    /// Called by the compositor after consuming a frame.
    pub fn clear_redraw_pending(&self, id: SurfaceId) {
        if let Some(tb) = self.surfaces.lock().unwrap().get(&id) {
            tb.redraw_pending.store(false, Ordering::Relaxed);
        }
    }

    /// Get all surfaces that have pending redraws.
    /// Used by the fast blit path to check which surfaces need updating.
    pub fn get_pending_surfaces(&self) -> Vec<SurfaceId> {
        let surfaces = self.surfaces.lock().unwrap();
        surfaces
            .iter()
            .filter(|(_, tb)| tb.redraw_pending.load(Ordering::Relaxed))
            .map(|(id, _)| *id)
            .collect()
    }

    fn create_triple_buffer(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat
    ) -> TripleBuffer {
        let w = width.max(1);
        let h = height.max(1);

        let create_texture = |label: &str| {
            device.create_texture(
                &wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT |
                           wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                }
            )
        };

        let tex0 = create_texture("surface_buffer_0");
        let tex1 = create_texture("surface_buffer_1");
        let tex2 = create_texture("surface_buffer_2");

        let view0 = tex0.create_view(&wgpu::TextureViewDescriptor::default());
        let view1 = tex1.create_view(&wgpu::TextureViewDescriptor::default());
        let view2 = tex2.create_view(&wgpu::TextureViewDescriptor::default());

        TripleBuffer {
            textures: [tex0, tex1, tex2],
            views: [view0, view1, view2],
            rendering_idx: AtomicU8::new(0),  // External thread renders to buffer 0
            ready_idx: AtomicU8::new(1),      // Buffer 1 is the mailbox
            display_idx: AtomicU8::new(2),    // Compositor displays buffer 2
            redraw_pending: std::sync::atomic::AtomicBool::new(false),
            width: w,
            height: h,
            format,
        }
    }
}
