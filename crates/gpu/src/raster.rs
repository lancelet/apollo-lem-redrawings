//! The raster tile layer: a bundle's scan pyramid drawn as textured quads
//! in crop pixel coordinates (docs/redraw-plan.md §4, §5).
//!
//! The scan is never resampled into paper coordinates: each tile is one
//! quad placed by the camera, `screen = (crop px - origin) * zoom`. A
//! frame pixel is `crop px + offset` ([`Pyramid::offset`]). The pure part
//! (the pyramid's description, [`Pyramid::pick_level`],
//! [`Pyramid::visible_tiles`]) needs no GPU; [`RasterLayer`] retains the
//! uploaded tiles and takes the camera as its only per-frame input.
//! Tiles are 8-bit grey (`R8Unorm`; 1-bit scans are grey 0/255 and
//! box-filtered into the coarser levels).

use serde::Deserialize;
use std::collections::HashMap;

/// What the screen shows of the scan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScanView {
    /// The crop pixel at the screen's top-left corner.
    pub origin: (f64, f64),
    /// Screen pixels per crop pixel.
    pub zoom: f64,
    /// The screen, device pixels.
    pub size: (u32, u32),
}

impl ScanView {
    /// The crop pixel under a screen pixel.
    pub fn to_crop(&self, px: (f64, f64)) -> (f64, f64) {
        (
            self.origin.0 + px.0 / self.zoom,
            self.origin.1 + px.1 / self.zoom,
        )
    }

    /// The view that shows the whole `extent` (crop pixels) centred.
    pub fn fitting(extent: (u32, u32), size: (u32, u32)) -> Self {
        let zoom = (size.0 as f64 / extent.0 as f64).min(size.1 as f64 / extent.1 as f64);
        Self::centred((extent.0 as f64 / 2.0, extent.1 as f64 / 2.0), zoom, size)
    }

    /// The view with crop pixel `centre` in the middle of the screen.
    pub fn centred(centre: (f64, f64), zoom: f64, size: (u32, u32)) -> Self {
        Self {
            origin: (
                centre.0 - size.0 as f64 / zoom / 2.0,
                centre.1 - size.1 as f64 / zoom / 2.0,
            ),
            zoom,
            size,
        }
    }

    /// The same view zoomed by `factor` about a screen pixel, which stays
    /// over the same crop pixel.
    pub fn zoomed(self, factor: f64, about: (f64, f64)) -> Self {
        let anchor = self.to_crop(about);
        let zoom = (self.zoom * factor).clamp(0.02, 64.0);
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
}

/// One tile of one level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileId {
    /// The level, 0 the full resolution.
    pub level: usize,
    /// Tile column.
    pub x: u32,
    /// Tile row.
    pub y: u32,
}

/// One level of the pyramid.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Level {
    /// Crop pixels per level pixel (1, 2, 4, ...).
    pub scale: u32,
    /// The level's size, level pixels.
    pub size: (u32, u32),
    /// Tiles across and down.
    pub tiles: (u32, u32),
}

/// The scan pyramid as the bundle's manifest describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pyramid {
    /// Tile edge, level pixels (edge tiles are smaller).
    pub tile: u32,
    /// The levels, finest first.
    pub levels: Vec<Level>,
    /// The path template, e.g. `frames/L{level}/{x}_{y}.webp`.
    pub path: String,
    /// The frame pixel of the crop's top-left corner.
    pub offset: (i64, i64),
}

#[derive(Deserialize)]
struct ManifestPyramid {
    tile: u32,
    path: String,
    levels: Vec<Level>,
}

#[derive(Deserialize)]
struct ManifestCrop {
    offset: (i64, i64),
}

#[derive(Deserialize)]
struct Manifest {
    pyramid: ManifestPyramid,
    crop: ManifestCrop,
}

/// Why a manifest or a tile could not be used.
#[derive(Debug)]
pub struct RasterError(pub String);

impl std::fmt::Display for RasterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RasterError {}

impl Pyramid {
    /// Read the pyramid and crop of a bundle's `manifest.json`.
    pub fn from_manifest(json: &str) -> Result<Self, RasterError> {
        let m: Manifest =
            serde_json::from_str(json).map_err(|e| RasterError(format!("manifest: {e}")))?;
        if m.pyramid.levels.is_empty() || m.pyramid.tile == 0 {
            return Err(RasterError("manifest: an empty pyramid".into()));
        }
        Ok(Self {
            tile: m.pyramid.tile,
            levels: m.pyramid.levels,
            path: m.pyramid.path,
            offset: m.crop.offset,
        })
    }

