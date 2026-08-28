/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![no_std]
#![allow(static_mut_refs)]
#![feature(alloc_error_handler)]

extern crate alloc;

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::NonNull;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Sense, Vec2};
use epoca_egui_gpu::{EguiRenderer, PaintStats, Surface as EguiSurface};
use epoca_gpu::wire::{
    self, GpuAddressMode, GpuBindingKind, GpuBlendFactor, GpuBlendOperation, GpuCompareFunction,
    GpuCullMode, GpuFrontFace, GpuIndexFormat, GpuPrimitiveTopology, GpuSamplerBindingType,
    GpuTextureAspect, GpuTextureFormat, GpuTextureSampleType, GpuTextureViewDimension,
    GpuVertexFormat, GpuVertexStepMode,
};
use epoca_gpu::{
    parse_capabilities, BatchEncoder, BindGroupEntry, BindGroupLayoutEntry, ColorTarget,
    RenderPipelineDescriptor, SamplerDescriptor, VertexAttribute, VertexBufferLayout,
};
use epoca_mesh::parse as parse_mesh;
use linked_list_allocator::Heap;

const INITIAL_HEAP_BYTES: usize = 64 * 1024;
const HEAP_GROWTH_BYTES: usize = 64 * 1024;
const MAX_HEAP_BYTES: usize = 16 * 1024 * 1024;
const CAPABILITIES_BYTES: usize = 512;
const INITIAL_EVENT_BYTES: usize = 1024;
const INPUT_EVENT_BYTES: usize = 8;
const INPUT_BATCH_BYTES: usize = INPUT_EVENT_BYTES * 64;
const EGUI_BASE_SLOT: u32 = 128;
const RESIZE_SETTLE_MS: u64 = 100;

const VERTEX_BUFFER: u32 = handle(1);
const INDEX_BUFFER: u32 = handle(2);
const UNIFORM_BUFFER: u32 = handle(3);
const COLOR_TEXTURE: u32 = handle(4);
const COLOR_TEXTURE_VIEW: u32 = handle(5);
const SAMPLER: u32 = handle(6);
const SHADER: u32 = handle(7);
const BIND_GROUP_LAYOUT: u32 = handle(8);
const PIPELINE_LAYOUT: u32 = handle(9);
const BIND_GROUP: u32 = handle(10);
const DEPTH_TEXTURE: u32 = handle(11);
const DEPTH_TEXTURE_VIEW: u32 = handle(12);
const PIPELINE: u32 = handle(13);

const MODEL_ASSET: &[u8] = b"showcase.epm";
const MAX_MODEL_BYTES: usize = 2 * 1024 * 1024;
const VERTEX_STRIDE: u64 = epoca_mesh::VERTEX_STRIDE as u64;
const UNIFORM_BYTES: u64 = 144;

const SHADER_SOURCE: &str = r#"
struct Uniforms {
    mvp: mat4x4<f32>,
    model: mat4x4<f32>,
    tint: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var color_texture: texture_2d<f32>;
@group(0) @binding(2) var color_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = uniforms.mvp * vec4<f32>(input.position, 1.0);
    output.normal = normalize((uniforms.model * vec4<f32>(input.normal, 0.0)).xyz);
    output.uv = input.uv;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let light = 0.28 + 0.72 * max(dot(normalize(input.normal), normalize(vec3<f32>(0.4, 0.8, 0.6))), 0.0);
    let surface = textureSample(color_texture, color_sampler, input.uv) * uniforms.tint;
    return vec4<f32>(surface.rgb * light, surface.a);
}
"#;

const CHECKERBOARD: [u8; 16] = [
    0xf0, 0x58, 0x48, 0xff, 0x38, 0xc8, 0xe8, 0xff, 0x38, 0xc8, 0xe8, 0xff, 0xf0, 0x58, 0x48, 0xff,
];

const fn handle(slot: u32) -> u32 {
    (1 << wire::GPU_HANDLE_SLOT_BITS) | slot
}

fn next_handle_generation(value: u32) -> Option<u32> {
    let generation = value >> wire::GPU_HANDLE_SLOT_BITS;
    generation
        .checked_add(1)
        .filter(|next| *next < (1 << (32 - wire::GPU_HANDLE_SLOT_BITS)))
        .map(|next| {
            (next << wire::GPU_HANDLE_SLOT_BITS) | (value & ((1 << wire::GPU_HANDLE_SLOT_BITS) - 1))
        })
}

polkavm_derive::min_stack_size!(1024 * 1024);

#[global_allocator]
static ALLOCATOR: GrowableHeap = GrowableHeap::empty();

struct HeapState {
    heap: Heap,
    reserved: usize,
}

struct GrowableHeap {
    state: UnsafeCell<HeapState>,
}

unsafe impl Sync for GrowableHeap {}

impl GrowableHeap {
    const fn empty() -> Self {
        Self {
            state: UnsafeCell::new(HeapState {
                heap: Heap::empty(),
                reserved: 0,
            }),
        }
    }

    unsafe fn initialize(&self) -> bool {
        let start = polkavm_derive::sbrk(0);
        if polkavm_derive::sbrk(INITIAL_HEAP_BYTES).is_null() {
            return false;
        }
        let state = &mut *self.state.get();
        state.heap.init(start, INITIAL_HEAP_BYTES);
        state.reserved = INITIAL_HEAP_BYTES;
        true
    }

