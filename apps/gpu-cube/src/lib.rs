/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![no_std]
#![allow(static_mut_refs)]
#![feature(alloc_error_handler)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::NonNull;
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
use linked_list_allocator::Heap;

const INITIAL_HEAP_BYTES: usize = 64 * 1024;
const HEAP_GROWTH_BYTES: usize = 64 * 1024;
const MAX_HEAP_BYTES: usize = 8 * 1024 * 1024;
const CAPABILITIES_BYTES: usize = 512;
const INITIAL_EVENT_BYTES: usize = 1024;

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

const VERTEX_STRIDE: u64 = 20;
const VERTEX_COUNT: usize = 24;
const INDEX_COUNT: u32 = 36;
const UNIFORM_BYTES: u64 = 64;

const SHADER_SOURCE: &str = r#"
struct Uniforms {
    mvp: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var color_texture: texture_2d<f32>;
@group(0) @binding(2) var color_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = uniforms.mvp * vec4<f32>(input.position, 1.0);
    output.uv = input.uv;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(color_texture, color_sampler, input.uv);
}
"#;

const VERTICES: [[f32; 5]; VERTEX_COUNT] = [
    [-1.0, -1.0, 1.0, 0.0, 1.0],
    [1.0, -1.0, 1.0, 1.0, 1.0],
    [1.0, 1.0, 1.0, 1.0, 0.0],
    [-1.0, 1.0, 1.0, 0.0, 0.0],
    [1.0, -1.0, -1.0, 0.0, 1.0],
    [-1.0, -1.0, -1.0, 1.0, 1.0],
    [-1.0, 1.0, -1.0, 1.0, 0.0],
    [1.0, 1.0, -1.0, 0.0, 0.0],
    [1.0, -1.0, 1.0, 0.0, 1.0],
    [1.0, -1.0, -1.0, 1.0, 1.0],
    [1.0, 1.0, -1.0, 1.0, 0.0],
    [1.0, 1.0, 1.0, 0.0, 0.0],
    [-1.0, -1.0, -1.0, 0.0, 1.0],
    [-1.0, -1.0, 1.0, 1.0, 1.0],
    [-1.0, 1.0, 1.0, 1.0, 0.0],
    [-1.0, 1.0, -1.0, 0.0, 0.0],
    [-1.0, 1.0, 1.0, 0.0, 1.0],
    [1.0, 1.0, 1.0, 1.0, 1.0],
    [1.0, 1.0, -1.0, 1.0, 0.0],
    [-1.0, 1.0, -1.0, 0.0, 0.0],
    [-1.0, -1.0, -1.0, 0.0, 1.0],
    [1.0, -1.0, -1.0, 1.0, 1.0],
    [1.0, -1.0, 1.0, 1.0, 0.0],
    [-1.0, -1.0, 1.0, 0.0, 0.0],
];

const INDICES: [u16; INDEX_COUNT as usize] = [
    0, 1, 2, 2, 3, 0, 4, 5, 6, 6, 7, 4, 8, 9, 10, 10, 11, 8, 12, 13, 14, 14, 15, 12, 16, 17, 18,
    18, 19, 16, 20, 21, 22, 22, 23, 20,
];

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
    fn host_log(pointer: u32, length: u32);
}

struct Surface {
    format: u16,
    width: u32,
    height: u32,
    generation: u32,
}

struct CubeApp {
    surface: Surface,
    next_sequence: u64,
    frame: u32,
    resources_ready: bool,
    submission_pending: bool,
    configured_generation: u32,
    depth_texture: u32,
    depth_texture_view: u32,
    event_bytes: Vec<u8>,
}

impl CubeApp {
    fn new(surface: Surface) -> Self {
        Self {
            surface,
            next_sequence: 1,
            frame: 0,
            resources_ready: false,
            submission_pending: false,
            configured_generation: 0,
            depth_texture: DEPTH_TEXTURE,
            depth_texture_view: DEPTH_TEXTURE_VIEW,
            event_bytes: vec![0; INITIAL_EVENT_BYTES],
        }
    }

