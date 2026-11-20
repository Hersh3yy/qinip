//! The compositor: the GPU side of the two skins (Liquid Metal, Disco Ball).
//!
//! Rune lays out the screen and hands every frame here. The view (`view.rs`) places `El::effect`
//! slots: boxes that draw nothing themselves but keep their place and shape in the painter
//! order. This file turns each slot into one rune-fx layer and paints it exactly where the slot
//! sits, so a slot placed under a knob's pointer is painted under it, and a slot placed after a
//! bar's content is painted over it. The slot's mark (`El::effect(MARK)`) says which material,
//! its `effect_data` carries a few numbers (a border width, a tile's angle), and its presented
//! rect and corner radius are the shape.
//!
//! The pattern is Rune's `examples/showreel/src/gfx.rs`, trimmed to the six marks the skins
//! use. With the Flat skin the view places no slots, this draws nothing of its own, and the
//! window can rest between frames as it did before.
//!
//! Headless runs (`--snap`) go through here too; `summary` prints the GPU frame time after one
//! (two tiny timestamp passes bracket the frame), which is how the disco wall's cost is checked.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use rune::compose::{Compositor, Frame};
use rune::render::{EffectSite, Instance, KIND_EFFECT};
use rune_fx::{FillAnchor, Fx, FxFrame, FxLayer, FxOptions, FxRenderer, FxSlot, LiquidMetalParams, Shape, SparkleParams, rgb};

use crate::state::App;

// ---- Marks: which material a slot asks for. The `effect_data` layout is noted per mark. ------

/// Chrome liquid metal filling the slot (knob pots, transport buttons). Anchored to the element,
/// so every pot and button shows the same surface. data: [bevel pt (0 = 6), dome 0..1 (0 = 0.6)]
pub const CHROME_FILL: u32 = 1;
/// A chrome tube along the slot's edge (the waveform screen). data: [band width pt (0 = 3)]
pub const CHROME_BORDER: u32 = 2;
/// A gold tube along the slot's edge (the playing row's title). data: [band width pt (0 = 2)]
pub const GOLD_BORDER: u32 = 3;
/// Dark gunmetal filling the slot (the top and bottom bars). Anchored to the window, so the two
/// bars read as parts of one slab. data: [bevel pt (0 = 4)]
pub const GUNMETAL_FILL: u32 = 4;
/// One mirror tile of the disco wall: chrome with its own studio tilt and ripple seed.
/// data: [studio angle rad, seed]
pub const DISCO_TILE: u32 = 5;
/// Twinkling stars over a bar. data: [count (0 = 8), star size pt (0 = 6)]
pub const SPARKLES: u32 = 6;

/// `v` when set, else `default` (effect data is zero where the view did not care).
fn or(v: f32, default: f32) -> f32 {
    if v > 0.0 { v } else { default }
}

/// One rune-fx layer for a slot, or None for a mark this compositor does not know.
fn layer(mark: u32, d: [f32; 4], shape: Shape, t: f32) -> Option<FxLayer> {
    let fx = match mark {
        CHROME_FILL => Fx::fill(
            LiquidMetalParams { bevel: or(d[0], 6.0), dome: or(d[1], 0.6), scale: 60.0, ..LiquidMetalParams::chrome() }
                .anchor(FillAnchor::Element),
        ),
        CHROME_BORDER => Fx::fill(LiquidMetalParams::chrome().border(or(d[0], 3.0))),
        GOLD_BORDER => Fx::fill(LiquidMetalParams::gold().anchor(FillAnchor::Element).border(or(d[0], 2.0))),
        // The gunmetal preset with a dimmer studio, so the bars read as dark metal, not chrome.
        GUNMETAL_FILL => Fx::fill(LiquidMetalParams {
            tint: rgb(0x6b727b),
            light: rgb(0xc9d3e0),
            bevel: or(d[0], 4.0),
            dome: 0.25,
            scale: 260.0,
            flow: 0.45,
            speed: 0.2,
            ..LiquidMetalParams::gunmetal()
        }),
        DISCO_TILE => {
            // Every tile is the same small, nearly flat chrome mirror, tilted on its own
            // (`angle`, from the view's hash of its row and column), so each catches a
            // different part of the studio. The tilt wanders and the tile's brightness
            // breathes, each at its own phase, so glints travel across the wall. The surface
            // itself is still (`flow` 0): a still slab costs a quarter of a flowing one, and
            // there are a thousand of them.
            let phase = d[1] * 0.37;
            let glint = 0.62 + 0.38 * (t * 0.7 + phase).sin();
            let p = LiquidMetalParams {
                angle: d[0] + 0.5 * (t * 0.25 + phase).sin(),
                bevel: 2.0,
                dome: 0.3,
                scale: 48.0,
                flow: 0.0,
                speed: 0.0,
                contrast: 0.7,
                ..LiquidMetalParams::chrome()
            }
            .anchor(FillAnchor::Element);
            return Some(Fx::fill(p.intensity(glint)).on(shape).at(t).seed(d[1] as u32 + 1));
        }
        SPARKLES => Fx::sparkles(SparkleParams {
            count: or(d[0], 8.0) as u32,
            size: or(d[1], 6.0),
            margin: 2.0,
            color: rgb(0xfff1c4),
            ..Default::default()
        }),
        _ => return None,
    };
    Some(fx.on(shape).at(t))
}

