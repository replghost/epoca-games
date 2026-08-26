/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::fmt;
use egui::epaint::{ClippedPrimitive, ImageData, Primitive, TextureId};
use egui::{Rect, TextureFilter, TextureOptions, TextureWrapMode, TexturesDelta};
use epoca_gpu::wire::{
    self, GpuAddressMode, GpuBindingKind, GpuBlendFactor, GpuBlendOperation, GpuFilterMode,
    GpuFrontFace, GpuIndexFormat, GpuPrimitiveTopology, GpuSamplerBindingType, GpuTextureAspect,
    GpuTextureFormat, GpuTextureSampleType, GpuTextureViewDimension, GpuVertexFormat,
    GpuVertexStepMode,
};
use epoca_gpu::{
    BatchEncoder, BindGroupEntry, BindGroupLayoutEntry, ColorTarget, EncodeError,
    RenderPipelineDescriptor, SamplerDescriptor, VertexAttribute, VertexBufferLayout,
};

const VERTEX_STRIDE: u64 = 20;
const SCREEN_UNIFORM_BYTES: u64 = 16;
const VERTEX_BUFFER_BYTES: usize = 2 * 1024 * 1024;
const INDEX_BUFFER_BYTES: usize = 1024 * 1024;
const FIRST_TEXTURE_SLOT: u32 = 16;
const MAX_SLOT: u32 = (1 << wire::GPU_HANDLE_SLOT_BITS) - 1;
const MAX_GENERATION: u32 = (1 << (32 - wire::GPU_HANDLE_SLOT_BITS)) - 1;

const SCREEN_UNIFORM: u32 = 0;
const VERTEX_BUFFER: u32 = 1;
const INDEX_BUFFER: u32 = 2;
const SCREEN_LAYOUT: u32 = 3;
const TEXTURE_LAYOUT: u32 = 4;
const PIPELINE_LAYOUT: u32 = 5;
const SCREEN_BIND_GROUP: u32 = 6;
const SHADER: u32 = 7;
const PIPELINE: u32 = 8;

const SHADER_SOURCE: &str = r#"
struct Screen {
    logical_size: vec2<f32>,
    _padding: vec2<f32>,
};

@group(0) @binding(0) var<uniform> screen: Screen;
@group(1) @binding(0) var color_texture: texture_2d<f32>;
@group(1) @binding(1) var color_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

fn unpack_color(color: u32) -> vec4<f32> {
    return vec4<f32>(
        f32(color & 255u),
        f32((color >> 8u) & 255u),
        f32((color >> 16u) & 255u),
        f32((color >> 24u) & 255u),
    ) / 255.0;
}

fn linear_from_gamma(value: vec3<f32>) -> vec3<f32> {
    let lower = value / vec3<f32>(12.92);
    let higher = pow(
        (value + vec3<f32>(0.055)) / vec3<f32>(1.055),
        vec3<f32>(2.4),
    );
    return select(higher, lower, value < vec3<f32>(0.04045));
}

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: u32,
) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(
        2.0 * position.x / screen.logical_size.x - 1.0,
        1.0 - 2.0 * position.y / screen.logical_size.y,
        0.0,
        1.0,
    );
    output.uv = uv;
    output.color = unpack_color(color);
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let gamma = input.color * textureSample(color_texture, color_sampler, input.uv);
    return vec4<f32>(linear_from_gamma(gamma.rgb), gamma.a);
}
"#;

#[derive(Clone, Copy, Debug)]
pub struct Surface {
    pub format: u16,
    pub physical_width: u32,
    pub physical_height: u32,
    pub logical_width: u32,
    pub logical_height: u32,
    pub pixels_per_point: f32,
    pub generation: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PaintStats {
    pub vertices: u32,
    pub indices: u32,
    pub draws: u32,
    pub upload_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RendererError {
    InvalidHandleRange,
    NotInitialized,
    AlreadyInitialized,
    InvalidSurface,
    InvalidTexture,
    MissingTexture,
    UnsupportedPrimitive,
    VertexBufferLimit,
    IndexBufferLimit,
    FieldOutOfRange,
    Encode(EncodeError),
}

impl fmt::Display for RendererError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidHandleRange => "egui GPU handle range is invalid",
            Self::NotInitialized => "egui GPU renderer is not initialized",
            Self::AlreadyInitialized => "egui GPU renderer is already initialized",
            Self::InvalidSurface => "egui GPU surface is invalid",
            Self::InvalidTexture => "egui texture update is invalid",
            Self::MissingTexture => "egui mesh references a missing texture",
            Self::UnsupportedPrimitive => "egui callback primitives are unsupported",
            Self::VertexBufferLimit => "egui vertex buffer limit exceeded",
            Self::IndexBufferLimit => "egui index buffer limit exceeded",
            Self::FieldOutOfRange => "egui GPU field is out of range",
            Self::Encode(_) => "egui GPU command encoding failed",
        })
    }
}