    fn update(&mut self) {
        self.poll_events();
        if self.submission_pending {
            return;
        }
        let surface = read_surface();
        if surface.format != self.surface.format {
            fatal(b"gpu-cube: surface format changed");
        }
        self.surface = surface;
        if !self.resources_ready {
            match self.submit_resources() {
                0 => {
                    self.resources_ready = true;
                    self.submission_pending = true;
                    self.configured_generation = self.surface.generation;
                    self.next_sequence += 1;
                    log(b"gpu-cube: resources submitted");
                }
                1 => return,
                _ => fatal(b"gpu-cube: resource submission rejected"),
            }
            return;
        }
        if self.configured_generation != self.surface.generation {
            match self.submit_surface_resize() {
                0 => {
                    self.configured_generation = self.surface.generation;
                    self.submission_pending = true;
                    self.next_sequence += 1;
                }
                1 => {}
                _ => fatal(b"gpu-cube: surface resize rejected"),
            }
            return;
        }
        let batch = self
            .render_batch()
            .unwrap_or_else(|_| fatal(b"gpu-cube: frame encoding failed"));
        match epoca_gpu::pvm::submit(&batch) {
            0 => {
                self.submission_pending = true;
                self.next_sequence += 1;
                self.frame = self.frame.wrapping_add(1);
            }
            1 => {}
            _ => fatal(b"gpu-cube: frame submission rejected"),
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
                    fatal(b"gpu-cube: invalid GPU event length");
                }
                self.event_bytes
                    .try_reserve_exact(required - self.event_bytes.len())
                    .unwrap_or_else(|_| fatal(b"gpu-cube: GPU event allocation failed"));
                self.event_bytes.resize(required, 0);
                continue;
            }
            if length as usize > self.event_bytes.len() {
                fatal(b"gpu-cube: oversized GPU event");
            }
            let bytes = &self.event_bytes[..length as usize];
            if bytes.len() < wire::GPU_EVENT_HEADER_BYTES
                || bytes[..4] != wire::GPU_EVENT_MAGIC
                || u16::from_le_bytes(bytes[4..6].try_into().unwrap()) != wire::GPU_WIRE_VERSION
                || u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize != bytes.len()
                || u32::from_le_bytes(bytes[12..16].try_into().unwrap()) != 0
            {
                fatal(b"gpu-cube: malformed GPU event");
            }
            let event_type = u16::from_le_bytes(bytes[6..8].try_into().unwrap());
            if event_type == wire::GpuEventType::SubmissionComplete as u16 {
                self.submission_pending = false;
                continue;
            }
            match event_type {
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
                        fatal(b"gpu-cube: GPU batch rejected")
                    }
                }
                value if value == wire::GpuEventType::DeviceLost as u16 => {
                    fatal(b"gpu-cube: device lost")
                }
                _ => fatal(b"gpu-cube: GPU validation failed"),
            }
        }
    }

    fn submit_resources(&self) -> i32 {
        let mut batch = BatchEncoder::new(self.next_sequence)
            .unwrap_or_else(|_| fatal(b"gpu-cube: resource batch allocation failed"));
        let vertices = vertex_bytes();
        let indices = index_bytes();
        batch
            .create_buffer(
                VERTEX_BUFFER,
                wire::GPU_BUFFER_USAGE_COPY_DST | wire::GPU_BUFFER_USAGE_VERTEX,
                vertices.len() as u64,
            )
            .and_then(|_| batch.write_buffer(VERTEX_BUFFER, 0, &vertices))
            .and_then(|_| {
                batch.create_buffer(
                    INDEX_BUFFER,
                    wire::GPU_BUFFER_USAGE_COPY_DST | wire::GPU_BUFFER_USAGE_INDEX,
                    indices.len() as u64,
                )
            })
            .and_then(|_| batch.write_buffer(INDEX_BUFFER, 0, &indices))
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
            .unwrap_or_else(|_| fatal(b"gpu-cube: resource encoding failed"));
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
            self.surface.width,
            self.surface.height,
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
            .unwrap_or_else(|_| fatal(b"gpu-cube: resize batch allocation failed"));
        let texture = next_handle_generation(self.depth_texture)
            .unwrap_or_else(|| fatal(b"gpu-cube: depth texture generations exhausted"));
        let view = next_handle_generation(self.depth_texture_view)
            .unwrap_or_else(|| fatal(b"gpu-cube: depth view generations exhausted"));
        batch
            .destroy_resource(self.depth_texture_view)
            .and_then(|_| batch.destroy_resource(self.depth_texture))
            .and_then(|_| self.encode_depth_resources(&mut batch, texture, view))
            .unwrap_or_else(|_| fatal(b"gpu-cube: resize encoding failed"));
        let status = epoca_gpu::pvm::submit(&batch);
        if status == 0 {
            self.depth_texture = texture;
            self.depth_texture_view = view;
        }
        status
    }

    fn render_batch(&self) -> Result<BatchEncoder, epoca_gpu::EncodeError> {
        let mut batch = BatchEncoder::new(self.next_sequence)?;
        let uniform = uniform_bytes(
            self.frame as f32 * 0.02,
            self.surface.width as f32 / self.surface.height as f32,
        );
        batch.write_buffer(UNIFORM_BUFFER, 0, &uniform)?;
        batch.begin_render_pass(
            0,
            self.depth_texture_view,
            self.surface.generation,
            wire::GPU_RENDER_PASS_COLOR_STORE | wire::GPU_RENDER_PASS_DEPTH_STORE,
            [0.025, 0.035, 0.07, 1.0],
            1.0,
        )?;
        batch.set_pipeline(PIPELINE)?;
        batch.set_vertex_buffer(0, VERTEX_BUFFER, 0, (VERTEX_COUNT as u64) * VERTEX_STRIDE)?;
        batch.set_index_buffer(
            INDEX_BUFFER,
            GpuIndexFormat::Uint16 as u32,
            0,
            (INDEX_COUNT as u64) * 2,
        )?;
        batch.set_bind_group(0, BIND_GROUP, &[])?;
        batch.set_viewport(
            0.0,
            0.0,
            self.surface.width as f32,
            self.surface.height as f32,
            0.0,
            1.0,
        )?;
        batch.set_scissor_rect(0, 0, self.surface.width, self.surface.height)?;
        batch.draw_indexed(INDEX_COUNT, 1, 0, 0, 0)?;
        batch.end_render_pass()?;
        Ok(batch)
    }
}