    /// The full-resolution size, crop pixels.
    pub fn extent(&self) -> (u32, u32) {
        self.levels[0].size
    }

    /// The tile's path under the bundle directory.
    pub fn tile_path(&self, id: TileId) -> String {
        self.path
            .replace("{level}", &id.level.to_string())
            .replace("{x}", &id.x.to_string())
            .replace("{y}", &id.y.to_string())
    }

    /// The level to draw at `zoom` (screen px per crop px): the coarsest
    /// whose pixels are no larger than a screen pixel, so nothing is
    /// magnified; the finest when even that is magnified.
    pub fn pick_level(&self, zoom: f64) -> usize {
        let mut pick = 0;
        for (i, l) in self.levels.iter().enumerate() {
            if zoom * l.scale as f64 <= 1.0 + 1e-9 {
                pick = i;
            }
        }
        pick
    }

    /// A tile's box in crop pixels `(x0, y0, x1, y1)`.
    pub fn tile_rect(&self, id: TileId) -> [f32; 4] {
        let l = &self.levels[id.level];
        let s = l.scale as f64;
        let (x0, y0) = (id.x * self.tile, id.y * self.tile);
        let (x1, y1) = (
            (x0 + self.tile).min(l.size.0),
            (y0 + self.tile).min(l.size.1),
        );
        [
            (x0 as f64 * s) as f32,
            (y0 as f64 * s) as f32,
            (x1 as f64 * s) as f32,
            (y1 as f64 * s) as f32,
        ]
    }

    /// The tiles of `level` that touch the screen, row-major.
    pub fn visible_tiles(&self, level: usize, view: &ScanView) -> Vec<TileId> {
        let l = &self.levels[level];
        let span = (self.tile as f64) * l.scale as f64;
        let (x1, y1) = view.to_crop((view.size.0 as f64, view.size.1 as f64));
        let range = |lo: f64, hi: f64, n: u32| -> std::ops::Range<u32> {
            let a = (lo / span).floor().max(0.0);
            let b = (hi / span).ceil().min(n as f64);
            if b <= a {
                0..0
            } else {
                a as u32..b as u32
            }
        };
        let (xs, ys) = (
            range(view.origin.0, x1, l.tiles.0),
            range(view.origin.1, y1, l.tiles.1),
        );
        ys.flat_map(|y| xs.clone().map(move |x| TileId { level, x, y }))
            .collect()
    }
}

/// A tile read from disk and decoded to grey.
#[cfg(not(target_arch = "wasm32"))]
pub struct DecodedTile {
    /// Width, pixels.
    pub width: u32,
    /// Height, pixels.
    pub height: u32,
    /// Row-major grey.
    pub grey: Vec<u8>,
    /// Bytes read from the file.
    pub file_bytes: usize,
}

/// Read and decode one tile of a bundle directory.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_tile(
    dir: &std::path::Path,
    pyramid: &Pyramid,
    id: TileId,
) -> Result<DecodedTile, RasterError> {
    let path = dir.join(pyramid.tile_path(id));
    let bytes =
        std::fs::read(&path).map_err(|e| RasterError(format!("{}: {e}", path.display())))?;
    let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP)
        .map_err(|e| RasterError(format!("{}: {e}", path.display())))?
        .into_luma8();
    Ok(DecodedTile {
        width: img.width(),
        height: img.height(),
        file_bytes: bytes.len(),
        grey: img.into_raw(),
    })
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUniform {
    origin_zoom: [f32; 4],
    screen: [f32; 4],
    ink: [f32; 4],
    paper: [f32; 4],
}

struct Tile {
    bind: wgpu::BindGroup,
    bytes: u64,
    rect: [f32; 4],
}

/// Which of the `held` tiles (id, bytes) to drop to bring `total` bytes
/// down to `budget`: none of `keep`, the finest levels first (lowest level
/// number), then by position, so the choice is deterministic.
fn evictions(held: &[(TileId, u64)], total: u64, budget: u64, keep: &[TileId]) -> Vec<TileId> {
    let mut order: Vec<_> = held.iter().filter(|(id, _)| !keep.contains(id)).collect();
    order.sort_by_key(|(id, _)| (id.level, id.y, id.x));
    let mut total = total;
    let mut out = Vec::new();
    for (id, bytes) in order {
        if total <= budget {
            break;
        }
        total -= bytes;
        out.push(*id);
    }
    out
}