impl core::error::Error for RendererError {}

impl From<EncodeError> for RendererError {
    fn from(error: EncodeError) -> Self {
        Self::Encode(error)
    }
}

#[derive(Clone)]
struct TextureBinding {
    id: TextureId,
    size: [usize; 2],
    options: TextureOptions,
    texture: u32,
    view: u32,
    sampler: u32,
    bind_group: u32,
    texture_layout: u32,
}

#[derive(Clone)]
struct HandleAllocator {
    next_slot: u32,
    free: Vec<u32>,
}

impl HandleAllocator {
    fn new(next_slot: u32) -> Self {
        Self {
            next_slot,
            free: Vec::new(),
        }
    }

    fn allocate(&mut self) -> Result<u32, RendererError> {
        while let Some(prior) = self.free.pop() {
            let generation = prior >> wire::GPU_HANDLE_SLOT_BITS;
            if generation < MAX_GENERATION {
                return Ok(((generation + 1) << wire::GPU_HANDLE_SLOT_BITS) | (prior & MAX_SLOT));
            }
        }
        if self.next_slot > MAX_SLOT {
            return Err(RendererError::InvalidHandleRange);
        }
        let slot = self.next_slot;
        self.next_slot = self
            .next_slot
            .checked_add(1)
            .ok_or(RendererError::InvalidHandleRange)?;
        Ok(handle(slot))
    }

    fn release(&mut self, handle: u32) {
        self.free.push(handle);
    }

    fn release_texture(&mut self, binding: &TextureBinding) {
        self.release(binding.bind_group);
        self.release(binding.sampler);
        self.release(binding.view);
        self.release(binding.texture);
    }
}

#[derive(Clone, Copy)]
struct Draw {
    texture: TextureId,
    clip: [u32; 4],
    first_index: u32,
    index_count: u32,
    base_vertex: i32,
}

pub struct EguiRenderer {
    base_slot: u32,
    texture_handles: HandleAllocator,
    textures: Vec<TextureBinding>,
    initialized: bool,
    last_stats: PaintStats,
}

impl EguiRenderer {
    pub fn new(base_slot: u32) -> Result<Self, RendererError> {
        if base_slot == 0
            || base_slot
                .checked_add(FIRST_TEXTURE_SLOT)
                .is_none_or(|slot| slot > MAX_SLOT)
        {
            return Err(RendererError::InvalidHandleRange);
        }
        Ok(Self {
            base_slot,
            texture_handles: HandleAllocator::new(base_slot + FIRST_TEXTURE_SLOT),
            textures: Vec::new(),
            initialized: false,
            last_stats: PaintStats::default(),
        })
    }