fn read_surface() -> Surface {
    let mut bytes = [0u8; CAPABILITIES_BYTES];
    let length = epoca_gpu::pvm::capabilities(&mut bytes);
    if length <= 0 || length as usize > bytes.len() {
        fatal(b"gpu-cube: capabilities unavailable");
    }
    let capabilities = parse_capabilities(&bytes[..length as usize])
        .unwrap_or_else(|_| fatal(b"gpu-cube: invalid capabilities"));
    Surface {
        format: capabilities.surface_format,
        width: capabilities.physical_width,
        height: capabilities.physical_height,
        generation: capabilities.surface_generation,
    }
}

fn binding_layout() -> [BindGroupLayoutEntry; 3] {
    [
        BindGroupLayoutEntry {
            binding: 0,
            visibility: wire::GPU_SHADER_STAGE_VERTEX,
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
        attribute_count: 2,
    }];
    const ATTRIBUTES: [VertexAttribute; 2] = [
        VertexAttribute {
            format: GpuVertexFormat::Float32x3 as u16,
            shader_location: 0,
            offset: 0,
        },
        VertexAttribute {
            format: GpuVertexFormat::Float32x2 as u16,
            shader_location: 1,
            offset: 12,
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

fn vertex_bytes() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(VERTEX_COUNT * VERTEX_STRIDE as usize);
    for vertex in VERTICES {
        for value in vertex {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

fn index_bytes() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(INDICES.len() * 2);
    for index in INDICES {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    bytes
}

fn uniform_bytes(angle: f32, aspect: f32) -> [u8; 64] {
    let (sin_y, cos_y) = (libm::sinf(angle), libm::cosf(angle));
    let (sin_x, cos_x) = (libm::sinf(angle * 0.63), libm::cosf(angle * 0.63));
    let rotation_y = [
        cos_y, 0.0, -sin_y, 0.0, 0.0, 1.0, 0.0, 0.0, sin_y, 0.0, cos_y, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let rotation_x = [
        1.0, 0.0, 0.0, 0.0, 0.0, cos_x, sin_x, 0.0, 0.0, -sin_x, cos_x, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let view = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -4.0, 1.0,
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
    let matrix = multiply(projection, multiply(view, multiply(rotation_y, rotation_x)));
    let mut bytes = [0u8; 64];
    for (index, value) in matrix.into_iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
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

static mut APP: Option<CubeApp> = None;

#[polkavm_derive::polkavm_export]
extern "C" fn init() {
    if !unsafe { ALLOCATOR.initialize() } {
        fatal(b"gpu-cube: heap reservation failed");
    }
    log(b"gpu-cube: init");
}

#[polkavm_derive::polkavm_export]
extern "C" fn update() {
    unsafe {
        if APP.is_none() {
            APP = Some(CubeApp::new(read_surface()));
            log(b"gpu-cube: capabilities ready");
        }
        APP.as_mut().unwrap().update();
    }
}

fn log(message: &[u8]) {
    unsafe { host_log(message.as_ptr() as u32, message.len() as u32) }
}

fn fatal(message: &[u8]) -> ! {
    log(message);
    trap()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    fatal(b"gpu-cube: panic")
}

#[alloc_error_handler]
fn allocation_error(_layout: Layout) -> ! {
    fatal(b"gpu-cube: allocation failed")
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
