//! lmsheets-gpu: a styled scene drawn with wgpu, on the web and natively.
//!
//! The renderer takes a [`Styled`] scene (the theme already applied, the
//! dashes already fitted) and draws it under a [`View`]: the paper point
//! at the screen's top-left corner and the zoom in pixels per
//! millimetre. Every primitive is drawn analytically (see
//! [`instances`]), so there is no tessellation to redo on zoom; only
//! the view uniform changes per frame, and the instance buffer when the
//! scene does. Lines thinner than a pixel keep their true coverage as
//! alpha rather than dropping out.

#![warn(missing_docs)]

pub mod instances;
pub mod overlay;
pub mod raster;

use instances::Instance;
use lmsheets_document::value::style::Rgba;
use lmsheets_render::resolve::Styled;

/// Instances of styled items to light: a colour (linear RGB), an opacity
/// and a minimum width in device px, so a thin line stays visible at any
/// zoom.
#[derive(Clone, Copy, Debug)]
pub struct Lit<'a> {
    /// The styled items' indices.
    pub items: &'a [usize],
    /// The colour.
    pub colour: [f32; 3],
    /// The opacity.
    pub alpha: f32,
    /// The least width of a stroke or arc, device px.
    pub min_px: f32,
}

/// What the screen shows of the paper.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// The paper point at the top-left corner of the screen, mm.
    pub origin: (f64, f64),
    /// Device pixels per paper millimetre.
    pub zoom: f64,
    /// The screen, device pixels.
    pub size: (u32, u32),
    /// The thinnest a line is drawn, device pixels: one for a faithful
    /// grey, more for a bolder screen.
    pub min_width: f64,
}

impl View {
    /// The paper point under a screen pixel.
    pub fn to_paper(&self, px: (f64, f64)) -> (f64, f64) {
        (
            self.origin.0 + px.0 / self.zoom,
            self.origin.1 + px.1 / self.zoom,
        )
    }

    /// The screen pixel of a paper point.
    pub fn to_screen(&self, mm: (f64, f64)) -> (f64, f64) {
        (
            (mm.0 - self.origin.0) * self.zoom,
            (mm.1 - self.origin.1) * self.zoom,
        )
    }

    /// The same view zoomed by `factor` about a screen pixel, which stays
    /// over the same paper point.
    pub fn zoomed(self, factor: f64, about: (f64, f64)) -> Self {
        let anchor = self.to_paper(about);
        let zoom = (self.zoom * factor).clamp(0.05, 2000.0);
        Self {
            origin: (anchor.0 - about.0 / zoom, anchor.1 - about.1 / zoom),
            zoom,
            ..self
        }
    }

    /// The same view moved by a screen offset.
    pub fn panned(self, by: (f64, f64)) -> Self {
        Self {
            origin: (
                self.origin.0 - by.0 / self.zoom,
                self.origin.1 - by.1 / self.zoom,
            ),
            ..self
        }
    }