    pub fn initialize(
        &mut self,
        batch: &mut BatchEncoder,
        surface_format: u16,
    ) -> Result<(), RendererError> {
        if self.initialized {
            return Err(RendererError::AlreadyInitialized);
        }
        let handles = StaticHandles::new(self.base_slot);
        batch.transaction(|batch| {
            batch.create_buffer(
                handles.screen_uniform,
                wire::GPU_BUFFER_USAGE_COPY_DST | wire::GPU_BUFFER_USAGE_UNIFORM,
                SCREEN_UNIFORM_BYTES,
            )?;
            batch.create_buffer(
                handles.vertex_buffer,
                wire::GPU_BUFFER_USAGE_COPY_DST | wire::GPU_BUFFER_USAGE_VERTEX,
                VERTEX_BUFFER_BYTES as u64,
            )?;
            batch.create_buffer(
                handles.index_buffer,
                wire::GPU_BUFFER_USAGE_COPY_DST | wire::GPU_BUFFER_USAGE_INDEX,
                INDEX_BUFFER_BYTES as u64,
            )?;
            batch.create_bind_group_layout(handles.screen_layout, &[screen_layout_entry()])?;
            batch.create_bind_group_layout(handles.texture_layout, &texture_layout_entries())?;
            batch.create_pipeline_layout(
                handles.pipeline_layout,
                &[handles.screen_layout, handles.texture_layout],
            )?;
            batch.create_bind_group(
                handles.screen_bind_group,
                handles.screen_layout,
                &[BindGroupEntry {
                    binding: 0,
                    resource: handles.screen_uniform,
                    kind: GpuBindingKind::UniformBuffer as u16,
                    offset: 0,
                    size: SCREEN_UNIFORM_BYTES,
                }],
            )?;
            batch.create_shader_wgsl(handles.shader, SHADER_SOURCE)?;
            batch.create_render_pipeline(RenderPipelineDescriptor {
                id: handles.pipeline,
                layout: handles.pipeline_layout,
                shader: handles.shader,
                flags: 0,
                depth_format: 0,
                sample_count: 1,
                topology: GpuPrimitiveTopology::TriangleList as u8,
                front_face: GpuFrontFace::Ccw as u8,
                cull_mode: 0,
                strip_index_format: 0,
                depth_compare: 0,
                vertex_layouts: &[VertexBufferLayout {
                    array_stride: VERTEX_STRIDE,
                    step_mode: GpuVertexStepMode::Vertex as u8,
                    first_attribute: 0,
                    attribute_count: 3,
                }],
                vertex_attributes: &[
                    VertexAttribute {
                        format: GpuVertexFormat::Float32x2 as u16,
                        shader_location: 0,
                        offset: 0,
                    },
                    VertexAttribute {
                        format: GpuVertexFormat::Float32x2 as u16,
                        shader_location: 1,
                        offset: 8,
                    },
                    VertexAttribute {
                        format: GpuVertexFormat::Uint32 as u16,
                        shader_location: 2,
                        offset: 16,
                    },
                ],
                color_targets: &[ColorTarget {
                    format: surface_format,
                    write_mask: wire::GPU_COLOR_WRITE_RED
                        | wire::GPU_COLOR_WRITE_GREEN
                        | wire::GPU_COLOR_WRITE_BLUE
                        | wire::GPU_COLOR_WRITE_ALPHA,
                    color_operation: GpuBlendOperation::Add as u8,
                    color_source_factor: GpuBlendFactor::One as u8,
                    color_destination_factor: GpuBlendFactor::OneMinusSrcAlpha as u8,
                    alpha_operation: GpuBlendOperation::Add as u8,
                    alpha_source_factor: GpuBlendFactor::One as u8,
                    alpha_destination_factor: GpuBlendFactor::OneMinusSrcAlpha as u8,
                }],
            })?;
            Ok::<(), EncodeError>(())
        })?;
        self.initialized = true;
        Ok(())
    }