    unsafe fn grow(&self, layout: Layout) -> bool {
        let Some(required) = layout.size().checked_add(layout.align()) else {
            return false;
        };
        let minimum = required.max(HEAP_GROWTH_BYTES);
        let Some(growth) = minimum
            .checked_add(HEAP_GROWTH_BYTES - 1)
            .map(|value| value / HEAP_GROWTH_BYTES * HEAP_GROWTH_BYTES)
        else {
            return false;
        };
        let state = &mut *self.state.get();
        if state.reserved.saturating_add(growth) > MAX_HEAP_BYTES
            || polkavm_derive::sbrk(growth).is_null()
        {
            return false;
        }
        state.heap.extend(growth);
        state.reserved += growth;
        true
    }
}

unsafe impl GlobalAlloc for GrowableHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return layout.align() as *mut u8;
        }
        if let Ok(allocation) = (&mut *self.state.get()).heap.allocate_first_fit(layout) {
            return allocation.as_ptr();
        }
        if !self.grow(layout) {
            return core::ptr::null_mut();
        }
        (&mut *self.state.get())
            .heap
            .allocate_first_fit(layout)
            .map_or(core::ptr::null_mut(), NonNull::as_ptr)
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if layout.size() != 0 {
            (&mut *self.state.get())
                .heap
                .deallocate(NonNull::new_unchecked(pointer), layout);
        }
    }
}

#[polkavm_derive::polkavm_import]
extern "C" {
    fn host_poll_input(pointer: u32, capacity: u32) -> u32;
    fn host_time_ms() -> u64;
    fn host_log(pointer: u32, length: u32);
    fn host_asset_read(
        name_pointer: u32,
        name_length: u32,
        offset: u32,
        output_pointer: u32,
        capacity: u32,
    ) -> u32;
}

#[derive(Clone, Copy)]
struct Surface {
    format: u16,
    physical_width: u32,
    physical_height: u32,
    logical_width: u32,
    logical_height: u32,
    scale_factor: f32,
    generation: u32,
}

struct Model {
    bytes: Vec<u8>,
    vertex_bytes: usize,
    vertex_count: u32,
    index_count: u32,
    base_color: [f32; 4],
    center: [f32; 3],
    fit_scale: f32,
}

impl Model {
    fn vertices(&self) -> &[u8] {
        &self.bytes[epoca_mesh::HEADER_BYTES..epoca_mesh::HEADER_BYTES + self.vertex_bytes]
    }

    fn indices(&self) -> &[u8] {
        &self.bytes[epoca_mesh::HEADER_BYTES + self.vertex_bytes..]
    }
}

struct SceneLab {
    surface: Surface,
    next_sequence: u64,
    frame: u32,
    resources_ready: bool,
    submission_pending: bool,
    configured_generation: u32,
    resize_not_before_ms: u64,
    depth_texture: u32,
    depth_texture_view: u32,
    event_bytes: Vec<u8>,
    input: InputState,
    context: egui::Context,
    ui_renderer: EguiRenderer,
    last_paint: PaintStats,
    paused: bool,
    speed: f32,
    yaw: f32,
    pitch: f32,
    distance: f32,
    tint: [f32; 3],
    model: Model,
}

impl SceneLab {
    fn new(surface: Surface) -> Self {
        let model = load_model();
        Self {
            surface,
            next_sequence: 1,
            frame: 0,
            resources_ready: false,
            submission_pending: false,
            configured_generation: 0,
            resize_not_before_ms: 0,
            depth_texture: DEPTH_TEXTURE,
            depth_texture_view: DEPTH_TEXTURE_VIEW,
            event_bytes: vec![0; INITIAL_EVENT_BYTES],
            input: InputState::new(),
            context: egui::Context::default(),
            ui_renderer: EguiRenderer::new(EGUI_BASE_SLOT)
                .unwrap_or_else(|_| fatal(b"scene-lab: invalid egui handle range")),
            last_paint: PaintStats::default(),
            paused: false,
            speed: 1.0,
            yaw: 0.0,
            pitch: -0.28,
            distance: 4.5,
            tint: [1.0, 1.0, 1.0],
            model,
        }
    }

    fn update(&mut self) {
        self.poll_events();
        if self.submission_pending {
            return;
        }
        let surface = read_surface();
        if surface.format != self.surface.format {
            fatal(b"scene-lab: surface format changed");
        }
        let surface_changed = surface.generation != self.surface.generation;
        self.surface = surface;
        if !self.resources_ready {
            match self.submit_resources() {
                0 => {
                    self.resources_ready = true;
                    self.submission_pending = true;
                    self.configured_generation = self.surface.generation;
                    self.next_sequence += 1;
                    log(b"scene-lab: resources submitted");
                }
                1 => return,
                _ => fatal(b"scene-lab: resource submission rejected"),
            }
            return;
        }

        if self.configured_generation != self.surface.generation {
            let now = unsafe { host_time_ms() };
            if surface_changed {
                self.resize_not_before_ms = now.saturating_add(RESIZE_SETTLE_MS);
                return;
            }
            if now < self.resize_not_before_ms {
                return;
            }
            match self.submit_surface_resize() {
                0 => {
                    self.configured_generation = self.surface.generation;
                    self.submission_pending = true;
                    self.next_sequence += 1;
                }
                1 => {}
                _ => fatal(b"scene-lab: surface resize rejected"),
            }
            return;
        }

        let batch = self.render_batch();
        match epoca_gpu::pvm::submit(&batch) {
            0 => {
                self.next_sequence += 1;
                self.submission_pending = true;
                self.frame = self.frame.wrapping_add(1);
            }
            1 => {}
            _ => fatal(b"scene-lab: frame submission rejected"),
        }
    }