/// A slot's `El::effect` mark, decoded from its record the way Rune packs it (two exact 16-bit
/// floats in `border`; `rune::render::effect_mark` does the same but is private).
fn effect_mark(i: &Instance) -> u32 {
    (i.border[0] as u32 & 0xFFFF) | (i.border[1] as u32 & 0xFFFF) << 16
}

// ---- The compositor ------------------------------------------------------------------------

#[derive(Default)]
pub struct Gfx {
    /// rune-fx's renderer, made on the first frame (it needs the device and target format).
    fx: Option<FxRenderer>,
    /// GPU frame timer, headless runs only (None when the device has no timestamp queries).
    timer: Option<Timer>,
    timer_tried: bool,
    /// Effect layers drawn in the last frame: 0 with the Flat skin, so the window can rest.
    drawn: usize,
    /// For the headless summary: frames drawn, most layers in one frame, GPU ms sum and count.
    frames: u32,
    max_layers: usize,
    gpu: (f64, u32),
}

impl Compositor<App> for Gfx {
    /// Timestamp queries for the headless frame time. Rune's own Metal backend offers them.
    fn features(&self) -> wgpu::Features {
        wgpu::Features::TIMESTAMP_QUERY
    }

    /// Liquid metal flows on its own, so keep the display link running while any slot is drawn.
    fn animating(&self) -> bool {
        self.drawn > 0
    }

    fn draws_own(&self) -> bool {
        self.drawn > 0
    }

    fn summary(&mut self, _device: &wgpu::Device) -> Option<String> {
        let gpu = if self.gpu.1 > 0 {
            format!("gpu frame mean {:.3} ms over {} timed frames", self.gpu.0 / f64::from(self.gpu.1), self.gpu.1)
        } else {
            "no GPU timestamps".to_owned()
        };
        Some(format!("qinip-rune gfx: {} frames, at most {} effect layers in a frame, {gpu}", self.frames, self.max_layers))
    }

    fn render(&mut self, f: &mut Frame<'_, App>) {
        let (device, queue, size) = (f.device, f.queue, f.size);
        let t = f.rt.t as f32;

        // One rune-fx layer per effect slot in the painter stream. `at` keeps each layer's slot
        // index (ascending, as the stream is walked) so the draw hook can find its layer.
        let mut at: Vec<u32> = Vec::new();
        let mut layers: Vec<FxLayer> = Vec::new();
        let mut slots: Vec<FxSlot> = Vec::new();
        for (n, i) in f.rt.instances.iter().enumerate() {
            if i.params[1] != KIND_EFFECT {
                continue;
            }
            // The record is in device pixels; `k` is device pixels per point at the slot.
            let k = i.params[3];
            let [x, y, w, h] = i.rect.map(|v| v / k);
            let r = i.radii.iter().fold(0.0f32, |a, v| a.max(v.abs())) / k;
            let Some(l) = layer(effect_mark(i), i.uv, Shape::rounded(x, y, w, h, r), t) else { continue };
            at.push(n as u32);
            layers.push(l);
            // The slot's clip and soft edges (a scrolled table row), so the effect is cut like
            // the primitives around it.
            slots.push(FxSlot::default().scale(k).clip(i.clip, i.fade));
        }
        self.drawn = layers.len();
        self.max_layers = self.max_layers.max(layers.len());
        self.frames += 1;

        let fx = self.fx.get_or_insert_with(|| FxRenderer::new(device, f.format, FxOptions { dither: true }));
        fx.set_srgb_blend(f.srgb_blend());
        // A slot inside an isolated layer (a fading row) is drawn into a float target.
        fx.add_format(device, wgpu::TextureFormat::Rgba16Float);
        if !layers.is_empty() {
            let frame = FxFrame { viewport: [size.0, size.1], scale: f.rt.env.scale, layers: &layers, backdrop: None, sources: &[] };
            fx.prepare_with_slots(device, queue, &frame, &slots);
        }

        // The frame timer (headless only): a finished reading comes back a few frames later.
        if !self.timer_tried {
            self.timer_tried = true;
            if rune::tooling::headless_run().is_some() {
                self.timer = Timer::new(device, queue);
            }
        }
        let _ = device.poll(wgpu::PollType::Poll);
        let gpu_ms = self.timer.as_mut().and_then(|tm| tm.begin(f.encoder));
        if let Some(tm) = &self.timer
            && tm.recording
        {
            marker_pass(f.encoder, f.target, &tm.qs, Some(0), None);
        }

        // Rune's own frame, stopping at every slot so its layer is painted right there.
        let clear = f.rt.clear_color();
        f.ui.prepare(device, queue, &mut f.rt.text.atlas, &f.rt.instances, size);
        f.ui.draw_with(f.encoder, f.target, clear, &mut |pass, s: &EffectSite<'_>| {
            if let Ok(k) = at.binary_search(&s.index) {
                fx.draw_layer_in(pass, s.format, s.origin, k);
            }
        });