/// Retains uploaded tiles and draws the ones the camera sees.
pub struct RasterLayer {
    pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind: wgpu::BindGroup,
    tile_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    instances: wgpu::Buffer,
    tiles: HashMap<TileId, Tile>,
    texture_bytes: u64,
    /// Colour of ink (grey 0), linear RGBA as written to the target.
    pub ink: [f32; 4],
    /// Colour of paper (grey 255).
    pub paper: [f32; 4],
    /// What the target is cleared to (outside every tile).
    pub background: [f32; 4],
}

impl RasterLayer {
    /// A layer for targets of `format`.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lmsheets raster"),
            source: wgpu::ShaderSource::Wgsl(include_str!("raster.wgsl").into()),
        });
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("raster camera"),
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
        let tile_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("raster tile"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("raster camera"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("raster camera"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("raster"),
            bind_group_layouts: &[Some(&camera_layout), Some(&tile_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("raster"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        // Magnified pixels stay square; minified ones average.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("raster"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("raster tiles"),
            size: 16,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            camera_buffer,
            camera_bind,
            tile_layout,
            sampler,
            instances,
            tiles: HashMap::new(),
            texture_bytes: 0,
            ink: [0.04, 0.04, 0.06, 1.0],
            paper: [0.97, 0.96, 0.92, 1.0],
            background: [0.30, 0.30, 0.32, 1.0],
        }
    }

    /// Upload a tile (8-bit grey, row-major) whose box in crop pixels is
    /// `pyramid.tile_rect(id)`. Replaces a tile already held.
    pub fn insert(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pyramid: &Pyramid,
        id: TileId,
        size: (u32, u32),
        grey: &[u8],
    ) {
        assert_eq!(grey.len(), size.0 as usize * size.1 as usize, "grey tile");
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("scan tile"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            grey,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.0),
                rows_per_image: Some(size.1),
            },
            wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scan tile"),
            layout: &self.tile_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let rect = pyramid.tile_rect(id);
        if let Some(old) = self.tiles.insert(
            id,
            Tile {
                bind,
                bytes: grey.len() as u64,
                rect,
            },
        ) {
            // A replaced tile's texture goes with its bind group.
            self.texture_bytes -= old.bytes;
        }
        self.texture_bytes += grey.len() as u64;
    }

    /// Whether a tile is held.
    pub fn has(&self, id: TileId) -> bool {
        self.tiles.contains_key(&id)
    }

    /// How many tiles are held, and the texture bytes they occupy.
    pub fn held(&self) -> (usize, u64) {
        (self.tiles.len(), self.texture_bytes)
    }

    /// Drop held tiles until the texture bytes are at most `budget`, never
    /// one in `keep` (what the view shows), finest level first. Returns how
    /// many were dropped. For a memory-poor device.
    pub fn evict_to(&mut self, budget: u64, keep: &[TileId]) -> usize {
        let held: Vec<(TileId, u64)> = self.tiles.iter().map(|(id, t)| (*id, t.bytes)).collect();
        let drop = evictions(&held, self.texture_bytes, budget, keep);
        for id in &drop {
            if let Some(t) = self.tiles.remove(id) {
                self.texture_bytes -= t.bytes;
            }
        }
        drop.len()
    }

    /// The tiles `view` needs that are not held yet.
    pub fn missing(&self, pyramid: &Pyramid, view: &ScanView) -> Vec<TileId> {
        let level = pyramid.pick_level(view.zoom);
        pyramid
            .visible_tiles(level, view)
            .into_iter()
            .filter(|id| !self.has(*id))
            .collect()
    }

    /// Draw the held tiles of the level `view` calls for into `target`,
    /// clearing it to the background first. Returns the tiles drawn.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        pyramid: &Pyramid,
        view: &ScanView,
    ) -> usize {
        let u = CameraUniform {
            origin_zoom: [
                view.origin.0 as f32,
                view.origin.1 as f32,
                view.zoom as f32,
                0.0,
            ],
            screen: [view.size.0 as f32, view.size.1 as f32, 0.0, 0.0],
            ink: self.ink,
            paper: self.paper,
        };
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&u));
        let level = pyramid.pick_level(view.zoom);
        let drawn: Vec<TileId> = pyramid
            .visible_tiles(level, view)
            .into_iter()
            .filter(|id| self.has(*id))
            .collect();
        let rects: Vec<[f32; 4]> = drawn.iter().map(|id| self.tiles[id].rect).collect();
        let bytes: &[u8] = bytemuck::cast_slice(&rects);
        if self.instances.size() < bytes.len() as u64 {
            self.instances = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("raster tiles"),
                size: (bytes.len() as u64).next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.instances, 0, bytes);
        }
        let [r, g, b, a] = self.background.map(f64::from);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scan"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.camera_bind, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        for (i, id) in drawn.iter().enumerate() {
            pass.set_bind_group(1, &self.tiles[id].bind, &[]);
            pass.draw(0..6, i as u32..i as u32 + 1);
        }
        drawn.len()
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct OwnerUniform {
    origin_zoom: [f32; 4],
    screen: [f32; 4],
    tint: [f32; 4],
    serving_tint: [f32; 4],
    pin_tint: [f32; 4],
    counts: [u32; 4],
    values: [[u32; 4]; 16],
}

