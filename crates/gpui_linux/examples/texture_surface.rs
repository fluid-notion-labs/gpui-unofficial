//! Element-level texture seam: paint an external wgpu texture through the
//! `surface()` element, in ordinary layout, composited by gpui's own frame.
//!
//! One wgpu device is shared between the demo texture and gpui's renderer —
//! the same setup an embedded bevy viewport would use. Ordering matters:
//!
//! 1. create an empty `GpuContext` handle and hand it to the platform;
//! 2. open a window — the renderer fills the handle, with an instance bound
//!    to the *platform's* own display connection (surfaces must be created
//!    from the same connection their window belongs to; a second Wayland
//!    connection here would trip libwayland's cross-connection assert);
//! 3. read the device/queue back out of the handle and produce the texture.
//!
//! The texture here is CPU-filled (gradient) via `queue.write_texture` — in
//! a real integration it would be the output of another renderer (bevy,
//! video decoder) living on the same device.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, Application, Bounds, Context, DevicePixels, Render, Size, SurfaceSource, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, rgb, size, surface,
};
use gpui_wgpu::{GpuContext, wgpu};

const TEXTURE_SIZE: u32 = 512;

fn make_gradient_data(width: u32, height: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let fx = x as f32 / (width - 1) as f32;
            let fy = y as f32 / (height - 1) as f32;
            // diagonal blue→orange gradient with a white corner marker block
            let (r, g, b) = if x < 32 && y < 32 {
                (255u8, 255u8, 255u8)
            } else {
                (
                    (40.0 + 200.0 * fx) as u8,
                    (60.0 + 120.0 * fy) as u8,
                    (200.0 - 160.0 * fx) as u8,
                )
            };
            data.extend_from_slice(&[r, g, b, 255]);
        }
    }
    data
}

/// Creates the demo texture on the shared device, once the renderer has
/// filled the context. Returns the texture view to paint via `surface()`.
fn create_demo_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("texture_surface-demo"),
        size: wgpu::Extent3d {
            width: TEXTURE_SIZE,
            height: TEXTURE_SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &make_gradient_data(TEXTURE_SIZE, TEXTURE_SIZE),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(TEXTURE_SIZE * 4),
            rows_per_image: Some(TEXTURE_SIZE),
        },
        wgpu::Extent3d {
            width: TEXTURE_SIZE,
            height: TEXTURE_SIZE,
            depth_or_array_layers: 1,
        },
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

fn main() {
    let _ = env_logger::try_init();

    // Empty handle: the first window's renderer fills it.
    let gpu_context: GpuContext = Rc::new(RefCell::new(None));
    let platform = gpui_linux_gpui_unofficial::current_platform_with_gpu(gpu_context.clone());

    Application::with_platform(platform).run(move |cx: &mut App| {
        let bounds = Bounds {
            origin: Default::default(),
            size: size(px(720.0), px(520.0)),
        };
        let handle = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|_| DemoWindow {
                        texture_view: None,
                    })
                },
            )
            .unwrap();

        // The renderer created the shared context during open_window; now
        // produce the demo texture on the same device and hand it over.
        let (device, queue) = {
            let ctx = gpu_context.borrow();
            let ctx = ctx.as_ref().expect("renderer must fill the shared context");
            (ctx.device.clone(), ctx.queue.clone())
        };
        let texture_view = create_demo_texture(&device, &queue);

        handle
            .update(cx, |window: &mut DemoWindow, _window, _cx| {
                window.texture_view = Some(texture_view);
            })
            .unwrap();

        cx.activate(true);
    });
}

struct DemoWindow {
    texture_view: Option<wgpu::TextureView>,
}

fn surface_source(view: &wgpu::TextureView) -> SurfaceSource {
    SurfaceSource::Texture {
        texture_view: view.clone(),
        texture_format: wgpu::TextureFormat::Rgba8UnormSrgb,
        texture_size: Size {
            width: DevicePixels::from(TEXTURE_SIZE as i32),
            height: DevicePixels::from(TEXTURE_SIZE as i32),
        },
    }
}

impl Render for DemoWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        eprintln!("RENDER has_texture={} thread={:?}", self.texture_view.is_some(), std::thread::current().name());
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_4()
            .p_8()
            .bg(rgb(0x101018))
            .child(
                div()
                    .text_lg()
                    .text_color(rgb(0xf0f0f0))
                    .child("element-level texture seam — SurfaceSource::Texture"),
            )
            .child(
                div().text_sm().text_color(rgb(0x909098)).child(
                    "the card below is a plain wgpu texture painted by the surface_rgba pipeline",
                ),
            )
            .child(
                div().flex_1().min_h(px(200.)).child(match &self.texture_view {
                    Some(view) => surface(surface_source(view)).size_full().into_any_element(),
                    None => div()
                        .flex_1()
                        .bg(rgb(0x282838))
                        .child("creating texture…")
                        .into_any_element(),
                }),
            )
            .child(
                div().flex().gap_4().child(match &self.texture_view {
                    Some(view) => div()
                        .w_1_3()
                        .h_32()
                        .child(surface(surface_source(view)).size_full())
                        .into_any_element(),
                    None => div()
                        .w_1_3()
                        .h_32()
                        .bg(rgb(0x282838))
                        .into_any_element(),
                }),
            )
    }
}