    fn poll_events(&mut self) {
        loop {
            let length = epoca_gpu::pvm::receive(&mut self.event_bytes);
            if length == 0 {
                return;
            }
            if length < 0 {
                let required = length.unsigned_abs() as usize;
                if required <= self.event_bytes.len() || required > wire::MAX_GPU_EVENT_BYTES {
                    fatal(b"scene-lab: invalid GPU event length");
                }
                self.event_bytes
                    .try_reserve_exact(required - self.event_bytes.len())
                    .unwrap_or_else(|_| fatal(b"scene-lab: GPU event allocation failed"));
                self.event_bytes.resize(required, 0);
                continue;
            }
            if length as usize > self.event_bytes.len() {
                fatal(b"scene-lab: oversized GPU event");
            }
            let bytes = &self.event_bytes[..length as usize];
            if bytes.len() < wire::GPU_EVENT_HEADER_BYTES
                || bytes[..4] != wire::GPU_EVENT_MAGIC
                || u16::from_le_bytes(bytes[4..6].try_into().unwrap()) != wire::GPU_WIRE_VERSION
                || u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize != bytes.len()
                || u32::from_le_bytes(bytes[12..16].try_into().unwrap()) != 0
            {
                fatal(b"scene-lab: malformed GPU event");
            }
            let event_type = u16::from_le_bytes(bytes[6..8].try_into().unwrap());
            match event_type {
                value if value == wire::GpuEventType::SubmissionComplete as u16 => {
                    self.submission_pending = false;
                }
                value if value == wire::GpuEventType::SurfaceChanged as u16 => {}
                value if value == wire::GpuEventType::BatchRejected as u16 => {
                    let payload = &bytes[wire::GPU_EVENT_HEADER_BYTES..];
                    let error_code = payload
                        .get(4..8)
                        .map(|value| u32::from_le_bytes(value.try_into().unwrap()))
                        .unwrap_or(0);
                    if error_code == wire::GPU_BATCH_ERROR_STALE_SURFACE {
                        self.submission_pending = false;
                    } else {
                        fatal_event(b"scene-lab: GPU batch rejected", bytes, 8, 16)
                    }
                }
                value if value == wire::GpuEventType::ShaderDiagnostic as u16 => {
                    let payload = &bytes[wire::GPU_EVENT_HEADER_BYTES..];
                    let severity = payload
                        .get(4..6)
                        .map(|value| u16::from_le_bytes(value.try_into().unwrap()))
                        .unwrap_or(1);
                    if severity == 1 {
                        fatal_event(b"scene-lab: shader compilation failed", bytes, 24, 32);
                    }
                    log_event_text(bytes, 24, 32);
                }
                value
                    if value == wire::GpuEventType::ResourceFailed as u16
                        || value == wire::GpuEventType::UncapturedError as u16 =>
                {
                    fatal_event(b"scene-lab: WebGPU resource failed", bytes, 4, 12)
                }
                value if value == wire::GpuEventType::DeviceLost as u16 => {
                    fatal_event(b"scene-lab: device lost", bytes, 4, 12)
                }
                _ => fatal(b"scene-lab: unknown GPU event"),
            }
        }
    }

