//! The raster tile layer on a bundle's scan pyramid, in a native window or
//! headless.
//!
//!     cargo run -q --release -p lmsheets-gpu --example scan -- BUNDLE/LDW280-17619/sh3
//!     cargo run -q --release -p lmsheets-gpu --example scan -- BUNDLE/LDW280-17619/sh3 \
//!         --screenshot out.png [--zoom Z] [--centre X,Y] [--size WxH] [--frames N]
//!
//! The window: drag to pan, scroll or pinch to zoom, `0` to fit, Esc to
//! quit. Zoom is screen pixels per crop pixel (crop = the bundle's scan
//! tiles; frame pixel = crop pixel + manifest `crop.offset`); `--centre`
//! is a crop pixel. Without `--zoom` the whole sheet is fitted. The
//! headless mode prints what it loaded and the mean frame time over
//! `--frames` renders (default 100), tile uploads excluded.

use lmsheets_gpu::raster::{load_tile, Pyramid, RasterLayer, ScanView};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

/// The pyramid, the layer, and what has been read from disk so far.
struct Scan {
    dir: PathBuf,
    pyramid: Pyramid,
    layer: RasterLayer,
    tiles_read: usize,
    file_bytes: usize,
}

impl Scan {
    fn open(dir: &Path, device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let json = std::fs::read_to_string(dir.join("manifest.json"))
            .unwrap_or_else(|e| panic!("{}: {e}", dir.join("manifest.json").display()));
        Self {
            dir: dir.to_path_buf(),
            pyramid: Pyramid::from_manifest(&json).expect("a manifest with a pyramid"),
            layer: RasterLayer::new(device, format),
            tiles_read: 0,
            file_bytes: 0,
        }
    }

    /// Read and upload the tiles `view` needs and does not yet have.
    fn ensure(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, view: &ScanView) {
        for id in self.layer.missing(&self.pyramid, view) {
            let t = load_tile(&self.dir, &self.pyramid, id).expect("a readable tile");
            self.layer.insert(
                device,
                queue,
                &self.pyramid,
                id,
                (t.width, t.height),
                &t.grey,
            );
            self.tiles_read += 1;
            self.file_bytes += t.file_bytes;
        }
    }

    fn describe(&self, view: &ScanView) -> String {
        let (held, tex) = self.layer.held();
        format!(
            "level {} at zoom {:.4}: {} tiles read, {} file bytes, {} held ({} texture bytes)",
            self.pyramid.pick_level(view.zoom),
            view.zoom,
            self.tiles_read,
            self.file_bytes,
            held,
            tex
        )
    }
}

struct Args {
    dir: PathBuf,
    screenshot: Option<PathBuf>,
    zoom: Option<f64>,
    centre: Option<(f64, f64)>,
    size: (u32, u32),
    frames: u32,
}

fn parse_args() -> Args {
    let mut a = Args {
        dir: PathBuf::new(),
        screenshot: None,
        zoom: None,
        centre: None,
        size: (1600, 1000),
        frames: 100,
    };
    let mut it = std::env::args().skip(1);
    let pair = |s: &str, sep: char| -> (f64, f64) {
        let (x, y) = s.split_once(sep).expect("two numbers");
        (x.parse().expect("a number"), y.parse().expect("a number"))
    };
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--screenshot" => a.screenshot = it.next().map(PathBuf::from),
            "--zoom" => a.zoom = it.next().and_then(|s| s.parse().ok()),
            "--centre" => a.centre = it.next().map(|s| pair(&s, ',')),
            "--size" => {
                let (w, h) = pair(&it.next().expect("WxH"), 'x');
                a.size = (w as u32, h as u32);
            }
            "--frames" => a.frames = it.next().and_then(|s| s.parse().ok()).expect("a count"),
            _ => a.dir = PathBuf::from(arg),
        }
    }
    assert!(
        a.dir.join("manifest.json").exists(),
        "give a sheet directory with a manifest.json"
    );
    a
}

fn start_view(scan: &Scan, args: &Args, size: (u32, u32)) -> ScanView {
    let fit = ScanView::fitting(scan.pyramid.extent(), size);
    let extent = scan.pyramid.extent();
    let centre = args
        .centre
        .unwrap_or((extent.0 as f64 / 2.0, extent.1 as f64 / 2.0));
    ScanView::centred(centre, args.zoom.unwrap_or(fit.zoom), size)
}