    pub fn paint(
        &mut self,
        batch: &mut BatchEncoder,
        textures_delta: TexturesDelta,
        primitives: &[ClippedPrimitive],
        surface: Surface,
    ) -> Result<PaintStats, RendererError> {
        if !self.initialized {
            return Err(RendererError::NotInitialized);
        }
        if surface.physical_width == 0
            || surface.physical_height == 0
            || surface.logical_width == 0
            || surface.logical_height == 0
            || !surface.pixels_per_point.is_finite()
            || surface.pixels_per_point <= 0.0
        {
            return Err(RendererError::InvalidSurface);
        }
        let handles = StaticHandles::new(self.base_slot);

        let mut textures = self.textures.clone();
        let mut texture_handles = self.texture_handles.clone();
        let mut stats = PaintStats::default();
        batch.transaction(|batch| {
            update_textures(
                batch,
                &mut textures,
                &mut texture_handles,
                handles.texture_layout,
                &textures_delta,
                &mut stats,
            )?;

            let mut vertex_bytes = Vec::new();
            let mut index_bytes = Vec::new();
            let mut draws = Vec::new();
            encode_meshes(
                primitives,
                &textures,
                surface,
                &mut vertex_bytes,
                &mut index_bytes,
                &mut draws,
                &mut stats,
            )?;

            let mut screen = [0u8; SCREEN_UNIFORM_BYTES as usize];
            screen[0..4].copy_from_slice(&(surface.logical_width as f32).to_le_bytes());
            screen[4..8].copy_from_slice(&(surface.logical_height as f32).to_le_bytes());
            batch.write_buffer(handles.screen_uniform, 0, &screen)?;
            if !vertex_bytes.is_empty() {
                batch.write_buffer(handles.vertex_buffer, 0, &vertex_bytes)?;
                batch.write_buffer(handles.index_buffer, 0, &index_bytes)?;
            }
            stats.upload_bytes = stats
                .upload_bytes
                .saturating_add(screen.len() + vertex_bytes.len() + index_bytes.len());

            batch.begin_render_pass(
                0,
                0,
                surface.generation,
                wire::GPU_RENDER_PASS_COLOR_LOAD | wire::GPU_RENDER_PASS_COLOR_STORE,
                [0.0; 4],
                1.0,
            )?;
            batch.set_pipeline(handles.pipeline)?;
            batch.set_vertex_buffer(0, handles.vertex_buffer, 0, vertex_bytes.len() as u64)?;
            batch.set_index_buffer(
                handles.index_buffer,
                GpuIndexFormat::Uint32 as u32,
                0,
                index_bytes.len() as u64,
            )?;
            batch.set_bind_group(0, handles.screen_bind_group, &[])?;
            batch.set_viewport(
                0.0,
                0.0,
                surface.physical_width as f32,
                surface.physical_height as f32,
                0.0,
                1.0,
            )?;
            for draw in &draws {
                let binding = textures
                    .iter()
                    .find(|binding| binding.id == draw.texture)
                    .ok_or(RendererError::MissingTexture)?;
                batch.set_scissor_rect(draw.clip[0], draw.clip[1], draw.clip[2], draw.clip[3])?;
                batch.set_bind_group(1, binding.bind_group, &[])?;
                batch.draw_indexed(draw.index_count, 1, draw.first_index, draw.base_vertex, 0)?;
            }
            batch.end_render_pass()?;

            for id in &textures_delta.free {
                let index = textures
                    .iter()
                    .position(|binding| binding.id == *id)
                    .ok_or(RendererError::MissingTexture)?;
                let binding = textures.swap_remove(index);
                destroy_texture(batch, &binding)?;
                texture_handles.release_texture(&binding);
            }
            Ok::<(), RendererError>(())
        })?;

        self.textures = textures;
        self.texture_handles = texture_handles;
        self.last_stats = stats;
        Ok(stats)
    }

    pub fn last_stats(&self) -> PaintStats {
        self.last_stats
    }
}

#[derive(Clone, Copy)]
struct StaticHandles {
    screen_uniform: u32,
    vertex_buffer: u32,
    index_buffer: u32,
    screen_layout: u32,
    texture_layout: u32,
    pipeline_layout: u32,
    screen_bind_group: u32,
    shader: u32,
    pipeline: u32,
}

impl StaticHandles {
    fn new(base_slot: u32) -> Self {
        Self {
            screen_uniform: handle(base_slot + SCREEN_UNIFORM),
            vertex_buffer: handle(base_slot + VERTEX_BUFFER),
            index_buffer: handle(base_slot + INDEX_BUFFER),
            screen_layout: handle(base_slot + SCREEN_LAYOUT),
            texture_layout: handle(base_slot + TEXTURE_LAYOUT),
            pipeline_layout: handle(base_slot + PIPELINE_LAYOUT),
            screen_bind_group: handle(base_slot + SCREEN_BIND_GROUP),
            shader: handle(base_slot + SHADER),
            pipeline: handle(base_slot + PIPELINE),
        }
    }
}

fn handle(slot: u32) -> u32 {
    (1 << wire::GPU_HANDLE_SLOT_BITS) | slot
}

fn screen_layout_entry() -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding: 0,
        visibility: wire::GPU_SHADER_STAGE_VERTEX,
        kind: GpuBindingKind::UniformBuffer as u16,
        flags: 0,
        minimum_binding_size: SCREEN_UNIFORM_BYTES,
        parameter_0: 0,
        parameter_1: 0,
    }
}

fn texture_layout_entries() -> [BindGroupLayoutEntry; 2] {
    [
        BindGroupLayoutEntry {
            binding: 0,
            visibility: wire::GPU_SHADER_STAGE_FRAGMENT,
            kind: GpuBindingKind::Texture as u16,
            flags: 0,
            minimum_binding_size: 0,
            parameter_0: GpuTextureSampleType::FloatFilterable as u32,
            parameter_1: GpuTextureViewDimension::D2 as u32,
        },
        BindGroupLayoutEntry {
            binding: 1,
            visibility: wire::GPU_SHADER_STAGE_FRAGMENT,
            kind: GpuBindingKind::Sampler as u16,
            flags: 0,
            minimum_binding_size: 0,
            parameter_0: GpuSamplerBindingType::Filtering as u32,
            parameter_1: 0,
        },
    ]
}