    fn submit_resources(&mut self) -> i32 {
        let mut batch = BatchEncoder::new(self.next_sequence)
            .unwrap_or_else(|_| fatal(b"scene-lab: resource batch allocation failed"));
        let vertices = self.model.vertices();
        let indices = self.model.indices();
        batch
            .create_buffer(
                VERTEX_BUFFER,
                wire::GPU_BUFFER_USAGE_COPY_DST | wire::GPU_BUFFER_USAGE_VERTEX,
                vertices.len() as u64,
            )
            .and_then(|_| batch.write_buffer(VERTEX_BUFFER, 0, vertices))
            .and_then(|_| {
                batch.create_buffer(
                    INDEX_BUFFER,
                    wire::GPU_BUFFER_USAGE_COPY_DST | wire::GPU_BUFFER_USAGE_INDEX,
                    indices.len() as u64,
                )
            })
            .and_then(|_| batch.write_buffer(INDEX_BUFFER, 0, indices))
            .and_then(|_| {
                batch.create_buffer(
                    UNIFORM_BUFFER,
                    wire::GPU_BUFFER_USAGE_COPY_DST | wire::GPU_BUFFER_USAGE_UNIFORM,
                    UNIFORM_BYTES,
                )
            })
            .and_then(|_| {
                batch.create_texture_2d(
                    COLOR_TEXTURE,
                    2,
                    2,
                    1,
                    1,
                    GpuTextureFormat::Rgba8UnormSrgb as u16,
                    wire::GPU_TEXTURE_USAGE_COPY_DST | wire::GPU_TEXTURE_USAGE_TEXTURE_BINDING,
                )
            })
            .and_then(|_| batch.write_texture_2d(COLOR_TEXTURE, 0, 0, 0, 2, 2, 8, 2, &CHECKERBOARD))
            .and_then(|_| {
                batch.create_texture_view(
                    COLOR_TEXTURE_VIEW,
                    COLOR_TEXTURE,
                    GpuTextureFormat::Rgba8UnormSrgb as u16,
                    GpuTextureAspect::All as u8,
                    0,
                    1,
                    0,
                    1,
                )
            })
            .and_then(|_| {
                batch.create_sampler(
                    SAMPLER,
                    SamplerDescriptor {
                        address_u: GpuAddressMode::Repeat as u8,
                        address_v: GpuAddressMode::Repeat as u8,
                        address_w: GpuAddressMode::ClampToEdge as u8,
                        mag_filter: wire::GpuFilterMode::Linear as u8,
                        min_filter: wire::GpuFilterMode::Linear as u8,
                        mipmap_filter: wire::GpuFilterMode::Nearest as u8,
                        compare: 0,
                        max_anisotropy: 1,
                        lod_min: 0.0,
                        lod_max: 32.0,
                    },
                )
            })
            .and_then(|_| batch.create_shader_wgsl(SHADER, SHADER_SOURCE))
            .and_then(|_| batch.create_bind_group_layout(BIND_GROUP_LAYOUT, &binding_layout()))
            .and_then(|_| batch.create_pipeline_layout(PIPELINE_LAYOUT, &[BIND_GROUP_LAYOUT]))
            .and_then(|_| {
                batch.create_bind_group(BIND_GROUP, BIND_GROUP_LAYOUT, &binding_entries())
            })
            .and_then(|_| {
                self.encode_depth_resources(&mut batch, self.depth_texture, self.depth_texture_view)
            })
            .and_then(|_| create_pipeline(&mut batch, self.surface.format))
            .unwrap_or_else(|_| fatal(b"scene-lab: scene resource encoding failed"));
        self.ui_renderer
            .initialize(&mut batch, self.surface.format)
            .unwrap_or_else(|_| fatal(b"scene-lab: egui resource encoding failed"));
        epoca_gpu::pvm::submit(&batch)
    }

    fn encode_depth_resources(
        &self,
        batch: &mut BatchEncoder,
        texture: u32,
        view: u32,
    ) -> Result<(), epoca_gpu::EncodeError> {
        batch.create_texture_2d(
            texture,
            self.surface.physical_width,
            self.surface.physical_height,
            1,
            1,
            GpuTextureFormat::Depth24Plus as u16,
            wire::GPU_TEXTURE_USAGE_RENDER_ATTACHMENT,
        )?;
        batch.create_texture_view(
            view,
            texture,
            GpuTextureFormat::Depth24Plus as u16,
            GpuTextureAspect::DepthOnly as u8,
            0,
            1,
            0,
            1,
        )
    }

    fn submit_surface_resize(&mut self) -> i32 {
        let mut batch = BatchEncoder::new(self.next_sequence)
            .unwrap_or_else(|_| fatal(b"scene-lab: resize batch allocation failed"));
        let texture = next_handle_generation(self.depth_texture)
            .unwrap_or_else(|| fatal(b"scene-lab: depth texture generations exhausted"));
        let view = next_handle_generation(self.depth_texture_view)
            .unwrap_or_else(|| fatal(b"scene-lab: depth view generations exhausted"));
        batch
            .destroy_resource(self.depth_texture_view)
            .and_then(|_| batch.destroy_resource(self.depth_texture))
            .and_then(|_| self.encode_depth_resources(&mut batch, texture, view))
            .unwrap_or_else(|_| fatal(b"scene-lab: resize encoding failed"));
        let status = epoca_gpu::pvm::submit(&batch);
        if status == 0 {
            self.depth_texture = texture;
            self.depth_texture_view = view;
        }
        status
    }