fn headless(args: Args, t0: Instant) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        pollster::block_on(instance.request_adapter(&Default::default())).expect("an adapter");
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("a device");
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut scan = Scan::open(&args.dir, &device, format);
    let view = start_view(&scan, &args, args.size);
    let extent = wgpu::Extent3d {
        width: args.size.0,
        height: args.size.1,
        depth_or_array_layers: 1,
    };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("screenshot"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&Default::default());
    let row = (args.size.0 * 4).next_multiple_of(256);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: row as u64 * args.size.1 as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    scan.ensure(&device, &queue, &view);
    let render = |scan: &mut Scan, copy: bool| {
        let mut enc = device.create_command_encoder(&Default::default());
        let n = scan.layer.render(
            &device,
            &queue,
            &mut enc,
            &target_view,
            &scan.pyramid,
            &view,
        );
        if copy {
            enc.copy_texture_to_buffer(
                target.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row),
                        rows_per_image: Some(args.size.1),
                    },
                },
                extent,
            );
        }
        queue.submit([enc.finish()]);
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the GPU");
        n
    };
    let drawn = render(&mut scan, true);
    let first = t0.elapsed();

    let t = Instant::now();
    for _ in 0..args.frames {
        render(&mut scan, false);
    }
    let frame_ms = t.elapsed().as_secs_f64() * 1000.0 / args.frames.max(1) as f64;

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("a mappable buffer"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the GPU");
    let data = slice.get_mapped_range().expect("a mapped range");
    let mut rgba = Vec::with_capacity((args.size.0 * args.size.1 * 4) as usize);
    for y in 0..args.size.1 as usize {
        rgba.extend_from_slice(&data[y * row as usize..][..args.size.0 as usize * 4]);
    }
    drop(data);
    let out = args.screenshot.as_ref().expect("--screenshot OUT.png");
    image::save_buffer(
        out,
        &rgba,
        args.size.0,
        args.size.1,
        image::ColorType::Rgba8,
    )
    .expect("a written PNG");
    println!("{}", scan.describe(&view));
    println!(
        "drew {drawn} tiles; first frame {:.0} ms from start (device, manifest, tiles, render, wait); {:.3} ms/frame over {} frames; wrote {}",
        first.as_secs_f64() * 1000.0,
        frame_ms,
        args.frames,
        out.display()
    );
}

struct Gpu {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    scan: Scan,
    view: ScanView,
}

struct App {
    args: Args,
    t0: Instant,
    first: bool,
    gpu: Option<Gpu>,
    cursor: (f64, f64),
    dragging: bool,
}

impl App {
    fn draw(&mut self) {
        let Some(g) = self.gpu.as_mut() else { return };
        g.scan.ensure(&g.device, &g.queue, &g.view);
        let frame = match g.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => {
                g.surface.configure(&g.device, &g.config);
                return;
            }
        };
        let target = frame.texture.create_view(&Default::default());
        let mut enc = g.device.create_command_encoder(&Default::default());
        g.scan.layer.render(
            &g.device,
            &g.queue,
            &mut enc,
            &target,
            &g.scan.pyramid,
            &g.view,
        );
        g.queue.submit([enc.finish()]);
        g.queue.present(frame);
        if self.first {
            self.first = false;
            println!(
                "first frame {:.0} ms from start; {}",
                self.t0.elapsed().as_secs_f64() * 1000.0,
                g.scan.describe(&g.view)
            );
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes().with_title("lmsheets scan"))
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
        let scan = Scan::open(&self.args.dir, &device, format);
        let view = start_view(&scan, &self.args, (config.width, config.height));
        self.gpu = Some(Gpu {
            window,
            device,
            queue,
            surface,
            config,
            scan,
            view,
        });
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
                if let (true, Some(g)) = (self.dragging, self.gpu.as_mut()) {
                    g.view = g.view.panned((to.0 - self.cursor.0, to.1 - self.cursor.1));
                    g.window.request_redraw();
                }
                self.cursor = to;
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => self.dragging = state == ElementState::Pressed,
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
            WindowEvent::PinchGesture { delta, .. } => {
                if let Some(g) = self.gpu.as_mut() {
                    g.view = g.view.zoomed((delta * 1.5).exp(), self.cursor);
                    g.window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match event.logical_key {
                    Key::Character(ref c) if c == "0" => {
                        if let Some(g) = self.gpu.as_mut() {
                            g.view = ScanView::fitting(g.scan.pyramid.extent(), g.view.size);
                            g.window.request_redraw();
                        }
                    }
                    Key::Named(NamedKey::Escape) => event_loop.exit(),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let t0 = Instant::now();
    let args = parse_args();
    if args.screenshot.is_some() {
        return headless(args, t0);
    }
    let event_loop = EventLoop::new().expect("an event loop");
    let mut app = App {
        args,
        t0,
        first: true,
        gpu: None,
        cursor: (0.0, 0.0),
        dragging: false,
    };
    event_loop.run_app(&mut app).expect("the event loop");
}