fn update_textures(
    batch: &mut BatchEncoder,
    textures: &mut Vec<TextureBinding>,
    handles: &mut HandleAllocator,
    texture_layout: u32,
    delta: &TexturesDelta,
    stats: &mut PaintStats,
) -> Result<(), RendererError> {
    for (id, image_delta) in &delta.set {
        let ImageData::Color(image) = &image_delta.image;
        let rgba = image_rgba(image)?;
        let width = u32::try_from(image.size[0]).map_err(|_| RendererError::FieldOutOfRange)?;
        let height = u32::try_from(image.size[1]).map_err(|_| RendererError::FieldOutOfRange)?;
        let bytes_per_row = width.checked_mul(4).ok_or(RendererError::FieldOutOfRange)?;
        stats.upload_bytes = stats.upload_bytes.saturating_add(rgba.len());

        if let Some(position) = image_delta.pos {
            let binding = textures
                .iter_mut()
                .find(|binding| binding.id == *id)
                .ok_or(RendererError::MissingTexture)?;
            if position[0]
                .checked_add(image.size[0])
                .is_none_or(|right| right > binding.size[0])
                || position[1]
                    .checked_add(image.size[1])
                    .is_none_or(|bottom| bottom > binding.size[1])
            {
                return Err(RendererError::InvalidTexture);
            }
            batch.write_texture_2d(
                binding.texture,
                0,
                u32::try_from(position[0]).map_err(|_| RendererError::FieldOutOfRange)?,
                u32::try_from(position[1]).map_err(|_| RendererError::FieldOutOfRange)?,
                width,
                height,
                bytes_per_row,
                height,
                &rgba,
            )?;
            if binding.options != image_delta.options {
                batch.destroy_resource(binding.bind_group)?;
                batch.destroy_resource(binding.sampler)?;
                handles.release(binding.bind_group);
                handles.release(binding.sampler);
                binding.sampler = handles.allocate()?;
                binding.bind_group = handles.allocate()?;
                batch.create_sampler(binding.sampler, sampler_descriptor(image_delta.options))?;
                batch.create_bind_group(
                    binding.bind_group,
                    binding.texture_layout,
                    &texture_bindings(binding),
                )?;
                binding.options = image_delta.options;
            }
            continue;
        }

        if let Some(index) = textures.iter().position(|binding| binding.id == *id) {
            let prior = textures[index].clone();
            if prior.size == image.size && prior.options == image_delta.options {
                batch.write_texture_2d(
                    prior.texture,
                    0,
                    0,
                    0,
                    width,
                    height,
                    bytes_per_row,
                    height,
                    &rgba,
                )?;
                continue;
            }
            destroy_texture(batch, &prior)?;
            handles.release_texture(&prior);
            let binding = allocate_texture_binding(
                handles,
                *id,
                image.size,
                image_delta.options,
                texture_layout,
            )?;
            create_texture(batch, &binding, width, height, &rgba)?;
            textures[index] = binding;
            continue;
        }

        let binding = allocate_texture_binding(
            handles,
            *id,
            image.size,
            image_delta.options,
            texture_layout,
        )?;
        create_texture(batch, &binding, width, height, &rgba)?;
        textures.push(binding);
    }
    Ok(())
}

fn allocate_texture_binding(
    handles: &mut HandleAllocator,
    id: TextureId,
    size: [usize; 2],
    options: TextureOptions,
    texture_layout: u32,
) -> Result<TextureBinding, RendererError> {
    Ok(TextureBinding {
        id,
        size,
        options,
        texture: handles.allocate()?,
        view: handles.allocate()?,
        sampler: handles.allocate()?,
        bind_group: handles.allocate()?,
        texture_layout,
    })
}