    fn render_batch(&mut self) -> BatchEncoder {
        let raw_input = self.input.gather(self.surface);
        let context = self.context.clone();
        let mut paused = self.paused;
        let mut speed = self.speed;
        let mut yaw = self.yaw;
        let mut pitch = self.pitch;
        let mut distance = self.distance;
        let mut tint = self.tint;
        let frame = self.frame;
        let surface = self.surface;
        let last_paint = self.last_paint;
        let vertex_count = self.model.vertex_count;
        let index_count = self.model.index_count;

        let output = context.run(raw_input, |ctx| {
            egui::SidePanel::right("scene-controls")
                .default_width(280.0)
                .show(ctx, |ui| {
                    ui.heading("PVM Scene Lab");
                    ui.label("egui and 3D share one GPU command stream.");
                    ui.separator();
                    ui.checkbox(&mut paused, "Pause animation");
                    ui.add(egui::Slider::new(&mut speed, 0.0..=3.0).text("Rotation speed"));
                    ui.add(egui::Slider::new(&mut distance, 2.8..=8.0).text("Camera distance"));
                    ui.collapsing("Material tint", |ui| {
                        ui.add(egui::Slider::new(&mut tint[0], 0.25..=1.0).text("Red"));
                        ui.add(egui::Slider::new(&mut tint[1], 0.25..=1.0).text("Green"));
                        ui.add(egui::Slider::new(&mut tint[2], 0.25..=1.0).text("Blue"));
                    });
                    if ui.button("Reset camera and material").clicked() {
                        yaw = 0.0;
                        pitch = -0.28;
                        distance = 4.5;
                        tint = [1.0, 1.0, 1.0];
                    }
                    ui.separator();
                    ui.monospace(format!(
                        "Model: {vertex_count} vertices / {index_count} indices"
                    ));
                    ui.monospace(format!("Frame {frame}"));
                    ui.monospace(format!(
                        "{} x {} px @ {:.2}x",
                        surface.physical_width, surface.physical_height, surface.scale_factor
                    ));
                    ui.monospace(format!(
                        "UI: {} draws / {} vertices",
                        last_paint.draws, last_paint.vertices
                    ));
                });
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    let response = ui.allocate_rect(ui.max_rect(), Sense::drag());
                    if response.dragged() {
                        let delta = ctx.input(|input| input.pointer.delta());
                        yaw += delta.x * 0.01;
                        pitch = (pitch + delta.y * 0.01).clamp(-1.2, 1.2);
                    }
                    ui.label("Drag the scene to orbit");
                });
        });

        self.paused = paused;
        self.speed = speed;
        self.yaw = yaw;
        self.pitch = pitch;
        self.distance = distance;
        self.tint = tint;
        if !self.paused {
            self.yaw += 0.012 * self.speed;
        }

        let primitives = context.tessellate(output.shapes, output.pixels_per_point);
        let mut batch = BatchEncoder::new(self.next_sequence)
            .unwrap_or_else(|_| fatal(b"scene-lab: frame batch allocation failed"));
        let uniform = uniform_bytes(
            self.yaw,
            self.pitch,
            self.distance,
            self.surface.physical_width as f32 / self.surface.physical_height as f32,
            self.tint,
            self.model.base_color,
            self.model.center,
            self.model.fit_scale,
        );
        batch
            .write_buffer(UNIFORM_BUFFER, 0, &uniform)
            .and_then(|_| {
                batch.begin_render_pass(
                    0,
                    self.depth_texture_view,
                    self.surface.generation,
                    wire::GPU_RENDER_PASS_COLOR_STORE | wire::GPU_RENDER_PASS_DEPTH_STORE,
                    [0.025, 0.035, 0.07, 1.0],
                    1.0,
                )
            })
            .and_then(|_| batch.set_pipeline(PIPELINE))
            .and_then(|_| {
                batch.set_vertex_buffer(
                    0,
                    VERTEX_BUFFER,
                    0,
                    (self.model.vertex_count as u64) * VERTEX_STRIDE,
                )
            })
            .and_then(|_| {
                batch.set_index_buffer(
                    INDEX_BUFFER,
                    GpuIndexFormat::Uint32 as u32,
                    0,
                    (self.model.index_count as u64) * 4,
                )
            })
            .and_then(|_| batch.set_bind_group(0, BIND_GROUP, &[]))
            .and_then(|_| {
                batch.set_viewport(
                    0.0,
                    0.0,
                    self.surface.physical_width as f32,
                    self.surface.physical_height as f32,
                    0.0,
                    1.0,
                )
            })
            .and_then(|_| {
                batch.set_scissor_rect(
                    0,
                    0,
                    self.surface.physical_width,
                    self.surface.physical_height,
                )
            })
            .and_then(|_| batch.draw_indexed(self.model.index_count, 1, 0, 0, 0))
            .and_then(|_| batch.end_render_pass())
            .unwrap_or_else(|_| fatal(b"scene-lab: scene frame encoding failed"));
        self.last_paint = self
            .ui_renderer
            .paint(
                &mut batch,
                output.textures_delta,
                &primitives,
                EguiSurface {
                    format: self.surface.format,
                    physical_width: self.surface.physical_width,
                    physical_height: self.surface.physical_height,
                    logical_width: self.surface.logical_width,
                    logical_height: self.surface.logical_height,
                    pixels_per_point: self.surface.scale_factor,
                    generation: self.surface.generation,
                },
            )
            .unwrap_or_else(|_| fatal(b"scene-lab: egui frame encoding failed"));
        batch
    }
}

struct InputState {
    modifiers: Modifiers,
    pointer: Pos2,
}

impl InputState {
    const fn new() -> Self {
        Self {
            modifiers: Modifiers::NONE,
            pointer: Pos2::ZERO,
        }
    }

    fn gather(&mut self, surface: Surface) -> RawInput {
        let mut input = RawInput {
            screen_rect: Some(Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(surface.logical_width as f32, surface.logical_height as f32),
            )),
            time: Some(unsafe { host_time_ms() } as f64 / 1_000.0),
            predicted_dt: 1.0 / 60.0,
            modifiers: self.modifiers,
            ..Default::default()
        };
        let mut bytes = [0u8; INPUT_BATCH_BYTES];
        loop {
            let written =
                unsafe { host_poll_input(bytes.as_mut_ptr() as u32, bytes.len() as u32) as usize }
                    .min(bytes.len());
            for event in bytes[..written].chunks_exact(INPUT_EVENT_BYTES) {
                self.push_event(event, surface.scale_factor, &mut input.events);
            }
            if written < bytes.len() {
                break;
            }
        }
        input.modifiers = self.modifiers;
        input
    }

    fn push_event(&mut self, bytes: &[u8], scale: f32, events: &mut Vec<Event>) {
        let event_type = bytes[0];
        let code = bytes[1];
        let position = Pos2::new(
            u16::from_le_bytes([bytes[2], bytes[3]]) as f32 / scale,
            u16::from_le_bytes([bytes[4], bytes[5]]) as f32 / scale,
        );
        match event_type {
            1 | 2 => {
                let pressed = event_type == 1;
                self.update_modifier(code, pressed);
                if let Some(key) = key_for_hid(code) {
                    events.push(Event::Key {
                        key,
                        physical_key: Some(key),
                        pressed,
                        repeat: false,
                        modifiers: self.modifiers,
                    });
                }
            }
            3 | 4 => {
                self.pointer = position;
                if let Some(button) = pointer_button(code) {
                    events.push(Event::PointerButton {
                        pos: position,
                        button,
                        pressed: event_type == 3,
                        modifiers: self.modifiers,
                    });
                }
            }
            5 => {
                self.pointer = position;
                events.push(Event::PointerMoved(position));
            }
            _ => {}
        }
    }

    fn update_modifier(&mut self, code: u8, pressed: bool) {
        match code {
            0xe0 | 0xe4 => self.modifiers.ctrl = pressed,
            0xe1 | 0xe5 => self.modifiers.shift = pressed,
            0xe2 | 0xe6 => self.modifiers.alt = pressed,
            _ => {}
        }
    }
}