/// The most node values the owner layer lights at once.
pub const MAX_LIT_NODES: usize = 64;

/// What the owner layer lights: tile values (node index + 1) of the
/// hovered group's members, tinted fully, and of the nodes that only serve
/// it, tinted lightly, in one colour. Over [`MAX_LIT_NODES`] values the
/// excess (serving first) is dropped.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OwnerLit {
    /// The colour (linear RGB).
    pub colour: [f32; 3],
    /// The colour of the nodes that only serve (their own role's); the
    /// members' colour where none.
    pub serving_colour: Option<[f32; 3]>,
    /// The pinned group's colour, where it is lit besides the hovered one.
    pub pin_colour: [f32; 3],
    /// The pinned group's values (none: no pin lit apart from the hover).
    pub pin: Vec<u32>,
    /// The members' values.
    pub members: Vec<u32>,
    /// The serving nodes' values.
    pub serving: Vec<u32>,
}

struct OwnerTileGpu {
    bind: wgpu::BindGroup,
    rect: [f32; 4],
}

/// The owner map's tiles (16-bit, one value per crop pixel: node index + 1,
/// 0 for none), drawn over the scan with the lit nodes' pixels tinted.
/// Tiles are the bundle's full-resolution owner tiles, `tile` crop px a
/// side; they are held as `R16Uint` and compared with a small uniform set
/// of node values in the shader.
pub struct OwnerLayer {
    pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind: wgpu::BindGroup,
    tile_layout: wgpu::BindGroupLayout,
    instances: wgpu::Buffer,
    tiles: HashMap<(u32, u32), OwnerTileGpu>,
    /// The tint's opacity.
    pub opacity: f32,
}

