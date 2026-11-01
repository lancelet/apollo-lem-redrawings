//! The inspector driving two panes, headless: the redraw in paper
//! coordinates on the left, the scan tiles on the right, their cameras
//! locked through the registration chain by `lmsheets-inspector`
//! (docs/redraw-plan.md chunk 19). Every view change is a [`Msg`] applied
//! by the pure `update`; this file only draws what the model's cameras say.
//!
//!     cargo run -q --release -p lmsheets-gpu --example inspect -- BUNDLE/LDW280-17619/sh3 \
//!         --screenshot out.png [--size WxH] [--mode side|redraw|scan] \
//!         [--look-at X,Y --scale S] [--log msgs.json] [--hover X,Y [--alt]]
//!
//! `--look-at` is a paper point (mm) to centre and `--scale` the paper scale
//! (screen px per mm); both become pans and zooms on the redraw pane. `--log`
//! replays a recorded JSON `Vec<Msg>` after that. `--hover X,Y` puts the
//! pointer at a window pixel: the inspector's pure pick (owner map when the scan
//! pane is zoomed in, else the redraw's geometry) names the node under it, which
//! is tinted in the scan pane (the owner tiles in the shader) and recoloured in
//! the redraw pane; the whole group (a dimension with its witness lines, the
//! centre lines through one centre) is lit unless `--alt` takes the piece.
//! Where the owner map is out of reach (the scan pane zoomed out) the group's
//! geometry is drawn through the chain as a thick outline instead. The redraw is the existing
//! lmsheets-render -> lmsheets-gpu stroke path; its lettering is not drawn
//! yet (the gpu renderer has no text), so notes and dimension values are
//! missing from the left pane. No window is opened.

use lmsheets_document::state::document::Document;
use lmsheets_document::value::style::Purpose;
use lmsheets_gpu::instances::segment;
use lmsheets_gpu::raster::{load_tile, OwnerLayer, OwnerLit, Pyramid, RasterLayer, ScanView};
use lmsheets_gpu::{Lit, Renderer, View};
use lmsheets_inspector::pick::{LIT_MIN_PX, OWNER_ZOOM, SERVING_MIN_PX};
use lmsheets_inspector::{
    update, Chain, Groups, Mode, Model, Msg, OwnerTable, OwnerTile, Pane, Picker,
};
use lmsheets_render::conventions::Conventions;
use lmsheets_render::draw::draw;
use lmsheets_render::resolve::resolve;
use lmsheets_render::scene::Scene;
use lmsheets_render::theme::Theme;
use std::path::PathBuf;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

struct Args {
    dir: PathBuf,
    out: PathBuf,
    size: (u32, u32),
    mode: Mode,
    look_at: Option<(f64, f64)>,
    scale: Option<f64>,
    log: Option<PathBuf>,
    hover: Option<(f64, f64)>,
    alt: bool,
}

fn parse_args() -> Args {
    let mut a = Args {
        dir: PathBuf::new(),
        out: PathBuf::new(),
        size: (1800, 700),
        mode: Mode::SideBySide,
        look_at: None,
        scale: None,
        log: None,
        hover: None,
        alt: false,
    };
    let pair = |s: &str, sep: char| -> (f64, f64) {
        let (x, y) = s.split_once(sep).expect("two numbers");
        (x.parse().expect("a number"), y.parse().expect("a number"))
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = || it.next().expect("a value after the flag");
        match arg.as_str() {
            "--screenshot" => a.out = PathBuf::from(val()),
            "--size" => {
                let (w, h) = pair(&val(), 'x');
                a.size = (w as u32, h as u32);
            }
            "--mode" => {
                a.mode = match val().as_str() {
                    "side" => Mode::SideBySide,
                    "redraw" => Mode::RedrawOnly,
                    "scan" => Mode::ScanOnly,
                    m => panic!("mode {m}: side, redraw or scan"),
                }
            }
            "--look-at" => a.look_at = Some(pair(&val(), ',')),
            "--scale" => a.scale = Some(val().parse().expect("a number")),
            "--log" => a.log = Some(PathBuf::from(val())),
            "--hover" => a.hover = Some(pair(&val(), ',')),
            "--alt" => a.alt = true,
            _ => a.dir = PathBuf::from(arg),
        }
    }
    assert!(
        a.dir.join("manifest.json").exists(),
        "give a bundle sheet directory"
    );
    assert!(!a.out.as_os_str().is_empty(), "--screenshot OUT.png");
    a
}