fn key_for_hid(code: u8) -> Option<Key> {
    Some(match code {
        0x04 => Key::A,
        0x06 => Key::C,
        0x09 => Key::F,
        0x0b => Key::H,
        0x16 => Key::S,
        0x19 => Key::V,
        0x1b => Key::X,
        0x1d => Key::Z,
        0x28 => Key::Enter,
        0x29 => Key::Escape,
        0x2a => Key::Backspace,
        0x2b => Key::Tab,
        0x2c => Key::Space,
        0x49 => Key::Insert,
        0x4a => Key::Home,
        0x4b => Key::PageUp,
        0x4c => Key::Delete,
        0x4d => Key::End,
        0x4e => Key::PageDown,
        0x4f => Key::ArrowRight,
        0x50 => Key::ArrowLeft,
        0x51 => Key::ArrowDown,
        0x52 => Key::ArrowUp,
        _ => return None,
    })
}

fn pointer_button(code: u8) -> Option<PointerButton> {
    Some(match code {
        1 => PointerButton::Primary,
        2 => PointerButton::Secondary,
        3 => PointerButton::Middle,
        _ => return None,
    })
}

fn read_surface() -> Surface {
    let mut bytes = [0u8; CAPABILITIES_BYTES];
    let length = epoca_gpu::pvm::capabilities(&mut bytes);
    if length <= 0 || length as usize > bytes.len() {
        fatal(b"scene-lab: capabilities unavailable");
    }
    let capabilities = parse_capabilities(&bytes[..length as usize])
        .unwrap_or_else(|_| fatal(b"scene-lab: invalid capabilities"));
    Surface {
        format: capabilities.surface_format,
        physical_width: capabilities.physical_width,
        physical_height: capabilities.physical_height,
        logical_width: capabilities.logical_width,
        logical_height: capabilities.logical_height,
        scale_factor: capabilities.scale_factor,
        generation: capabilities.surface_generation,
    }
}

fn binding_layout() -> [BindGroupLayoutEntry; 3] {
    [
        BindGroupLayoutEntry {
            binding: 0,
            visibility: wire::GPU_SHADER_STAGE_VERTEX | wire::GPU_SHADER_STAGE_FRAGMENT,
            kind: GpuBindingKind::UniformBuffer as u16,
            flags: 0,
            minimum_binding_size: UNIFORM_BYTES,
            parameter_0: 0,
            parameter_1: 0,
        },
        BindGroupLayoutEntry {
            binding: 1,
            visibility: wire::GPU_SHADER_STAGE_FRAGMENT,
            kind: GpuBindingKind::Texture as u16,
            flags: 0,
            minimum_binding_size: 0,
            parameter_0: GpuTextureSampleType::FloatFilterable as u32,
            parameter_1: GpuTextureViewDimension::D2 as u32,
        },
        BindGroupLayoutEntry {
            binding: 2,
            visibility: wire::GPU_SHADER_STAGE_FRAGMENT,
            kind: GpuBindingKind::Sampler as u16,
            flags: 0,
            minimum_binding_size: 0,
            parameter_0: GpuSamplerBindingType::Filtering as u32,
            parameter_1: 0,
        },
    ]
}

fn binding_entries() -> [BindGroupEntry; 3] {
    [
        BindGroupEntry {
            binding: 0,
            resource: UNIFORM_BUFFER,
            kind: GpuBindingKind::UniformBuffer as u16,
            offset: 0,
            size: UNIFORM_BYTES,
        },
        BindGroupEntry {
            binding: 1,
            resource: COLOR_TEXTURE_VIEW,
            kind: GpuBindingKind::Texture as u16,
            offset: 0,
            size: 0,
        },
        BindGroupEntry {
            binding: 2,
            resource: SAMPLER,
            kind: GpuBindingKind::Sampler as u16,
            offset: 0,
            size: 0,
        },
    ]
}

