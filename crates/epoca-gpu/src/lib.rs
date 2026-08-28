/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![no_std]

extern crate alloc;

use alloc::collections::TryReserveError;
use alloc::vec::Vec;
use core::fmt;

pub use epoca_gpu_wire as wire;
use wire::{
    GpuOpcode, GPU_BATCH_HEADER_BYTES, GPU_COMMAND_HEADER_BYTES, GPU_WIRE_MAGIC, GPU_WIRE_VERSION,
    MAX_GPU_BATCH_BYTES, MAX_GPU_COMMANDS,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncodeError {
    EmptySequence,
    TooManyCommands,
    BatchTooLarge,
    FieldOutOfRange,
    DescriptorLengthMismatch,
    AllocationFailed,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptySequence => "GPU batch sequence must be nonzero",
            Self::TooManyCommands => "GPU batch command limit exceeded",
            Self::BatchTooLarge => "GPU batch byte limit exceeded",
            Self::FieldOutOfRange => "GPU descriptor field is out of range",
            Self::DescriptorLengthMismatch => "GPU descriptor encoded an unexpected byte length",
            Self::AllocationFailed => "GPU batch allocation failed",
        })
    }
}

impl core::error::Error for EncodeError {}

impl From<TryReserveError> for EncodeError {
    fn from(_: TryReserveError) -> Self {
        Self::AllocationFailed
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BindGroupLayoutEntry {
    pub binding: u32,
    pub visibility: u32,
    pub kind: u16,
    pub flags: u16,
    pub minimum_binding_size: u64,
    pub parameter_0: u32,
    pub parameter_1: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct BindGroupEntry {
    pub binding: u32,
    pub resource: u32,
    pub kind: u16,
    pub offset: u64,
    pub size: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct SamplerDescriptor {
    pub address_u: u8,
    pub address_v: u8,
    pub address_w: u8,
    pub mag_filter: u8,
    pub min_filter: u8,
    pub mipmap_filter: u8,
    pub compare: u8,
    pub max_anisotropy: u8,
    pub lod_min: f32,
    pub lod_max: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct VertexBufferLayout {
    pub array_stride: u64,
    pub step_mode: u8,
    pub first_attribute: u16,
    pub attribute_count: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct VertexAttribute {
    pub format: u16,
    pub shader_location: u16,
    pub offset: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct ColorTarget {
    pub format: u16,
    pub write_mask: u16,
    pub color_operation: u8,
    pub color_source_factor: u8,
    pub color_destination_factor: u8,
    pub alpha_operation: u8,
    pub alpha_source_factor: u8,
    pub alpha_destination_factor: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct RenderPipelineDescriptor<'a> {
    pub id: u32,
    pub layout: u32,
    pub shader: u32,
    pub flags: u16,
    pub depth_format: u16,
    pub sample_count: u16,
    pub topology: u8,
    pub front_face: u8,
    pub cull_mode: u8,
    pub strip_index_format: u8,
    pub depth_compare: u8,
    pub vertex_layouts: &'a [VertexBufferLayout],
    pub vertex_attributes: &'a [VertexAttribute],
    pub color_targets: &'a [ColorTarget],
}

#[derive(Clone, Debug)]
pub struct BatchEncoder {
    bytes: Vec<u8>,
    command_count: u32,
}

impl BatchEncoder {
    pub fn new(sequence: u64) -> Result<Self, EncodeError> {
        if sequence == 0 {
            return Err(EncodeError::EmptySequence);
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(GPU_BATCH_HEADER_BYTES)?;
        bytes.extend_from_slice(&GPU_WIRE_MAGIC);
        push_u16(&mut bytes, GPU_WIRE_VERSION);
        push_u16(&mut bytes, 0);
        push_u32(&mut bytes, GPU_BATCH_HEADER_BYTES as u32);
        push_u32(&mut bytes, 0);
        push_u64(&mut bytes, sequence);
        Ok(Self {
            bytes,
            command_count: 0,
        })
    }

    pub fn sequence(&self) -> u64 {
        u64::from_le_bytes(self.bytes[16..24].try_into().expect("fixed batch header"))
    }

    pub fn command_count(&self) -> u32 {
        self.command_count
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn transaction<T, E>(
        &mut self,
        encode: impl FnOnce(&mut Self) -> Result<T, E>,
    ) -> Result<T, E> {
        let byte_length = self.bytes.len();
        let command_count = self.command_count;
        match encode(self) {
            Ok(value) => Ok(value),
            Err(error) => {
                self.bytes.truncate(byte_length);
                self.command_count = command_count;
                self.bytes[8..12].copy_from_slice(&(byte_length as u32).to_le_bytes());
                self.bytes[12..16].copy_from_slice(&command_count.to_le_bytes());
                Err(error)
            }
        }
    }

    pub fn create_buffer(&mut self, id: u32, usage: u32, size: u64) -> Result<(), EncodeError> {
        self.command(GpuOpcode::CreateBuffer, 16, |bytes| {
            push_u32(bytes, id);
            push_u32(bytes, usage);
            push_u64(bytes, size);
        })
    }

    pub fn write_buffer(&mut self, id: u32, offset: u64, data: &[u8]) -> Result<(), EncodeError> {
        let data_bytes = u32::try_from(data.len()).map_err(|_| EncodeError::FieldOutOfRange)?;
        let payload_bytes = padded_len(24, data.len())?;
        self.command(GpuOpcode::WriteBuffer, payload_bytes, |bytes| {
            push_u32(bytes, id);
            push_u32(bytes, 0);
            push_u64(bytes, offset);
            push_u32(bytes, data_bytes);
            push_u32(bytes, 0);
            bytes.extend_from_slice(data);
            pad4(bytes);
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_texture_2d(
        &mut self,
        id: u32,
        width: u32,
        height: u32,
        mip_level_count: u16,
        sample_count: u16,
        format: u16,
        usage: u32,
    ) -> Result<(), EncodeError> {
        self.command(GpuOpcode::CreateTexture, 24, |bytes| {
            push_u32(bytes, id);
            push_u32(bytes, width);
            push_u32(bytes, height);
            push_u16(bytes, mip_level_count);
            push_u16(bytes, sample_count);
            push_u16(bytes, format);
            bytes.push(wire::GpuTextureDimension::D2 as u8);
            bytes.push(0);
            push_u32(bytes, usage);
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn write_texture_2d(
        &mut self,
        id: u32,
        mip_level: u32,
        origin_x: u32,
        origin_y: u32,
        width: u32,
        height: u32,
        bytes_per_row: u32,
        rows_per_image: u32,
        data: &[u8],
    ) -> Result<(), EncodeError> {
        let data_bytes = u32::try_from(data.len()).map_err(|_| EncodeError::FieldOutOfRange)?;
        let payload_bytes = padded_len(44, data.len())?;
        self.command(GpuOpcode::WriteTexture, payload_bytes, |bytes| {
            for value in [
                id,
                mip_level,
                origin_x,
                origin_y,
                0,
                width,
                height,
                1,
                bytes_per_row,
                rows_per_image,
                data_bytes,
            ] {
                push_u32(bytes, value);
            }
            bytes.extend_from_slice(data);
            pad4(bytes);
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_texture_view(
        &mut self,
        id: u32,
        texture: u32,
        format: u16,
        aspect: u8,
        base_mip_level: u16,
        mip_level_count: u16,
        base_array_layer: u16,
        array_layer_count: u16,
    ) -> Result<(), EncodeError> {
        self.command(GpuOpcode::CreateTextureView, 20, |bytes| {
            push_u32(bytes, id);
            push_u32(bytes, texture);
            push_u16(bytes, format);
            bytes.push(wire::GpuTextureDimension::D2 as u8);
            bytes.push(aspect);
            push_u16(bytes, base_mip_level);
            push_u16(bytes, mip_level_count);
            push_u16(bytes, base_array_layer);
            push_u16(bytes, array_layer_count);
        })
    }

    pub fn create_sampler(
        &mut self,
        id: u32,
        descriptor: SamplerDescriptor,
    ) -> Result<(), EncodeError> {
        self.command(GpuOpcode::CreateSampler, 24, |bytes| {
            push_u32(bytes, id);
            bytes.extend_from_slice(&[
                descriptor.address_u,
                descriptor.address_v,
                descriptor.address_w,
                descriptor.mag_filter,
                descriptor.min_filter,
                descriptor.mipmap_filter,
                descriptor.compare,
                descriptor.max_anisotropy,
            ]);
            push_f32(bytes, descriptor.lod_min);
            push_f32(bytes, descriptor.lod_max);
            push_u32(bytes, 0);
        })
    }

    pub fn create_shader_wgsl(&mut self, id: u32, source: &str) -> Result<(), EncodeError> {
        let source_bytes = u32::try_from(source.len()).map_err(|_| EncodeError::FieldOutOfRange)?;
        let payload_bytes = padded_len(8, source.len())?;
        self.command(GpuOpcode::CreateShaderWgsl, payload_bytes, |bytes| {
            push_u32(bytes, id);
            push_u32(bytes, source_bytes);
            bytes.extend_from_slice(source.as_bytes());
            pad4(bytes);
        })
    }

    pub fn create_bind_group_layout(
        &mut self,
        id: u32,
        entries: &[BindGroupLayoutEntry],
    ) -> Result<(), EncodeError> {
        let entry_count = u32::try_from(entries.len()).map_err(|_| EncodeError::FieldOutOfRange)?;
        let payload_bytes = array_payload_len(8, entries.len(), 32)?;
        self.command(GpuOpcode::CreateBindGroupLayout, payload_bytes, |bytes| {
            push_u32(bytes, id);
            push_u32(bytes, entry_count);
            for entry in entries {
                push_u32(bytes, entry.binding);
                push_u32(bytes, entry.visibility);
                push_u16(bytes, entry.kind);
                push_u16(bytes, entry.flags);
                push_u32(bytes, 0);
                push_u64(bytes, entry.minimum_binding_size);
                push_u32(bytes, entry.parameter_0);
                push_u32(bytes, entry.parameter_1);
            }
        })
    }

    pub fn create_pipeline_layout(&mut self, id: u32, layouts: &[u32]) -> Result<(), EncodeError> {
        let layout_count =
            u32::try_from(layouts.len()).map_err(|_| EncodeError::FieldOutOfRange)?;
        let payload_bytes = array_payload_len(8, layouts.len(), 4)?;
        self.command(GpuOpcode::CreatePipelineLayout, payload_bytes, |bytes| {
            push_u32(bytes, id);
            push_u32(bytes, layout_count);
            for layout in layouts {
                push_u32(bytes, *layout);
            }
        })
    }

    pub fn create_bind_group(
        &mut self,
        id: u32,
        layout: u32,
        entries: &[BindGroupEntry],
    ) -> Result<(), EncodeError> {
        let entry_count = u32::try_from(entries.len()).map_err(|_| EncodeError::FieldOutOfRange)?;
        let payload_bytes = array_payload_len(12, entries.len(), 32)?;
        self.command(GpuOpcode::CreateBindGroup, payload_bytes, |bytes| {
            push_u32(bytes, id);
            push_u32(bytes, layout);
            push_u32(bytes, entry_count);
            for entry in entries {
                push_u32(bytes, entry.binding);
                push_u32(bytes, entry.resource);
                push_u16(bytes, entry.kind);
                push_u16(bytes, 0);
                push_u32(bytes, 0);
                push_u64(bytes, entry.offset);
                push_u64(bytes, entry.size);
            }
        })
    }

    pub fn create_render_pipeline(
        &mut self,
        descriptor: RenderPipelineDescriptor<'_>,
    ) -> Result<(), EncodeError> {
        let layout_count = u16::try_from(descriptor.vertex_layouts.len())
            .map_err(|_| EncodeError::FieldOutOfRange)?;
        let attribute_count = u16::try_from(descriptor.vertex_attributes.len())
            .map_err(|_| EncodeError::FieldOutOfRange)?;
        let target_count = u16::try_from(descriptor.color_targets.len())
            .map_err(|_| EncodeError::FieldOutOfRange)?;
        let payload_bytes = 40usize
            .checked_add(
                descriptor
                    .vertex_layouts
                    .len()
                    .checked_mul(16)
                    .ok_or(EncodeError::BatchTooLarge)?,
            )
            .and_then(|bytes| {
                descriptor
                    .vertex_attributes
                    .len()
                    .checked_mul(16)
                    .and_then(|next| bytes.checked_add(next))
            })
            .and_then(|bytes| {
                descriptor
                    .color_targets
                    .len()
                    .checked_mul(16)
                    .and_then(|next| bytes.checked_add(next))
            })
            .ok_or(EncodeError::BatchTooLarge)?;
        self.command(GpuOpcode::CreateRenderPipeline, payload_bytes, |bytes| {
            push_u32(bytes, descriptor.id);
            push_u32(bytes, descriptor.layout);
            push_u32(bytes, descriptor.shader);
            push_u16(bytes, layout_count);
            push_u16(bytes, attribute_count);
            push_u16(bytes, target_count);
            push_u16(bytes, descriptor.flags);
            push_u16(bytes, descriptor.depth_format);
            push_u16(bytes, descriptor.sample_count);
            bytes.extend_from_slice(&[
                descriptor.topology,
                descriptor.front_face,
                descriptor.cull_mode,
                descriptor.strip_index_format,
                descriptor.depth_compare,
            ]);
            bytes.extend_from_slice(&[0; 11]);
            for layout in descriptor.vertex_layouts {
                push_u64(bytes, layout.array_stride);
                bytes.push(layout.step_mode);
                bytes.extend_from_slice(&[0; 3]);
                push_u16(bytes, layout.first_attribute);
                push_u16(bytes, layout.attribute_count);
            }
            for attribute in descriptor.vertex_attributes {
                push_u16(bytes, attribute.format);
                push_u16(bytes, attribute.shader_location);
                push_u64(bytes, attribute.offset);
                push_u32(bytes, 0);
            }
            for target in descriptor.color_targets {
                push_u16(bytes, target.format);
                push_u16(bytes, target.write_mask);
                bytes.extend_from_slice(&[
                    target.color_operation,
                    target.color_source_factor,
                    target.color_destination_factor,
                    target.alpha_operation,
                    target.alpha_source_factor,
                    target.alpha_destination_factor,
                ]);
                bytes.extend_from_slice(&[0; 6]);
            }
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_render_pass(
        &mut self,
        color_view: u32,
        depth_view: u32,
        surface_generation: u32,
        flags: u32,
        clear_color: [f32; 4],
        clear_depth: f32,
    ) -> Result<(), EncodeError> {
        self.command(GpuOpcode::BeginRenderPass, 36, |bytes| {
            for value in [color_view, depth_view, surface_generation, flags] {
                push_u32(bytes, value);
            }
            for value in clear_color {
                push_f32(bytes, value);
            }
            push_f32(bytes, clear_depth);
        })
    }

    pub fn set_pipeline(&mut self, pipeline: u32) -> Result<(), EncodeError> {
        self.command(GpuOpcode::SetPipeline, 4, |bytes| push_u32(bytes, pipeline))
    }

    pub fn set_vertex_buffer(
        &mut self,
        slot: u32,
        buffer: u32,
        offset: u64,
        size: u64,
    ) -> Result<(), EncodeError> {
        self.command(GpuOpcode::SetVertexBuffer, 24, |bytes| {
            push_u32(bytes, slot);
            push_u32(bytes, buffer);
            push_u64(bytes, offset);
            push_u64(bytes, size);
        })
    }

    pub fn set_index_buffer(
        &mut self,
        buffer: u32,
        format: u32,
        offset: u64,
        size: u64,
    ) -> Result<(), EncodeError> {
        self.command(GpuOpcode::SetIndexBuffer, 24, |bytes| {
            push_u32(bytes, buffer);
            push_u32(bytes, format);
            push_u64(bytes, offset);
            push_u64(bytes, size);
        })
    }

    pub fn set_bind_group(
        &mut self,
        index: u32,
        bind_group: u32,
        dynamic_offsets: &[u32],
    ) -> Result<(), EncodeError> {
        let offset_count =
            u32::try_from(dynamic_offsets.len()).map_err(|_| EncodeError::FieldOutOfRange)?;
        let payload_bytes = array_payload_len(12, dynamic_offsets.len(), 4)?;
        self.command(GpuOpcode::SetBindGroup, payload_bytes, |bytes| {
            push_u32(bytes, index);
            push_u32(bytes, bind_group);
            push_u32(bytes, offset_count);
            for offset in dynamic_offsets {
                push_u32(bytes, *offset);
            }
        })
    }

    pub fn set_viewport(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        min_depth: f32,
        max_depth: f32,
    ) -> Result<(), EncodeError> {
        self.command(GpuOpcode::SetViewport, 24, |bytes| {
            for value in [x, y, width, height, min_depth, max_depth] {
                push_f32(bytes, value);
            }
        })
    }

    pub fn set_scissor_rect(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), EncodeError> {
        self.command(GpuOpcode::SetScissorRect, 16, |bytes| {
            for value in [x, y, width, height] {
                push_u32(bytes, value);
            }
        })
    }

    pub fn draw(
        &mut self,
        vertex_count: u32,
        instance_count: u32,
        first_vertex: u32,
        first_instance: u32,
    ) -> Result<(), EncodeError> {
        self.command(GpuOpcode::Draw, 16, |bytes| {
            for value in [vertex_count, instance_count, first_vertex, first_instance] {
                push_u32(bytes, value);
            }
        })
    }

    pub fn draw_indexed(
        &mut self,
        index_count: u32,
        instance_count: u32,
        first_index: u32,
        base_vertex: i32,
        first_instance: u32,
    ) -> Result<(), EncodeError> {
        self.command(GpuOpcode::DrawIndexed, 20, |bytes| {
            for value in [index_count, instance_count, first_index] {
                push_u32(bytes, value);
            }
            bytes.extend_from_slice(&base_vertex.to_le_bytes());
            push_u32(bytes, first_instance);
        })
    }

    pub fn end_render_pass(&mut self) -> Result<(), EncodeError> {
        self.command(GpuOpcode::EndRenderPass, 0, |_| {})
    }

    pub fn destroy_resource(&mut self, id: u32) -> Result<(), EncodeError> {
        self.command(GpuOpcode::DestroyResource, 4, |bytes| push_u32(bytes, id))
    }

    fn command(
        &mut self,
        opcode: GpuOpcode,
        payload_bytes: usize,
        encode: impl FnOnce(&mut Vec<u8>),
    ) -> Result<(), EncodeError> {
        if self.command_count >= MAX_GPU_COMMANDS {
            return Err(EncodeError::TooManyCommands);
        }
        if !payload_bytes.is_multiple_of(4) {
            return Err(EncodeError::FieldOutOfRange);
        }
        let command_bytes = GPU_COMMAND_HEADER_BYTES
            .checked_add(payload_bytes)
            .ok_or(EncodeError::BatchTooLarge)?;
        let total_bytes = self
            .bytes
            .len()
            .checked_add(command_bytes)
            .filter(|bytes| *bytes <= MAX_GPU_BATCH_BYTES)
            .ok_or(EncodeError::BatchTooLarge)?;
        let command_bytes_u32 =
            u32::try_from(command_bytes).map_err(|_| EncodeError::BatchTooLarge)?;
        self.bytes.try_reserve_exact(command_bytes)?;
        let start = self.bytes.len();
        push_u16(&mut self.bytes, opcode as u16);
        push_u16(&mut self.bytes, 0);
        push_u32(&mut self.bytes, command_bytes_u32);
        encode(&mut self.bytes);
        if self.bytes.len() != start + command_bytes {
            self.bytes.truncate(start);
            return Err(EncodeError::DescriptorLengthMismatch);
        }
        self.command_count += 1;
        self.bytes[8..12].copy_from_slice(&(total_bytes as u32).to_le_bytes());
        self.bytes[12..16].copy_from_slice(&self.command_count.to_le_bytes());
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilitiesError {
    Truncated,
    InvalidMagic,
    UnsupportedVersion,
    ReservedField,
    LengthMismatch,
    InvalidSurface,
    InvalidLimitTable,
}

#[derive(Clone, Copy, Debug)]
pub struct Capabilities<'a> {
    pub surface_format: u16,
    pub physical_width: u32,
    pub physical_height: u32,
    pub logical_width: u32,
    pub logical_height: u32,
    pub scale_factor: f32,
    pub surface_generation: u32,
    pub device_generation: u32,
    limits: &'a [u8],
}

impl Capabilities<'_> {
    pub fn limit(&self, key: wire::GpuCapabilityKey) -> Option<u64> {
        self.limits
            .chunks_exact(wire::GPU_CAPABILITY_ENTRY_BYTES)
            .find_map(|entry| (read_u16(entry, 0) == key as u16).then(|| read_u64(entry, 4)))
    }
}

pub fn parse_capabilities(bytes: &[u8]) -> Result<Capabilities<'_>, CapabilitiesError> {
    if bytes.len() < wire::GPU_CAPABILITIES_HEADER_BYTES {
        return Err(CapabilitiesError::Truncated);
    }
    if bytes[..4] != wire::GPU_CAPABILITIES_MAGIC {
        return Err(CapabilitiesError::InvalidMagic);
    }
    if read_u16(bytes, 4) != wire::GPU_WIRE_VERSION {
        return Err(CapabilitiesError::UnsupportedVersion);
    }
    if read_u16(bytes, 6) != 0
        || read_u16(bytes, 14) != 0
        || read_u32(bytes, 48) != 0
        || read_u32(bytes, 52) != 0
    {
        return Err(CapabilitiesError::ReservedField);
    }
    if read_u32(bytes, 8) as usize != bytes.len() {
        return Err(CapabilitiesError::LengthMismatch);
    }
    let limit_count = read_u32(bytes, 44) as usize;
    let expected = limit_count
        .checked_mul(wire::GPU_CAPABILITY_ENTRY_BYTES)
        .and_then(|entries| wire::GPU_CAPABILITIES_HEADER_BYTES.checked_add(entries))
        .ok_or(CapabilitiesError::InvalidLimitTable)?;
    if expected != bytes.len() {
        return Err(CapabilitiesError::InvalidLimitTable);
    }
    let surface_format = read_u16(bytes, 12);
    let physical_width = read_u32(bytes, 16);
    let physical_height = read_u32(bytes, 20);
    let logical_width = read_u32(bytes, 24);
    let logical_height = read_u32(bytes, 28);
    let scale_factor = f32::from_bits(read_u32(bytes, 32));
    let surface_generation = read_u32(bytes, 36);
    let device_generation = read_u32(bytes, 40);
    if surface_format == 0
        || physical_width == 0
        || physical_height == 0
        || logical_width == 0
        || logical_height == 0
        || !scale_factor.is_finite()
        || scale_factor <= 0.0
        || surface_generation == 0
        || device_generation == 0
    {
        return Err(CapabilitiesError::InvalidSurface);
    }
    let limits = &bytes[wire::GPU_CAPABILITIES_HEADER_BYTES..];
    let mut previous_key = 0;
    for entry in limits.chunks_exact(wire::GPU_CAPABILITY_ENTRY_BYTES) {
        let key = read_u16(entry, 0);
        if key <= previous_key || read_u16(entry, 2) != 0 || read_u32(entry, 12) != 0 {
            return Err(CapabilitiesError::InvalidLimitTable);
        }
        previous_key = key;
    }
    Ok(Capabilities {
        surface_format,
        physical_width,
        physical_height,
        logical_width,
        logical_height,
        scale_factor,
        surface_generation,
        device_generation,
        limits,
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        bytes[offset..offset + 2]
            .try_into()
            .expect("validated field offset"),
    )
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated field offset"),
    )
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("validated field offset"),
    )
}

fn padded_len(header_bytes: usize, inline_bytes: usize) -> Result<usize, EncodeError> {
    header_bytes
        .checked_add(inline_bytes)
        .and_then(|bytes| bytes.checked_add(3))
        .map(|bytes| bytes & !3)
        .ok_or(EncodeError::BatchTooLarge)
}

fn array_payload_len(
    header_bytes: usize,
    count: usize,
    stride: usize,
) -> Result<usize, EncodeError> {
    count
        .checked_mul(stride)
        .and_then(|bytes| header_bytes.checked_add(bytes))
        .ok_or(EncodeError::BatchTooLarge)
}

fn pad4(bytes: &mut Vec<u8>) {
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_f32(bytes: &mut Vec<u8>, value: f32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

#[cfg(feature = "pvm")]
pub mod pvm {
    use super::BatchEncoder;

    #[polkavm_derive::polkavm_import]
    extern "C" {
        fn epoca_gpu_capabilities(pointer: u32, capacity: u32) -> i32;
        fn epoca_gpu_submit(pointer: u32, length: u32) -> i32;
        fn epoca_gpu_receive(pointer: u32, capacity: u32) -> i32;
    }

    pub fn capabilities(bytes: &mut [u8]) -> i32 {
        unsafe { epoca_gpu_capabilities(bytes.as_mut_ptr() as u32, bytes.len() as u32) }
    }

    pub fn submit(batch: &BatchEncoder) -> i32 {
        unsafe {
            epoca_gpu_submit(
                batch.as_bytes().as_ptr() as u32,
                batch.as_bytes().len() as u32,
            )
        }
    }

    pub fn receive(bytes: &mut [u8]) -> i32 {
        unsafe { epoca_gpu_receive(bytes.as_mut_ptr() as u32, bytes.len() as u32) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use wire::{
        decode_gpu_batch, GpuTextureFormat, GPU_BUFFER_USAGE_COPY_DST, GPU_BUFFER_USAGE_VERTEX,
    };

    #[test]
    fn encodes_a_golden_multi_command_batch() {
        let mut encoder = BatchEncoder::new(7).unwrap();
        encoder
            .create_buffer(
                0x0010_0001,
                GPU_BUFFER_USAGE_COPY_DST | GPU_BUFFER_USAGE_VERTEX,
                32,
            )
            .unwrap();
        encoder.write_buffer(0x0010_0001, 4, &[1, 2, 3]).unwrap();
        encoder.destroy_resource(0x0010_0001).unwrap();

        let batch =
            decode_gpu_batch(encoder.as_bytes()).expect("SDK output must pass host decoding");
        assert_eq!(batch.sequence(), 7);
        assert_eq!(batch.command_count(), 3);
        assert_eq!(
            batch
                .commands()
                .map(|command| command.opcode)
                .collect::<Vec<_>>(),
            [
                GpuOpcode::CreateBuffer,
                GpuOpcode::WriteBuffer,
                GpuOpcode::DestroyResource,
            ]
        );
        assert_eq!(&encoder.as_bytes()[..4], b"EPG1");
        assert_eq!(
            u32::from_le_bytes(encoder.as_bytes()[8..12].try_into().unwrap()) as usize,
            encoder.as_bytes().len()
        );
    }

    #[test]
    fn encodes_pipeline_array_lengths_without_rust_layout_coupling() {
        let mut encoder = BatchEncoder::new(1).unwrap();
        encoder
            .create_render_pipeline(RenderPipelineDescriptor {
                id: 9,
                layout: 8,
                shader: 7,
                flags: wire::GPU_PIPELINE_DEPTH_WRITE,
                depth_format: GpuTextureFormat::Depth24Plus as u16,
                sample_count: 1,
                topology: wire::GpuPrimitiveTopology::TriangleList as u8,
                front_face: wire::GpuFrontFace::Ccw as u8,
                cull_mode: wire::GpuCullMode::Back as u8,
                strip_index_format: 0,
                depth_compare: wire::GpuCompareFunction::Less as u8,
                vertex_layouts: &[VertexBufferLayout {
                    array_stride: 20,
                    step_mode: wire::GpuVertexStepMode::Vertex as u8,
                    first_attribute: 0,
                    attribute_count: 2,
                }],
                vertex_attributes: &[
                    VertexAttribute {
                        format: wire::GpuVertexFormat::Float32x3 as u16,
                        shader_location: 0,
                        offset: 0,
                    },
                    VertexAttribute {
                        format: wire::GpuVertexFormat::Float32x2 as u16,
                        shader_location: 1,
                        offset: 12,
                    },
                ],
                color_targets: &[ColorTarget {
                    format: GpuTextureFormat::Bgra8Unorm as u16,
                    write_mask: 15,
                    color_operation: wire::GpuBlendOperation::Add as u8,
                    color_source_factor: wire::GpuBlendFactor::One as u8,
                    color_destination_factor: wire::GpuBlendFactor::Zero as u8,
                    alpha_operation: wire::GpuBlendOperation::Add as u8,
                    alpha_source_factor: wire::GpuBlendFactor::One as u8,
                    alpha_destination_factor: wire::GpuBlendFactor::Zero as u8,
                }],
            })
            .unwrap();
        let batch = decode_gpu_batch(encoder.as_bytes()).unwrap();
        let command = batch.commands().next().unwrap();
        assert_eq!(command.payload.len(), 104);
        assert_eq!(&command.payload[12..18], &[1, 0, 2, 0, 1, 0]);
    }

    #[test]
    fn parses_sorted_capability_limits() {
        let mut bytes =
            vec![0; wire::GPU_CAPABILITIES_HEADER_BYTES + wire::GPU_CAPABILITY_ENTRY_BYTES];
        bytes[..4].copy_from_slice(&wire::GPU_CAPABILITIES_MAGIC);
        bytes[4..6].copy_from_slice(&wire::GPU_WIRE_VERSION.to_le_bytes());
        let total_bytes = bytes.len() as u32;
        bytes[8..12].copy_from_slice(&total_bytes.to_le_bytes());
        bytes[12..14].copy_from_slice(&(GpuTextureFormat::Bgra8Unorm as u16).to_le_bytes());
        bytes[16..20].copy_from_slice(&960u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&640u32.to_le_bytes());
        bytes[24..28].copy_from_slice(&960u32.to_le_bytes());
        bytes[28..32].copy_from_slice(&640u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&1.0f32.to_le_bytes());
        bytes[36..40].copy_from_slice(&4u32.to_le_bytes());
        bytes[40..44].copy_from_slice(&2u32.to_le_bytes());
        bytes[44..48].copy_from_slice(&1u32.to_le_bytes());
        let entry = wire::GPU_CAPABILITIES_HEADER_BYTES;
        bytes[entry..entry + 2]
            .copy_from_slice(&(wire::GpuCapabilityKey::MaxBufferSize as u16).to_le_bytes());
        bytes[entry + 4..entry + 12].copy_from_slice(&(16u64 * 1024 * 1024).to_le_bytes());

        let capabilities = parse_capabilities(&bytes).unwrap();
        assert_eq!(
            capabilities.surface_format,
            GpuTextureFormat::Bgra8Unorm as u16
        );
        assert_eq!(
            (capabilities.physical_width, capabilities.physical_height),
            (960, 640)
        );
        assert_eq!(
            capabilities.limit(wire::GpuCapabilityKey::MaxBufferSize),
            Some(16 * 1024 * 1024)
        );

        bytes[entry + 2] = 1;
        assert_eq!(
            parse_capabilities(&bytes).unwrap_err(),
            CapabilitiesError::InvalidLimitTable
        );
    }

    #[test]
    fn rejects_zero_sequence_and_oversized_inline_upload() {
        assert_eq!(
            BatchEncoder::new(0).unwrap_err(),
            EncodeError::EmptySequence
        );
        let mut encoder = BatchEncoder::new(1).unwrap();
        let upload = vec![0; MAX_GPU_BATCH_BYTES];
        assert_eq!(
            encoder.write_buffer(1, 0, &upload),
            Err(EncodeError::BatchTooLarge)
        );
        assert_eq!(encoder.command_count(), 0);
        assert_eq!(encoder.as_bytes().len(), GPU_BATCH_HEADER_BYTES);
    }
}