fn target(device: &wgpu::Device, size: (u32, u32), usage: wgpu::TextureUsages) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pane"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage,
        view_formats: &[],
    })
}

/// The messages that put paper point `at` in the middle of the shown pane at
/// `scale`: a pan, then a zoom about the centre. Dragging moves the content
/// with the pointer, so the content point goes to the middle by dragging
/// (middle - point) in screen px.
fn look(m: &mut Model, at: (f64, f64), scale: Option<f64>) {
    let (pane, size) = match m.pane_size(Pane::Redraw) {
        Some(size) => (Pane::Redraw, size),
        None => (Pane::Scan, m.pane_size(Pane::Scan).unwrap()),
    };
    let middle = (size.0 as f64 / 2.0, size.1 as f64 / 2.0);
    let by = match pane {
        Pane::Redraw => {
            let here = m.centre();
            ((here.0 - at.0) * m.scale(), (here.1 - at.1) * m.scale())
        }
        Pane::Scan => {
            let (c, now, z) = (
                m.chain().paper_to_scan(at),
                m.scan_centre(),
                m.scan_camera().unwrap().zoom,
            );
            ((now.0 - c.0) * z, (now.1 - c.1) * z)
        }
    };
    update(m, Msg::Pan { pane, by });
    if let Some(s) = scale {
        let factor = s / m.scale();
        update(
            m,
            Msg::Zoom {
                pane,
                factor,
                about: middle,
            },
        );
    }
}

/// The hover colour of this example (the viewer takes its colours from the
/// theme's roles).
const HOVER: [f32; 3] = [1.0, 0.55, 0.1];