fn create_pipeline(
    batch: &mut BatchEncoder,
    surface_format: u16,
) -> Result<(), epoca_gpu::EncodeError> {
    const LAYOUTS: [VertexBufferLayout; 1] = [VertexBufferLayout {
        array_stride: VERTEX_STRIDE,
        step_mode: GpuVertexStepMode::Vertex as u8,
        first_attribute: 0,
        attribute_count: 3,
    }];
    const ATTRIBUTES: [VertexAttribute; 3] = [
        VertexAttribute {
            format: GpuVertexFormat::Float32x3 as u16,
            shader_location: 0,
            offset: 0,
        },
        VertexAttribute {
            format: GpuVertexFormat::Float32x3 as u16,
            shader_location: 1,
            offset: 12,
        },
        VertexAttribute {
            format: GpuVertexFormat::Float32x2 as u16,
            shader_location: 2,
            offset: 24,
        },
    ];
    let targets = [ColorTarget {
        format: surface_format,
        write_mask: wire::GPU_COLOR_WRITE_RED
            | wire::GPU_COLOR_WRITE_GREEN
            | wire::GPU_COLOR_WRITE_BLUE
            | wire::GPU_COLOR_WRITE_ALPHA,
        color_operation: GpuBlendOperation::Add as u8,
        color_source_factor: GpuBlendFactor::One as u8,
        color_destination_factor: GpuBlendFactor::Zero as u8,
        alpha_operation: GpuBlendOperation::Add as u8,
        alpha_source_factor: GpuBlendFactor::One as u8,
        alpha_destination_factor: GpuBlendFactor::Zero as u8,
    }];
    batch.create_render_pipeline(RenderPipelineDescriptor {
        id: PIPELINE,
        layout: PIPELINE_LAYOUT,
        shader: SHADER,
        flags: wire::GPU_PIPELINE_DEPTH_WRITE,
        depth_format: GpuTextureFormat::Depth24Plus as u16,
        sample_count: 1,
        topology: GpuPrimitiveTopology::TriangleList as u8,
        front_face: GpuFrontFace::Ccw as u8,
        cull_mode: GpuCullMode::Back as u8,
        strip_index_format: 0,
        depth_compare: GpuCompareFunction::Less as u8,
        vertex_layouts: &LAYOUTS,
        vertex_attributes: &ATTRIBUTES,
        color_targets: &targets,
    })
}

fn load_model() -> Model {
    let bytes = read_asset(MODEL_ASSET, MAX_MODEL_BYTES);
    let mesh = parse_mesh(&bytes).unwrap_or_else(|_| fatal(b"scene-lab: invalid showcase.epm"));
    let center = [
        (mesh.bounds_min[0] + mesh.bounds_max[0]) * 0.5,
        (mesh.bounds_min[1] + mesh.bounds_max[1]) * 0.5,
        (mesh.bounds_min[2] + mesh.bounds_max[2]) * 0.5,
    ];
    let maximum_extent = (mesh.bounds_max[0] - mesh.bounds_min[0])
        .max(mesh.bounds_max[1] - mesh.bounds_min[1])
        .max(mesh.bounds_max[2] - mesh.bounds_min[2]);
    if !maximum_extent.is_finite() || maximum_extent <= f32::EPSILON {
        fatal(b"scene-lab: showcase.epm has empty bounds");
    }
    let vertex_bytes = mesh.vertices.len();
    let vertex_count = mesh.vertex_count;
    let index_count = mesh.index_count;
    let base_color = mesh.base_color;
    Model {
        bytes,
        vertex_bytes,
        vertex_count,
        index_count,
        base_color,
        center,
        fit_scale: 2.0 / maximum_extent,
    }
}

fn read_asset(name: &[u8], maximum_bytes: usize) -> Vec<u8> {
    const CHUNK_BYTES: usize = 64 * 1024;
    let mut bytes = Vec::new();
    loop {
        let remaining = maximum_bytes.saturating_sub(bytes.len());
        if remaining == 0 {
            let mut probe = [0u8; 1];
            if unsafe {
                host_asset_read(
                    name.as_ptr() as u32,
                    name.len() as u32,
                    bytes.len() as u32,
                    probe.as_mut_ptr() as u32,
                    1,
                )
            } != 0
            {
                fatal(b"scene-lab: showcase.epm exceeds asset limit");
            }
            break;
        }
        let requested = remaining.min(CHUNK_BYTES);
        let offset = bytes.len();
        bytes
            .try_reserve_exact(requested)
            .unwrap_or_else(|_| fatal(b"scene-lab: model asset allocation failed"));
        bytes.resize(offset + requested, 0);
        let read = unsafe {
            host_asset_read(
                name.as_ptr() as u32,
                name.len() as u32,
                offset as u32,
                bytes[offset..].as_mut_ptr() as u32,
                requested as u32,
            )
        } as usize;
        if read > requested {
            fatal(b"scene-lab: invalid model asset read");
        }
        bytes.truncate(offset + read);
        if read < requested {
            break;
        }
    }
    if bytes.is_empty() {
        fatal(b"scene-lab: showcase.epm is missing");
    }
    bytes
}

