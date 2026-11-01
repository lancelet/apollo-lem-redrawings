//! The renderer in a native window: the same scene, the same shader, no
//! browser. Drag to pan, scroll to zoom, `0` to fit, `t` to switch the
//! theme, `1`–`3` to switch the document.
//!
//!     cargo run -q -p lmsheets-gpu --example native

use lmsheets_document::examples;
use lmsheets_document::state::document::Document;
use lmsheets_document::value::bounds::Bounded;
use lmsheets_document::value::style::Purpose;
use lmsheets_gpu::{Renderer, View};
use lmsheets_render::conventions::Conventions;
use lmsheets_render::draw::draw;
use lmsheets_render::resolve::resolve;
use lmsheets_render::scene::Scene;
use lmsheets_render::theme::Theme;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

const DOCUMENTS: [fn() -> Document; 3] = [examples::dimensions, examples::bracket, examples::block];

struct Gpu {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    view: View,
    bounds: ((f64, f64), (f64, f64)),
}

#[derive(Default)]
struct App {
    gpu: Option<Gpu>,
    document: usize,
    dark: bool,
    cursor: (f64, f64),
    dragging: bool,
}

impl App {
    fn scene(&mut self) {
        let Some(g) = self.gpu.as_mut() else { return };
        let doc = DOCUMENTS[self.document]();
        let scene = Scene::of(draw(&doc, Purpose::Screen));
        let theme = if self.dark {
            Theme::dark()
        } else {
            Theme::screen()
        };
        let styled = resolve(&scene, &doc, &theme, Conventions::default());
        g.renderer.set_scene(&g.device, &g.queue, &styled);
        let b = scene.bounds().expect("a drawing");
        g.bounds = ((b.min().x, b.min().y), (b.max().x, b.max().y));
    }

    fn fit(&mut self) {
        let Some(g) = self.gpu.as_mut() else { return };
        g.view = View::fitting(
            g.bounds.0,
            g.bounds.1,
            40.0,
            (g.config.width, g.config.height),
            1.0,
        );
    }

    fn draw(&mut self) {
        let Some(g) = self.gpu.as_mut() else { return };
        let frame = match g.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => {
                g.surface.configure(&g.device, &g.config);
                return;
            }
        };
        let target = frame.texture.create_view(&Default::default());
        let mut encoder = g.device.create_command_encoder(&Default::default());
        g.renderer.render(&g.queue, &mut encoder, &target, &g.view);
        g.queue.submit([encoder.finish()]);
        g.queue.present(frame);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes().with_title("lmsheets (native)"))
                .expect("a window"),
        );
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone()).expect("a surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("an adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("a device");
        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: Vec::new(),
            color_space: Default::default(),
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        let renderer = Renderer::new(&device, format);
        self.gpu = Some(Gpu {
            window,
            device,
            queue,
            surface,
            config,
            renderer,
            view: View {
                origin: (0.0, 0.0),
                zoom: 1.0,
                size: (size.width, size.height),
                min_width: 1.0,
            },
            bounds: ((0.0, 0.0), (1.0, 1.0)),
        });
        self.scene();
        self.fit();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(g) = self.gpu.as_mut() {
                    g.config.width = size.width.max(1);
                    g.config.height = size.height.max(1);
                    g.surface.configure(&g.device, &g.config);
                    g.view.size = (g.config.width, g.config.height);
                    g.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.draw(),
            WindowEvent::CursorMoved { position, .. } => {
                let to = (position.x, position.y);
                if self.dragging {
                    if let Some(g) = self.gpu.as_mut() {
                        g.view = g.view.panned((to.0 - self.cursor.0, to.1 - self.cursor.1));
                        g.window.request_redraw();
                    }
                }
                self.cursor = to;
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.dragging = state == ElementState::Pressed;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64 * 20.0,
                    MouseScrollDelta::PixelDelta(p) => p.y,
                };
                if let Some(g) = self.gpu.as_mut() {
                    g.view = g.view.zoomed((dy * 0.01).exp(), self.cursor);
                    g.window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match event.logical_key {
                    Key::Character(ref c) if c == "0" => self.fit(),
                    Key::Character(ref c) if c == "t" => {
                        self.dark = !self.dark;
                        self.scene();
                    }
                    Key::Character(ref c) if matches!(c.as_str(), "1" | "2" | "3") => {
                        self.document = c.parse::<usize>().unwrap_or(1) - 1;
                        self.scene();
                        self.fit();
                    }
                    Key::Named(NamedKey::Escape) => event_loop.exit(),
                    _ => return,
                }
                if let Some(g) = self.gpu.as_ref() {
                    g.window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("an event loop");
    let mut app = App {
        dark: true,
        ..Default::default()
    };
    event_loop.run_app(&mut app).expect("the event loop");
}