fn create_texture(
    batch: &mut BatchEncoder,
    binding: &TextureBinding,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<(), RendererError> {
    batch.create_texture_2d(
        binding.texture,
        width,
        height,
        1,
        1,
        GpuTextureFormat::Rgba8Unorm as u16,
        wire::GPU_TEXTURE_USAGE_COPY_DST | wire::GPU_TEXTURE_USAGE_TEXTURE_BINDING,
    )?;
    batch.write_texture_2d(
        binding.texture,
        0,
        0,
        0,
        width,
        height,
        width.checked_mul(4).ok_or(RendererError::FieldOutOfRange)?,
        height,
        rgba,
    )?;
    batch.create_texture_view(
        binding.view,
        binding.texture,
        GpuTextureFormat::Rgba8Unorm as u16,
        GpuTextureAspect::All as u8,
        0,
        1,
        0,
        1,
    )?;
    batch.create_sampler(binding.sampler, sampler_descriptor(binding.options))?;
    batch.create_bind_group(
        binding.bind_group,
        binding.texture_layout,
        &texture_bindings(binding),
    )?;
    Ok(())
}

fn texture_bindings(binding: &TextureBinding) -> [BindGroupEntry; 2] {
    [
        BindGroupEntry {
            binding: 0,
            resource: binding.view,
            kind: GpuBindingKind::Texture as u16,
            offset: 0,
            size: 0,
        },
        BindGroupEntry {
            binding: 1,
            resource: binding.sampler,
            kind: GpuBindingKind::Sampler as u16,
            offset: 0,
            size: 0,
        },
    ]
}

fn destroy_texture(
    batch: &mut BatchEncoder,
    binding: &TextureBinding,
) -> Result<(), RendererError> {
    batch.destroy_resource(binding.bind_group)?;
    batch.destroy_resource(binding.sampler)?;
    batch.destroy_resource(binding.view)?;
    batch.destroy_resource(binding.texture)?;
    Ok(())
}

fn sampler_descriptor(options: TextureOptions) -> SamplerDescriptor {
    let address = match options.wrap_mode {
        TextureWrapMode::ClampToEdge => GpuAddressMode::ClampToEdge,
        TextureWrapMode::Repeat => GpuAddressMode::Repeat,
        TextureWrapMode::MirroredRepeat => GpuAddressMode::MirrorRepeat,
    } as u8;
    SamplerDescriptor {
        address_u: address,
        address_v: address,
        address_w: address,
        mag_filter: filter(options.magnification),
        min_filter: filter(options.minification),
        mipmap_filter: filter(options.mipmap_mode.unwrap_or(TextureFilter::Nearest)),
        compare: 0,
        max_anisotropy: 1,
        lod_min: 0.0,
        lod_max: 32.0,
    }
}

fn filter(value: TextureFilter) -> u8 {
    (match value {
        TextureFilter::Nearest => GpuFilterMode::Nearest,
        TextureFilter::Linear => GpuFilterMode::Linear,
    }) as u8
}

fn image_rgba(image: &egui::ColorImage) -> Result<Vec<u8>, RendererError> {
    let pixels = image
        .size
        .iter()
        .copied()
        .try_fold(1usize, usize::checked_mul)
        .ok_or(RendererError::InvalidTexture)?;
    if pixels != image.pixels.len() {
        return Err(RendererError::InvalidTexture);
    }
    let byte_length = pixels.checked_mul(4).ok_or(RendererError::InvalidTexture)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(byte_length)
        .map_err(|_| RendererError::InvalidTexture)?;
    for color in &image.pixels {
        bytes.extend_from_slice(&color.to_array());
    }
    Ok(bytes)
}

#[allow(clippy::too_many_arguments)]
fn encode_meshes(
    primitives: &[ClippedPrimitive],
    textures: &[TextureBinding],
    surface: Surface,
    vertex_bytes: &mut Vec<u8>,
    index_bytes: &mut Vec<u8>,
    draws: &mut Vec<Draw>,
    stats: &mut PaintStats,
) -> Result<(), RendererError> {
    for primitive in primitives {
        let Primitive::Mesh(mesh) = &primitive.primitive else {
            return Err(RendererError::UnsupportedPrimitive);
        };
        if !textures.iter().any(|binding| binding.id == mesh.texture_id) {
            return Err(RendererError::MissingTexture);
        }
        let Some(clip) = clip_rect(
            primitive.clip_rect,
            surface.pixels_per_point,
            surface.physical_width,
            surface.physical_height,
        ) else {
            continue;
        };
        if mesh.vertices.is_empty()
            || mesh.indices.is_empty()
            || mesh.indices.len() % 3 != 0
            || mesh
                .indices
                .iter()
                .any(|index| *index as usize >= mesh.vertices.len())
        {
            return Err(RendererError::InvalidTexture);
        }
        let vertex_start = vertex_bytes.len() / VERTEX_STRIDE as usize;
        let first_index = index_bytes.len() / 4;
        let vertex_addition = mesh
            .vertices
            .len()
            .checked_mul(VERTEX_STRIDE as usize)
            .ok_or(RendererError::VertexBufferLimit)?;
        let index_addition = mesh
            .indices
            .len()
            .checked_mul(4)
            .ok_or(RendererError::IndexBufferLimit)?;
        if vertex_bytes.len().saturating_add(vertex_addition) > VERTEX_BUFFER_BYTES {
            return Err(RendererError::VertexBufferLimit);
        }
        if index_bytes.len().saturating_add(index_addition) > INDEX_BUFFER_BYTES {
            return Err(RendererError::IndexBufferLimit);
        }
        vertex_bytes
            .try_reserve_exact(vertex_addition)
            .map_err(|_| RendererError::VertexBufferLimit)?;
        index_bytes
            .try_reserve_exact(index_addition)
            .map_err(|_| RendererError::IndexBufferLimit)?;
        for vertex in &mesh.vertices {
            vertex_bytes.extend_from_slice(&vertex.pos.x.to_le_bytes());
            vertex_bytes.extend_from_slice(&vertex.pos.y.to_le_bytes());
            vertex_bytes.extend_from_slice(&vertex.uv.x.to_le_bytes());
            vertex_bytes.extend_from_slice(&vertex.uv.y.to_le_bytes());
            vertex_bytes.extend_from_slice(&vertex.color.to_array());
        }
        for index in &mesh.indices {
            index_bytes.extend_from_slice(&index.to_le_bytes());
        }
        draws.push(Draw {
            texture: mesh.texture_id,
            clip,
            first_index: u32::try_from(first_index).map_err(|_| RendererError::FieldOutOfRange)?,
            index_count: u32::try_from(mesh.indices.len())
                .map_err(|_| RendererError::FieldOutOfRange)?,
            base_vertex: i32::try_from(vertex_start).map_err(|_| RendererError::FieldOutOfRange)?,
        });
        stats.vertices = stats
            .vertices
            .saturating_add(u32::try_from(mesh.vertices.len()).unwrap_or(u32::MAX));
        stats.indices = stats
            .indices
            .saturating_add(u32::try_from(mesh.indices.len()).unwrap_or(u32::MAX));
        stats.draws = stats.draws.saturating_add(1);
    }
    Ok(())
}

fn clip_rect(rect: Rect, pixels_per_point: f32, width: u32, height: u32) -> Option<[u32; 4]> {
    let min_x = to_pixel_floor(rect.min.x * pixels_per_point).clamp(0, width as i32);
    let min_y = to_pixel_floor(rect.min.y * pixels_per_point).clamp(0, height as i32);
    let max_x = to_pixel_ceil(rect.max.x * pixels_per_point).clamp(min_x, width as i32);
    let max_y = to_pixel_ceil(rect.max.y * pixels_per_point).clamp(min_y, height as i32);
    if min_x == max_x || min_y == max_y {
        return None;
    }
    Some([
        min_x as u32,
        min_y as u32,
        (max_x - min_x) as u32,
        (max_y - min_y) as u32,
    ])
}

fn to_pixel_floor(value: f32) -> i32 {
    if value <= i32::MIN as f32 {
        i32::MIN
    } else if value >= i32::MAX as f32 {
        i32::MAX
    } else {
        value as i32
    }
}

fn to_pixel_ceil(value: f32) -> i32 {
    let floor = to_pixel_floor(value);
    if value > floor as f32 {
        floor.saturating_add(1)
    } else {
        floor
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::sync::Arc;
    use alloc::vec;
    use egui::epaint::{ClippedPrimitive, PaintCallback, Primitive};
    use egui::{CentralPanel, Context, RawInput, TextureId, Vec2};

    const TEST_SURFACE: Surface = Surface {
        format: GpuTextureFormat::Bgra8Unorm as u16,
        physical_width: 640,
        physical_height: 480,
        logical_width: 640,
        logical_height: 480,
        pixels_per_point: 1.0,
        generation: 1,
    };

    fn frame(context: &Context) -> (TexturesDelta, Vec<ClippedPrimitive>) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(640.0, 480.0),
            )),
            ..Default::default()
        };
        let output = context.run(input, |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                ui.heading("PVM egui");
                ui.label("GPU adapter contract");
            });
        });
        let primitives = context.tessellate(output.shapes, output.pixels_per_point);
        (output.textures_delta, primitives)
    }

    #[test]
    fn initializes_and_encodes_a_real_egui_frame() {
        let mut renderer = EguiRenderer::new(128).unwrap();
        let mut resources = BatchEncoder::new(1).unwrap();
        renderer
            .initialize(&mut resources, TEST_SURFACE.format)
            .unwrap();
        let resource_batch = wire::decode_gpu_batch(resources.as_bytes()).unwrap();
        assert!(resource_batch
            .commands()
            .any(|command| command.opcode == wire::GpuOpcode::CreateRenderPipeline));

        let context = Context::default();
        let (textures, primitives) = frame(&context);
        let mut batch = BatchEncoder::new(2).unwrap();
        let stats = renderer
            .paint(&mut batch, textures, &primitives, TEST_SURFACE)
            .unwrap();
        assert!(stats.vertices > 0);
        assert!(stats.indices > 0);
        assert!(stats.draws > 0);

        let encoded = wire::decode_gpu_batch(batch.as_bytes()).unwrap();
        let opcodes = encoded
            .commands()
            .map(|command| command.opcode)
            .collect::<Vec<_>>();
        assert!(opcodes.contains(&wire::GpuOpcode::CreateTexture));
        assert!(opcodes.contains(&wire::GpuOpcode::BeginRenderPass));
        assert!(opcodes.contains(&wire::GpuOpcode::DrawIndexed));
        assert_eq!(opcodes.last(), Some(&wire::GpuOpcode::EndRenderPass));
    }

    #[test]
    fn frees_textures_only_after_the_render_pass() {
        let mut renderer = EguiRenderer::new(128).unwrap();
        let mut resources = BatchEncoder::new(1).unwrap();
        renderer
            .initialize(&mut resources, TEST_SURFACE.format)
            .unwrap();
        let context = Context::default();
        let (textures, primitives) = frame(&context);
        let mut first = BatchEncoder::new(2).unwrap();
        renderer
            .paint(&mut first, textures, &primitives, TEST_SURFACE)
            .unwrap();

        let mut second = BatchEncoder::new(3).unwrap();
        renderer
            .paint(
                &mut second,
                TexturesDelta {
                    set: Vec::new(),
                    free: vec![TextureId::default()],
                },
                &[],
                TEST_SURFACE,
            )
            .unwrap();
        let encoded = wire::decode_gpu_batch(second.as_bytes()).unwrap();
        let opcodes = encoded
            .commands()
            .map(|command| command.opcode)
            .collect::<Vec<_>>();
        let pass_end = opcodes
            .iter()
            .position(|opcode| *opcode == wire::GpuOpcode::EndRenderPass)
            .unwrap();
        let first_destroy = opcodes
            .iter()
            .position(|opcode| *opcode == wire::GpuOpcode::DestroyResource)
            .unwrap();
        assert!(first_destroy > pass_end);
    }

    #[test]
    fn rejected_primitives_leave_batch_and_renderer_unchanged() {
        let mut renderer = EguiRenderer::new(128).unwrap();
        let mut resources = BatchEncoder::new(1).unwrap();
        renderer
            .initialize(&mut resources, TEST_SURFACE.format)
            .unwrap();
        let mut batch = BatchEncoder::new(2).unwrap();
        let before = batch.as_bytes().to_vec();
        let primitive = ClippedPrimitive {
            clip_rect: Rect::EVERYTHING,
            primitive: Primitive::Callback(PaintCallback {
                rect: Rect::EVERYTHING,
                callback: Arc::new(()),
            }),
        };
        assert_eq!(
            renderer
                .paint(
                    &mut batch,
                    TexturesDelta::default(),
                    &[primitive],
                    TEST_SURFACE,
                )
                .unwrap_err(),
            RendererError::UnsupportedPrimitive
        );
        assert_eq!(batch.as_bytes(), before);
        assert_eq!(renderer.last_stats(), PaintStats::default());
    }

    #[test]
    fn released_handle_slots_advance_their_generation() {
        let mut handles = HandleAllocator::new(128);
        let first = handles.allocate().unwrap();
        handles.release(first);
        let second = handles.allocate().unwrap();
        assert_eq!(first & MAX_SLOT, second & MAX_SLOT);
        assert_eq!(
            (first >> wire::GPU_HANDLE_SLOT_BITS) + 1,
            second >> wire::GPU_HANDLE_SLOT_BITS
        );
    }
}