#[allow(clippy::too_many_arguments)]
fn uniform_bytes(
    yaw: f32,
    pitch: f32,
    distance: f32,
    aspect: f32,
    tint: [f32; 3],
    base_color: [f32; 4],
    center: [f32; 3],
    fit_scale: f32,
) -> [u8; UNIFORM_BYTES as usize] {
    let (sin_y, cos_y) = (libm::sinf(yaw), libm::cosf(yaw));
    let (sin_x, cos_x) = (libm::sinf(pitch), libm::cosf(pitch));
    let rotation_y = [
        cos_y, 0.0, -sin_y, 0.0, 0.0, 1.0, 0.0, 0.0, sin_y, 0.0, cos_y, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let rotation_x = [
        1.0, 0.0, 0.0, 0.0, 0.0, cos_x, sin_x, 0.0, 0.0, -sin_x, cos_x, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let scale = [
        fit_scale, 0.0, 0.0, 0.0, 0.0, fit_scale, 0.0, 0.0, 0.0, 0.0, fit_scale, 0.0, 0.0, 0.0,
        0.0, 1.0,
    ];
    let translation = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -center[0], -center[1],
        -center[2], 1.0,
    ];
    let model = multiply(
        rotation_y,
        multiply(rotation_x, multiply(scale, translation)),
    );
    let view = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -distance, 1.0,
    ];
    let near = 0.1;
    let far = 100.0;
    let f = 1.732_050_8;
    let projection = [
        f / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        f,
        0.0,
        0.0,
        0.0,
        0.0,
        far / (near - far),
        -1.0,
        0.0,
        0.0,
        (near * far) / (near - far),
        0.0,
    ];
    let mvp = multiply(projection, multiply(view, model));
    let mut bytes = [0u8; UNIFORM_BYTES as usize];
    for (offset, matrix) in [(0, mvp), (64, model)] {
        for (index, value) in matrix.into_iter().enumerate() {
            bytes[offset + index * 4..offset + index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    let material = [
        tint[0] * base_color[0],
        tint[1] * base_color[1],
        tint[2] * base_color[2],
        base_color[3],
    ];
    for (index, value) in material.into_iter().enumerate() {
        bytes[128 + index * 4..132 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn multiply(left: [f32; 16], right: [f32; 16]) -> [f32; 16] {
    let mut output = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            output[column * 4 + row] = (0..4)
                .map(|index| left[index * 4 + row] * right[column * 4 + index])
                .sum();
        }
    }
    output
}

static mut APP: Option<SceneLab> = None;

#[polkavm_derive::polkavm_export]
extern "C" fn init() {
    if !unsafe { ALLOCATOR.initialize() } {
        fatal(b"scene-lab: heap reservation failed");
    }
    log(b"scene-lab: init");
}

#[polkavm_derive::polkavm_export]
extern "C" fn update() {
    unsafe {
        if APP.is_none() {
            APP = Some(SceneLab::new(read_surface()));
            log(b"scene-lab: capabilities ready");
        }
        APP.as_mut().unwrap().update();
    }
}

fn log(message: &[u8]) {
    unsafe { host_log(message.as_ptr() as u32, message.len() as u32) }
}
fn log_event_text(event: &[u8], length_offset: usize, text_offset: usize) {
    let Some(payload) = event.get(wire::GPU_EVENT_HEADER_BYTES..) else {
        return;
    };
    let Some(length) = payload
        .get(length_offset..length_offset + 4)
        .map(|value| u32::from_le_bytes(value.try_into().unwrap()) as usize)
    else {
        return;
    };
    let Some(end) = text_offset.checked_add(length) else {
        return;
    };
    if let Some(text) = payload.get(text_offset..end) {
        log(text);
    }
}

fn fatal_event(message: &[u8], event: &[u8], length_offset: usize, text_offset: usize) -> ! {
    log(message);
    log_event_text(event, length_offset, text_offset);
    trap()
}

fn fatal(message: &[u8]) -> ! {
    log(message);
    trap()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    fatal(b"scene-lab: panic")
}

#[alloc_error_handler]
fn allocation_error(_layout: Layout) -> ! {
    fatal(b"scene-lab: allocation failed")
}

fn trap() -> ! {
    unsafe { core::arch::asm!("unimp", options(noreturn)) }
}

#[no_mangle]
unsafe extern "C" fn memset(destination: *mut u8, value: i32, length: usize) -> *mut u8 {
    for index in 0..length {
        core::ptr::write_volatile(destination.add(index), value as u8);
    }
    destination
}

#[no_mangle]
unsafe extern "C" fn memcpy(destination: *mut u8, source: *const u8, length: usize) -> *mut u8 {
    for index in 0..length {
        let value = core::ptr::read_volatile(source.add(index));
        core::ptr::write_volatile(destination.add(index), value);
    }
    destination
}

#[no_mangle]
unsafe extern "C" fn memmove(destination: *mut u8, source: *const u8, length: usize) -> *mut u8 {
    if (destination as usize) <= source as usize {
        for index in 0..length {
            let value = core::ptr::read_volatile(source.add(index));
            core::ptr::write_volatile(destination.add(index), value);
        }
    } else {
        for index in (0..length).rev() {
            let value = core::ptr::read_volatile(source.add(index));
            core::ptr::write_volatile(destination.add(index), value);
        }
    }
    destination
}

#[no_mangle]
unsafe extern "C" fn memcmp(left: *const u8, right: *const u8, length: usize) -> i32 {
    for index in 0..length {
        let left = core::ptr::read_volatile(left.add(index));
        let right = core::ptr::read_volatile(right.add(index));
        if left != right {
            return left as i32 - right as i32;
        }
    }
    0
}

#[no_mangle]
unsafe extern "C" fn __atomic_load_8(pointer: *const u64, _ordering: i32) -> u64 {
    core::ptr::read_volatile(pointer)
}

#[no_mangle]
unsafe extern "C" fn __atomic_store_8(pointer: *mut u64, value: u64, _ordering: i32) {
    core::ptr::write_volatile(pointer, value);
}