impl OwnerLayer {
    /// A layer for targets of `format`.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lmsheets owners"),
            source: wgpu::ShaderSource::Wgsl(include_str!("owner.wgsl").into()),
        });
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("owner camera"),
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
        let tile_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("owner tile"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("owner camera"),
            size: std::mem::size_of::<OwnerUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("owner camera"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("owners"),
            bind_group_layouts: &[Some(&camera_layout), Some(&tile_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("owners"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4],
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
            label: Some("owner tiles"),
            size: 16,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            camera_buffer,
            camera_bind,
            tile_layout,
            instances,
            tiles: HashMap::new(),
            opacity: 0.8,
        }
    }

    /// Upload owner tile `(x, y)`: `values` row-major, `size` px, its
    /// top-left at crop pixel `(x * tile, y * tile)`.
    pub fn insert(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        tile: u32,
        at: (u32, u32),
        size: (u32, u32),
        values: &[u16],
    ) {
        assert_eq!(
            values.len(),
            size.0 as usize * size.1 as usize,
            "owner tile"
        );
        let extent = wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("owner tile"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R16Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            bytemuck::cast_slice(values),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.0 * 2),
                rows_per_image: Some(size.1),
            },
            extent,
        );
        let view = texture.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("owner tile"),
            layout: &self.tile_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        let (x0, y0) = ((at.0 * tile) as f32, (at.1 * tile) as f32);
        self.tiles.insert(
            at,
            OwnerTileGpu {
                bind,
                rect: [x0, y0, x0 + size.0 as f32, y0 + size.1 as f32],
            },
        );
    }

    /// Whether owner tile `(x, y)` is held.
    pub fn has(&self, x: u32, y: u32) -> bool {
        self.tiles.contains_key(&(x, y))
    }

    /// How many tiles are held.
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    /// Whether none are.
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// The owner tiles `view` needs and does not hold, for a tile edge of
    /// `tile` crop px and `tiles` across and down.
    pub fn missing(&self, tile: u32, tiles: (u32, u32), view: &ScanView) -> Vec<(u32, u32)> {
        let (x1, y1) = view.to_crop((view.size.0 as f64, view.size.1 as f64));
        let t = tile as f64;
        let range = |lo: f64, hi: f64, n: u32| {
            let a = (lo / t).floor().max(0.0) as u32;
            let b = ((hi / t).ceil().max(0.0) as u32).min(n);
            a..b.max(a)
        };
        let (xs, ys) = (
            range(view.origin.0, x1, tiles.0),
            range(view.origin.1, y1, tiles.1),
        );
        ys.flat_map(|y| xs.clone().map(move |x| (x, y)))
            .filter(|t| !self.tiles.contains_key(t))
            .collect()
    }

    /// Tint the pixels of the lit nodes over what `target` holds, under
    /// `view`. Draws nothing and records no pass when nothing is lit or no
    /// tile is held.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        view: &ScanView,
        lit: Option<&OwnerLit>,
    ) {
        let Some(lit) =
            lit.filter(|l| !l.members.is_empty() || !l.serving.is_empty() || !l.pin.is_empty())
        else {
            return;
        };
        let c = lit.colour;
        let members = &lit.members[..lit.members.len().min(MAX_LIT_NODES)];
        let room = MAX_LIT_NODES - members.len();
        let serving = &lit.serving[..lit.serving.len().min(room)];
        let room = room - serving.len();
        let pin = &lit.pin[..lit.pin.len().min(room)];
        let mut values = [[0u32; 4]; 16];
        for (i, v) in members.iter().chain(serving).chain(pin).enumerate() {
            values[i / 4][i % 4] = *v;
        }
        let (x1, y1) = view.to_crop((view.size.0 as f64, view.size.1 as f64));
        let drawn: Vec<&OwnerTileGpu> = self
            .tiles
            .values()
            .filter(|t| {
                (t.rect[0] as f64) < x1
                    && (t.rect[2] as f64) > view.origin.0
                    && (t.rect[1] as f64) < y1
                    && (t.rect[3] as f64) > view.origin.1
            })
            .collect();
        if drawn.is_empty() {
            return;
        }
        let u = OwnerUniform {
            origin_zoom: [
                view.origin.0 as f32,
                view.origin.1 as f32,
                view.zoom as f32,
                0.0,
            ],
            screen: [view.size.0 as f32, view.size.1 as f32, 0.0, 0.0],
            tint: [c[0], c[1], c[2], self.opacity],
            serving_tint: {
                let s = lit.serving_colour.unwrap_or(c);
                [s[0], s[1], s[2], self.opacity]
            },
            pin_tint: [
                lit.pin_colour[0],
                lit.pin_colour[1],
                lit.pin_colour[2],
                self.opacity,
            ],
            counts: [
                members.len() as u32,
                serving.len() as u32,
                pin.len() as u32,
                0,
            ],
            values,
        };
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&u));
        let rects: Vec<[f32; 4]> = drawn.iter().map(|t| t.rect).collect();
        let bytes: &[u8] = bytemuck::cast_slice(&rects);
        if self.instances.size() < bytes.len() as u64 {
            self.instances = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("owner tiles"),
                size: (bytes.len() as u64).next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        queue.write_buffer(&self.instances, 0, bytes);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("owners"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.camera_bind, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        for (i, t) in drawn.iter().enumerate() {
            pass.set_bind_group(1, &t.bind, &[]);
            pass.draw(0..6, i as u32..i as u32 + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 17619's pyramid: 6623 x 4364, 1024 px tiles, three levels.
    fn pyramid() -> Pyramid {
        Pyramid::from_manifest(
            r#"{"crop":{"offset":[2039,1615]},"pyramid":{"tile":1024,
            "path":"frames/L{level}/{x}_{y}.webp","levels":[
            {"level":0,"scale":1,"size":[6623,4364],"tiles":[7,5]},
            {"level":1,"scale":2,"size":[3312,2182],"tiles":[4,3]},
            {"level":2,"scale":4,"size":[1656,1091],"tiles":[2,2]}]}}"#,
        )
        .unwrap()
    }

    #[test]
    fn the_manifest_gives_the_pyramid_and_paths() {
        let p = pyramid();
        assert_eq!(p.offset, (2039, 1615));
        assert_eq!(p.extent(), (6623, 4364));
        assert_eq!(
            p.tile_path(TileId {
                level: 1,
                x: 3,
                y: 2
            }),
            "frames/L1/3_2.webp"
        );
        assert!(Pyramid::from_manifest("{}").is_err());
    }

    #[test]
    fn the_level_is_the_coarsest_not_magnified() {
        let p = pyramid();
        // zoom: screen px per crop px.
        for (zoom, level) in [
            (0.1, 2),
            (0.24, 2),
            (0.25, 2),
            (0.26, 1),
            (0.5, 1),
            (0.51, 0),
            (1.0, 0),
            (8.0, 0),
        ] {
            assert_eq!(p.pick_level(zoom), level, "zoom {zoom}");
        }
    }

    #[test]
    fn only_tiles_that_touch_the_screen_are_visible() {
        let p = pyramid();
        let all = ScanView::fitting(p.extent(), (1600, 1000));
        assert_eq!(p.pick_level(all.zoom), 2);
        assert_eq!(p.visible_tiles(2, &all).len(), 4);
        let big = ScanView::fitting(p.extent(), (3000, 2000));
        assert_eq!(p.pick_level(big.zoom), 1);
        assert_eq!(p.visible_tiles(1, &big).len(), 12);
        // 1:1 on a 900 x 700 screen at crop (3000, 2000): tiles 2..=3 by 1..=2.
        let v = ScanView {
            origin: (3000.0, 2000.0),
            zoom: 1.0,
            size: (900, 700),
        };
        let t = p.visible_tiles(0, &v);
        let ids: Vec<(u32, u32)> = t.iter().map(|t| (t.x, t.y)).collect();
        assert_eq!(ids, [(2, 1), (3, 1), (2, 2), (3, 2)]);
        // Off the sheet: nothing. Edge tiles are clipped to the level.
        let off = ScanView {
            origin: (-3000.0, -3000.0),
            zoom: 1.0,
            size: (900, 700),
        };
        assert!(p.visible_tiles(0, &off).is_empty());
        let corner = p.tile_rect(TileId {
            level: 0,
            x: 6,
            y: 4,
        });
        assert_eq!(corner, [6144.0, 4096.0, 6623.0, 4364.0]);
        let coarse = p.tile_rect(TileId {
            level: 2,
            x: 1,
            y: 1,
        });
        assert_eq!(coarse, [4096.0, 4096.0, 6624.0, 4364.0]);
    }

    #[test]
    fn zooming_keeps_the_pixel_under_the_cursor() {
        let v = ScanView::fitting((6623, 4364), (1600, 1000));
        let z = v.zoomed(3.0, (400.0, 300.0));
        let (a, b) = (v.to_crop((400.0, 300.0)), z.to_crop((400.0, 300.0)));
        assert!((a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9);
    }

    #[test]
    fn eviction_drops_the_finest_unkept_tiles_first_until_under_budget() {
        let id = |level, x, y| TileId { level, x, y };
        let held = [
            (id(0, 0, 0), 10),
            (id(0, 1, 0), 10),
            (id(1, 0, 0), 5),
            (id(2, 0, 0), 1),
        ];
        let total = 26;
        assert!(evictions(&held, total, 30, &[]).is_empty());
        assert_eq!(evictions(&held, total, 20, &[]), [id(0, 0, 0)]);
        // The kept tile stays; the next finest goes.
        assert_eq!(evictions(&held, total, 20, &[id(0, 0, 0)]), [id(0, 1, 0)]);
        // Over budget with everything kept: nothing can go.
        let all: Vec<_> = held.iter().map(|h| h.0).collect();
        assert!(evictions(&held, total, 0, &all).is_empty());
        assert_eq!(evictions(&held, total, 0, &[id(2, 0, 0)]).len(), 3);
    }
}