fn main() {
    let args = parse_args();
    let chain = Chain::load(&args.dir).expect("a bundle's registration");
    let mut model = Model::new(chain, args.size);
    update(&mut model, Msg::SetMode(args.mode));
    update(&mut model, Msg::Fit);
    if let Some(at) = args.look_at {
        look(&mut model, at, args.scale);
    } else if let Some(s) = args.scale {
        let about = (args.size.0 as f64 / 4.0, args.size.1 as f64 / 2.0);
        let factor = s / model.scale();
        update(
            &mut model,
            Msg::Zoom {
                pane: Pane::Redraw,
                factor,
                about,
            },
        );
    }
    if let Some(p) = &args.log {
        let log: Vec<Msg> =
            serde_json::from_str(&std::fs::read_to_string(p).expect("a log")).expect("Msg JSON");
        for m in log {
            update(&mut model, m);
        }
    }

    // The redraw's styled scene, and the picker over it and the owner map.
    let text = std::fs::read_to_string(args.dir.join("document.json")).unwrap();
    let doc = Document::parse(&text).expect("the bundle's document");
    let scene = Scene::of(draw(&doc, Purpose::Screen));
    let styled = resolve(&scene, &doc, &Theme::screen(), Conventions::default());
    let manifest = std::fs::read_to_string(args.dir.join("manifest.json")).unwrap();
    let pyramid = Pyramid::from_manifest(&manifest).expect("a pyramid");
    let table = OwnerTable::from_json(
        &std::fs::read_to_string(args.dir.join("owners/owners.json")).expect("owners.json"),
    )
    .expect("a node table");
    let groups = Groups::new(&doc, &scene.planes[0], &table);
    let mut picker = Picker::new(table, pyramid.tile, &scene.planes[0], &doc, &styled);
    // The pane pixel of --hover. The owner tiles under the scan view are
    // read when it is zoomed in (the app fetches them the same way).
    let hover = args.hover.and_then(|w| {
        [Pane::Redraw, Pane::Scan].into_iter().find_map(|pane| {
            let (o, s) = (model.pane_origin(pane)?, model.pane_size(pane)?);
            let at = (w.0 - o.0 as f64, w.1 - o.1 as f64);
            (at.0 >= 0.0 && at.1 >= 0.0 && at.0 < s.0 as f64 && at.1 < s.1 as f64)
                .then_some((pane, at))
        })
    });
    let mut owner_tiles: Vec<(u32, u32, OwnerTile)> = Vec::new();
    if let Some(cam) = model.scan_camera().filter(|c| c.zoom >= OWNER_ZOOM) {
        let view = ScanView {
            origin: cam.origin,
            zoom: cam.zoom,
            size: cam.size,
        };
        let t = pyramid.tile as f64;
        let (x1, y1) = view.to_crop((cam.size.0 as f64, cam.size.1 as f64));
        let l0 = &pyramid.levels[0];
        for y in
            (view.origin.1 / t).floor().max(0.0) as u32..((y1 / t).ceil() as u32).min(l0.tiles.1)
        {
            for x in (view.origin.0 / t).floor().max(0.0) as u32
                ..((x1 / t).ceil() as u32).min(l0.tiles.0)
            {
                let bytes = std::fs::read(args.dir.join(format!("owners/{x}_{y}.png")))
                    .expect("an owner tile");
                let tile = OwnerTile::from_png(&bytes).expect("a 16-bit tile");
                picker.owners.insert(x, y, tile.clone());
                owner_tiles.push((x, y, tile));
            }
        }
    }
    if let Some((pane, at)) = hover {
        let node = picker.pick(&model, pane, at);
        let highlight = node.map(|n| groups.highlight(n, args.alt));
        update(
            &mut model,
            Msg::Hover {
                pane,
                at,
                node,
                highlight,
            },
        );
        println!(
            "hover: {pane:?} pane pixel ({:.0}, {:.0}) -> {}",
            at.0,
            at.1,
            match node.and_then(|n| picker.table.get(n)) {
                Some((kind, name)) => format!(
                    "{kind} {name} (node {}, {} redraw items; group of {} nodes, {} serving)",
                    node.unwrap().0,
                    picker.items_of(node.unwrap()).len(),
                    model.highlight().members.len(),
                    model.highlight().serving.len()
                ),
                None => "nothing".into(),
            }
        );
    }

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        pollster::block_on(instance.request_adapter(&Default::default())).expect("an adapter");
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("a device");

    let window = args.size;
    let out_tex = target(
        &device,
        window,
        wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
    );
    let pane_usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
    let mut enc = device.create_command_encoder(&Default::default());

    // The scan pane.
    if let Some(cam) = model.scan_camera() {
        let mut layer = RasterLayer::new(&device, FORMAT);
        let view = ScanView {
            origin: cam.origin,
            zoom: cam.zoom,
            size: cam.size,
        };
        for id in layer.missing(&pyramid, &view) {
            let t = load_tile(&args.dir, &pyramid, id).expect("a tile");
            layer.insert(&device, &queue, &pyramid, id, (t.width, t.height), &t.grey);
        }
        let tex = target(&device, cam.size, pane_usage);
        let n = layer.render(
            &device,
            &queue,
            &mut enc,
            &tex.create_view(&Default::default()),
            &pyramid,
            &view,
        );
        let mut owners = OwnerLayer::new(&device, FORMAT);
        for (x, y, t) in &owner_tiles {
            owners.insert(
                &device,
                &queue,
                pyramid.tile,
                (*x, *y),
                (t.width, t.height),
                &t.values,
            );
        }
        let colour = HOVER;
        let h = model.highlight();
        if cam.zoom >= OWNER_ZOOM {
            let lit = OwnerLit {
                colour,
                serving_colour: None,
                pin_colour: [0.0; 3],
                pin: Vec::new(),
                members: h.members.iter().map(|n| n.0 + 1).collect(),
                serving: h.serving.iter().map(|n| n.0 + 1).collect(),
            };
            owners.render(
                &device,
                &queue,
                &mut enc,
                &tex.create_view(&Default::default()),
                &view,
                Some(&lit),
            );
        } else {
            // Out of the owner map's reach: the group's geometry through
            // the chain, drawn thick.
            let mut outline = Renderer::new(&device, FORMAT);
            let mut inst = Vec::new();
            for (nodes, alpha, px) in [
                (&h.serving, 0.55f32, SERVING_MIN_PX),
                (&h.members, 0.9, LIT_MIN_PX),
            ] {
                for [a, b] in picker.outline(nodes, model.chain()) {
                    inst.push(segment(
                        a,
                        b,
                        1e-4,
                        [colour[0], colour[1], colour[2], alpha],
                        px,
                    ));
                }
            }
            outline.set_instances(&device, &queue, &inst);
            outline.render_over(
                &queue,
                &mut enc,
                &tex.create_view(&Default::default()),
                &View {
                    origin: cam.origin,
                    zoom: cam.zoom,
                    size: cam.size,
                    min_width: 1.0,
                },
            );
        }
        println!(
            "scan: zoom {:.4} crop px, centre ({:.1}, {:.1}), {n} tiles",
            cam.zoom,
            cam.origin.0 + cam.size.0 as f64 / cam.zoom / 2.0,
            cam.origin.1 + cam.size.1 as f64 / cam.zoom / 2.0
        );
        copy(
            &mut enc,
            &tex,
            &out_tex,
            model.pane_origin(Pane::Scan).unwrap(),
            cam.size,
        );
    }

    // The redraw pane.
    if let Some(cam) = model.redraw_camera() {
        let mut renderer = Renderer::new(&device, FORMAT);
        renderer.set_scene(&device, &queue, &styled);
        if let Some(n) = model.hovered() {
            let _ = n;
            let colour = HOVER;
            let h = model.highlight();
            let (members, serving) = (picker.items_in(&h.members), picker.items_in(&h.serving));
            renderer.set_highlight(
                &queue,
                &[
                    Lit {
                        items: &serving,
                        colour,
                        alpha: 0.55,
                        min_px: SERVING_MIN_PX,
                    },
                    Lit {
                        items: &members,
                        colour,
                        alpha: 1.0,
                        min_px: LIT_MIN_PX,
                    },
                ],
            );
        }
        let view = View {
            origin: cam.origin,
            zoom: cam.zoom,
            size: cam.size,
            min_width: 1.0,
        };
        let tex = target(&device, cam.size, pane_usage);
        renderer.render(
            &queue,
            &mut enc,
            &tex.create_view(&Default::default()),
            &view,
        );
        println!(
            "redraw: scale {:.4} px/mm, centre ({:.2}, {:.2}) mm, {} instances",
            cam.zoom,
            model.centre().0,
            model.centre().1,
            renderer.instance_count()
        );
        copy(
            &mut enc,
            &tex,
            &out_tex,
            model.pane_origin(Pane::Redraw).unwrap(),
            cam.size,
        );
    }

    let row = (window.0 * 4).next_multiple_of(256);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: row as u64 * window.1 as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    enc.copy_texture_to_buffer(
        out_tex.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(window.1),
            },
        },
        wgpu::Extent3d {
            width: window.0,
            height: window.1,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([enc.finish()]);
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("a mappable buffer"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the GPU");
    let data = slice.get_mapped_range().expect("a mapped range");
    let mut rgba = Vec::new();
    for y in 0..window.1 as usize {
        rgba.extend_from_slice(&data[y * row as usize..][..window.0 as usize * 4]);
    }
    drop(data);
    image::save_buffer(
        &args.out,
        &rgba,
        window.0,
        window.1,
        image::ColorType::Rgba8,
    )
    .expect("a PNG");
    println!("wrote {}", args.out.display());
}

fn copy(
    enc: &mut wgpu::CommandEncoder,
    from: &wgpu::Texture,
    to: &wgpu::Texture,
    at: (u32, u32),
    size: (u32, u32),
) {
    enc.copy_texture_to_texture(
        from.as_image_copy(),
        wgpu::TexelCopyTextureInfo {
            origin: wgpu::Origin3d {
                x: at.0,
                y: at.1,
                z: 0,
            },
            ..to.as_image_copy()
        },
        wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
    );
}