        if let Some(tm) = self.timer.as_mut()
            && tm.recording
        {
            marker_pass(f.encoder, f.target, &tm.qs, None, Some(1));
            tm.end();
        }
        if let Some(g) = gpu_ms {
            self.gpu.0 += f64::from(g);
            self.gpu.1 += 1;
        }
    }
}

// ---- GPU frame timer (the pattern in Rune's examples/cursed-settings/src/gfx.rs) -----------

/// Whole-frame GPU time from two timestamps, read back a couple of frames later without
/// stalling. Rune owns the frame's render pass, so the stamps go on two empty passes over the
/// same target placed before and after it; sharing the target keeps Metal from reordering them.
struct Timer {
    qs: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    /// 0 idle, 4 written, 1 copy recorded, 2 mapping, 3 mapped.
    state: Arc<AtomicU8>,
    period: f32,
    recording: bool,
}

impl Timer {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Timer> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let qs = device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("qinip ts"), ty: wgpu::QueryType::Timestamp, count: 2 });
        let buf = |label, usage| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: 16, usage, mapped_at_creation: false });
        let resolve = buf("qinip ts resolve", wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC);
        let read = buf("qinip ts read", wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ);
        Some(Timer { qs, resolve, read, state: Arc::new(AtomicU8::new(0)), period: queue.get_timestamp_period(), recording: false })
    }

    /// Collects a finished reading (ms) and decides whether this frame records one. Last
    /// frame's queries are resolved here, a command buffer later, so the end stamp has landed.
    fn begin(&mut self, enc: &mut wgpu::CommandEncoder) -> Option<f32> {
        let mut out = None;
        match self.state.load(Ordering::Acquire) {
            4 => {
                enc.resolve_query_set(&self.qs, 0..2, &self.resolve, 0);
                enc.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, 16);
                self.state.store(1, Ordering::Release);
            }
            1 => {
                self.state.store(2, Ordering::Release);
                let st = self.state.clone();
                self.read.slice(..).map_async(wgpu::MapMode::Read, move |r| st.store(if r.is_ok() { 3 } else { 0 }, Ordering::Release));
            }
            3 => {
                if let Ok(data) = self.read.slice(..).get_mapped_range() {
                    let stamp = |at: usize| u64::from_le_bytes(data[at..at + 8].try_into().unwrap_or([0; 8]));
                    let (t0, t1) = (stamp(0), stamp(8));
                    out = (t1 > t0).then(|| (t1 - t0) as f32 * self.period / 1e6);
                }
                self.read.unmap();
                self.state.store(0, Ordering::Release);
            }
            _ => {}
        }
        self.recording = self.state.load(Ordering::Acquire) == 0;
        out
    }

    fn end(&mut self) {
        if self.recording {
            self.state.store(4, Ordering::Release);
        }
    }
}

/// An empty render pass over `view` that only writes a timestamp at its start or end.
fn marker_pass(enc: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, qs: &wgpu::QuerySet, begin: Option<u32>, end: Option<u32>) {
    let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("qinip gpu timer"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: Some(wgpu::RenderPassTimestampWrites { query_set: qs, beginning_of_pass_write_index: begin, end_of_pass_write_index: end }),
        occlusion_query_set: None,
        multiview_mask: None,
    });
}