    /// The view that fits a paper box `(min, max)` with a margin, device
    /// pixels, on a screen of `size`.
    pub fn fitting(
        min: (f64, f64),
        max: (f64, f64),
        margin: f64,
        size: (u32, u32),
        min_width: f64,
    ) -> Self {
        let (w, h) = ((max.0 - min.0).max(1e-6), (max.1 - min.1).max(1e-6));
        let avail = (
            (size.0 as f64 - 2.0 * margin).max(1.0),
            (size.1 as f64 - 2.0 * margin).max(1.0),
        );
        let zoom = (avail.0 / w).min(avail.1 / h);
        let origin = (
            min.0 - (size.0 as f64 / zoom - w) / 2.0,
            min.1 - (size.1 as f64 / zoom - h) / 2.0,
        );
        Self {
            origin,
            zoom,
            size,
            min_width,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewUniform {
    origin_zoom: [f32; 4],
    screen: [f32; 4],
}

/// Draws styled scenes to a texture of one format.
pub struct Renderer {
    pipeline: wgpu::RenderPipeline,
    view_buffer: wgpu::Buffer,
    view_bind: wgpu::BindGroup,
    instances: wgpu::Buffer,
    count: u32,
    paper: Rgba,
    /// Which styled item each instance draws.
    owners: Vec<usize>,
    /// The instances as built, for recolouring without a rebuild.
    base: Vec<Instance>,
    /// Styled items not drawn (hidden layers), by item index.
    hidden: Vec<bool>,
    /// The items lit, in the order given.
    lit: Vec<LitItems>,
}

/// A [`Lit`] kept: its items by index in a lookup, its look.
struct LitItems {
    wanted: Vec<bool>,
    colour: [f32; 3],
    alpha: f32,
    min_px: f32,
}

impl Renderer {
    /// A renderer for targets of `format`.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lmsheets strokes"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("view"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let view_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("view"),
            size: std::mem::size_of::<ViewUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("view"),
            layout: &view_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: view_buffer.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("strokes"),
            bind_group_layouts: &[Some(&view_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("strokes"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: std::mem::size_of::<Instance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            view_buffer,
            view_bind,
            instances,
            count: 0,
            paper: Rgba {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            },
            owners: Vec::new(),
            base: Vec::new(),
            hidden: Vec::new(),
            lit: Vec::new(),
        }
    }

    /// Replace the scene.
    pub fn set_scene(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, styled: &Styled) {
        let (instances, owners) = instances::build(styled);
        self.paper = styled.paper;
        self.set_owned(device, queue, instances, owners);
    }

    /// Replace the instances with ones that each draw the styled item
    /// `owners[i]` (as [`instances::build`] returns, or [`instances::warp`]
    /// of it), so lighting, hiding and recolouring by item work on them.
    pub fn set_owned(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: Vec<Instance>,
        owners: Vec<usize>,
    ) {
        self.owners = owners;
        self.base = instances;
        self.hidden.clear();
        self.lit.clear();
        self.upload(device, queue);
    }

    /// The paper colour the target is cleared to.
    pub fn set_paper(&mut self, paper: Rgba) {
        self.paper = paper;
    }

    /// Light instances: each [`Lit`] recolours (and widens) the instances
    /// of its styled items; later ones win where items repeat. With none,
    /// every colour is restored. Only the instance buffer is rewritten: no
    /// scene is rebuilt.
    pub fn set_highlight(&mut self, queue: &wgpu::Queue, lit: &[Lit]) {
        let n = self.owners.iter().max().map_or(0, |m| m + 1);
        self.lit = lit
            .iter()
            .map(|l| {
                let mut wanted = vec![false; n];
                for &i in l.items.iter().filter(|&&i| i < n) {
                    wanted[i] = true;
                }
                LitItems {
                    wanted,
                    colour: l.colour,
                    alpha: l.alpha,
                    min_px: l.min_px,
                }
            })
            .collect();
        self.write(queue);
    }

    /// Stop drawing the styled items in `hidden` (a hidden layer); all
    /// others are drawn.
    pub fn set_hidden(&mut self, queue: &wgpu::Queue, hidden: &[usize]) {
        let n = self.owners.iter().max().map_or(0, |m| m + 1);
        self.hidden = vec![false; n];
        for &i in hidden.iter().filter(|&&i| i < n) {
            self.hidden[i] = true;
        }
        self.write(queue);
    }

    /// Give every styled item the colour `colour` returns for its index (a
    /// new theme, or the overlay's accent); lit items stay lit.
    pub fn set_colours(&mut self, queue: &wgpu::Queue, colour: impl Fn(usize) -> [f32; 4]) {
        for (inst, owner) in self.base.iter_mut().zip(&self.owners) {
            inst.rgba = colour(*owner);
        }
        self.write(queue);
    }

    /// The instances as built, before any lighting, hiding or recolouring.
    pub fn base(&self) -> &[Instance] {
        &self.base
    }

    /// The instances drawn now: the base with the hidden items left out and
    /// the lit ones recoloured. With no owners (bare instances) all are drawn.
    fn current(&self) -> Vec<Instance> {
        let hidden = |o: usize| self.hidden.get(o).copied().unwrap_or(false);
        let mut out = Vec::with_capacity(self.base.len());
        if self.owners.is_empty() {
            return self.base.clone();
        }
        for (inst, &owner) in self.base.iter().zip(&self.owners) {
            if hidden(owner) {
                continue;
            }
            let mut inst = *inst;
            for l in &self.lit {
                if l.wanted.get(owner).copied().unwrap_or(false) {
                    inst.rgba = [l.colour[0], l.colour[1], l.colour[2], l.alpha];
                    // Strokes and arcs keep a minimum width of their own.
                    if inst.k_a[0] < 1.5 {
                        inst.b[3] = inst.b[3].max(l.min_px);
                    }
                }
            }
            out.push(inst);
        }
        out
    }

    /// Rewrite the instance buffer from the base and the state.
    fn write(&mut self, queue: &wgpu::Queue) {
        let out = self.current();
        self.count = out.len() as u32;
        if !out.is_empty() {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&out));
        }
    }

    /// Grow the buffer if the base needs it, then write.
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let needed = (std::mem::size_of_val(self.base.as_slice()) as u64)
            .max(std::mem::size_of::<Instance>() as u64);
        if self.instances.size() < needed {
            self.instances = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instances"),
                size: needed.next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        self.write(queue);
    }

    /// Replace the scene with bare instances (an overlay drawn by
    /// [`Renderer::render_over`]); styled items are not tracked.
    pub fn set_instances(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[instances::Instance],
    ) {
        self.set_owned(device, queue, instances.to_vec(), Vec::new());
    }

    /// How many instances are drawn: the scene's, less the hidden items'.
    pub fn instance_count(&self) -> u32 {
        self.count
    }

    /// Which styled item each instance of the base draws, in order.
    pub fn owners(&self) -> &[usize] {
        &self.owners
    }

    /// Draw the scene under `view` into `target`, clearing it to the
    /// paper first.
    pub fn render(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        view: &View,
    ) {
        let p = self.paper;
        self.draw_with(queue, encoder, target, view, Some([p.r, p.g, p.b, p.a]));
    }

    /// Draw the scene under `view` into `target`, clearing it to `colour`
    /// (RGBA, premultiplied for a target that is composited later) first.
    pub fn render_cleared(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        view: &View,
        colour: [f64; 4],
    ) {
        self.draw_with(queue, encoder, target, view, Some(colour));
    }

    /// Draw the scene under `view` over what `target` holds (no clearing):
    /// an overlay.
    pub fn render_over(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        view: &View,
    ) {
        self.draw_with(queue, encoder, target, view, None);
    }

    fn draw_with(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        view: &View,
        clear: Option<[f64; 4]>,
    ) {
        if clear.is_none() && self.count == 0 {
            return;
        }
        let u = ViewUniform {
            origin_zoom: [
                view.origin.0 as f32,
                view.origin.1 as f32,
                view.zoom as f32,
                1.0,
            ],
            screen: [
                view.size.0 as f32,
                view.size.1 as f32,
                view.min_width as f32,
                0.0,
            ],
        };
        queue.write_buffer(&self.view_buffer, 0, bytemuck::bytes_of(&u));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("drawing"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: match clear {
                        Some([r, g, b, a]) => wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
                        None => wgpu::LoadOp::Load,
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if self.count > 0 {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.view_bind, &[]);
            pass.set_vertex_buffer(0, self.instances.slice(..));
            pass.draw(0..6, 0..self.count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_maps_both_ways_and_zooms_about_a_pixel() {
        let v = View::fitting((0.0, 0.0), (100.0, 50.0), 10.0, (1000, 500), 1.0);
        assert!((v.zoom - 9.6).abs() < 1e-9);
        let p = v.to_paper((500.0, 250.0));
        assert!((p.0 - 50.0).abs() < 1e-9 && (p.1 - 25.0).abs() < 1e-9);
        let back = v.to_screen(p);
        assert!((back.0 - 500.0).abs() < 1e-9);
        let z = v.zoomed(2.0, (100.0, 100.0));
        let q = z.to_paper((100.0, 100.0));
        let q0 = v.to_paper((100.0, 100.0));
        assert!((q.0 - q0.0).abs() < 1e-9 && (q.1 - q0.1).abs() < 1e-9);
        let m = v.panned((96.0, 0.0));
        assert!((m.origin.0 - (v.origin.0 - 10.0)).abs() < 1e-9);
    }
}
