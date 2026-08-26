/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![no_std]

use core::fmt;

pub const MAGIC: [u8; 4] = *b"EPM1";
pub const VERSION: u16 = 1;
pub const HEADER_BYTES: usize = 80;
pub const VERTEX_STRIDE: usize = 32;
pub const INDEX_BYTES: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MeshError {
    Truncated,
    InvalidMagic,
    UnsupportedVersion,
    InvalidHeader,
    InvalidCounts,
    InvalidLayout,
    InvalidMaterial,
    InvalidBounds,
    InvalidVertex,
    InvalidIndex,
}

impl fmt::Display for MeshError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Truncated => "mesh is truncated",
            Self::InvalidMagic => "mesh magic is invalid",
            Self::UnsupportedVersion => "mesh version is unsupported",
            Self::InvalidHeader => "mesh header is invalid",
            Self::InvalidCounts => "mesh counts are invalid",
            Self::InvalidLayout => "mesh byte layout is invalid",
            Self::InvalidMaterial => "mesh material is invalid",
            Self::InvalidBounds => "mesh bounds are invalid",
            Self::InvalidVertex => "mesh vertex data is invalid",
            Self::InvalidIndex => "mesh index data is invalid",
        })
    }
}

impl core::error::Error for MeshError {}

#[derive(Clone, Copy, Debug)]
pub struct Mesh<'a> {
    pub vertex_count: u32,
    pub index_count: u32,
    pub base_color: [f32; 4],
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
    pub vertices: &'a [u8],
    pub indices: &'a [u8],
}

pub fn parse(bytes: &[u8]) -> Result<Mesh<'_>, MeshError> {
    if bytes.len() < HEADER_BYTES {
        return Err(MeshError::Truncated);
    }
    if bytes[..4] != MAGIC {
        return Err(MeshError::InvalidMagic);
    }
    if read_u16(bytes, 4) != VERSION {
        return Err(MeshError::UnsupportedVersion);
    }
    if read_u16(bytes, 6) as usize != HEADER_BYTES || read_u32(bytes, 76) != 0 {
        return Err(MeshError::InvalidHeader);
    }

    let vertex_count = read_u32(bytes, 8);
    let index_count = read_u32(bytes, 12);
    if vertex_count == 0 || index_count == 0 || index_count % 3 != 0 {
        return Err(MeshError::InvalidCounts);
    }
    if read_u32(bytes, 16) as usize != VERTEX_STRIDE
        || read_u32(bytes, 20) as usize != INDEX_BYTES
        || read_u32(bytes, 24) as usize != HEADER_BYTES
    {
        return Err(MeshError::InvalidLayout);
    }

    let vertex_bytes = usize::try_from(vertex_count)
        .ok()
        .and_then(|count| count.checked_mul(VERTEX_STRIDE))
        .ok_or(MeshError::InvalidLayout)?;
    let index_offset = HEADER_BYTES
        .checked_add(vertex_bytes)
        .ok_or(MeshError::InvalidLayout)?;
    let index_bytes = usize::try_from(index_count)
        .ok()
        .and_then(|count| count.checked_mul(INDEX_BYTES))
        .ok_or(MeshError::InvalidLayout)?;
    let expected_bytes = index_offset
        .checked_add(index_bytes)
        .ok_or(MeshError::InvalidLayout)?;
    if read_u32(bytes, 28) as usize != index_offset || bytes.len() != expected_bytes {
        return Err(MeshError::InvalidLayout);
    }

    let base_color = read_f32_array::<4>(bytes, 32);
    if base_color
        .iter()
        .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
    {
        return Err(MeshError::InvalidMaterial);
    }
    let bounds_min = read_f32_array::<3>(bytes, 48);
    let bounds_max = read_f32_array::<3>(bytes, 64);
    if bounds_min.iter().zip(bounds_max).any(|(minimum, maximum)| {
        !minimum.is_finite() || !maximum.is_finite() || *minimum > maximum
    }) {
        return Err(MeshError::InvalidBounds);
    }

    let vertices = &bytes[HEADER_BYTES..index_offset];
    for vertex in vertices.chunks_exact(VERTEX_STRIDE) {
        let mut values = [0.0; 8];
        for (index, value) in values.iter_mut().enumerate() {
            *value = read_f32(vertex, index * 4);
        }
        let normal_length_squared =
            values[3] * values[3] + values[4] * values[4] + values[5] * values[5];
        if values.iter().any(|value| !value.is_finite())
            || !normal_length_squared.is_finite()
            || normal_length_squared <= f32::EPSILON
        {
            return Err(MeshError::InvalidVertex);
        }
    }

    let indices = &bytes[index_offset..];
    if indices
        .chunks_exact(INDEX_BYTES)
        .any(|index| read_u32(index, 0) >= vertex_count)
    {
        return Err(MeshError::InvalidIndex);
    }

    Ok(Mesh {
        vertex_count,
        index_count,
        base_color,
        bounds_min,
        bounds_max,
        vertices,
        indices,
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        bytes[offset..offset + 2]
            .try_into()
            .expect("validated field"),
    )
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated field"),
    )
}

fn read_f32(bytes: &[u8], offset: usize) -> f32 {
    f32::from_bits(read_u32(bytes, offset))
}

fn read_f32_array<const N: usize>(bytes: &[u8], offset: usize) -> [f32; N] {
    core::array::from_fn(|index| read_f32(bytes, offset + index * 4))
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::vec;
    use std::vec::Vec;

    fn mesh_bytes() -> Vec<u8> {
        let mut bytes = vec![0; HEADER_BYTES];
        bytes[..4].copy_from_slice(&MAGIC);
        bytes[4..6].copy_from_slice(&VERSION.to_le_bytes());
        bytes[6..8].copy_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
        bytes[8..12].copy_from_slice(&3u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&3u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&(VERTEX_STRIDE as u32).to_le_bytes());
        bytes[20..24].copy_from_slice(&(INDEX_BYTES as u32).to_le_bytes());
        bytes[24..28].copy_from_slice(&(HEADER_BYTES as u32).to_le_bytes());
        bytes[28..32].copy_from_slice(&((HEADER_BYTES + 3 * VERTEX_STRIDE) as u32).to_le_bytes());
        for (index, value) in [0.8f32, 0.6, 0.4, 1.0].into_iter().enumerate() {
            bytes[32 + index * 4..36 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        for (index, value) in [-1.0f32, -1.0, 0.0].into_iter().enumerate() {
            bytes[48 + index * 4..52 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        for (index, value) in [1.0f32, 1.0, 0.0].into_iter().enumerate() {
            bytes[64 + index * 4..68 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        for position in [[-1.0f32, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]] {
            for value in [
                position[0],
                position[1],
                position[2],
                0.0,
                0.0,
                1.0,
                0.0,
                0.0,
            ] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        for index in [0u32, 1, 2] {
            bytes.extend_from_slice(&index.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn parses_valid_mesh_without_copying_payloads() {
        let bytes = mesh_bytes();
        let mesh = parse(&bytes).unwrap();
        assert_eq!(mesh.vertex_count, 3);
        assert_eq!(mesh.index_count, 3);
        assert_eq!(mesh.base_color, [0.8, 0.6, 0.4, 1.0]);
        assert_eq!(mesh.vertices.as_ptr(), bytes[HEADER_BYTES..].as_ptr());
    }

    #[test]
    fn rejects_out_of_range_indices() {
        let mut bytes = mesh_bytes();
        let offset = HEADER_BYTES + 3 * VERTEX_STRIDE;
        bytes[offset..offset + 4].copy_from_slice(&3u32.to_le_bytes());
        assert_eq!(parse(&bytes).unwrap_err(), MeshError::InvalidIndex);
    }

    #[test]
    fn rejects_non_finite_vertices() {
        let mut bytes = mesh_bytes();
        bytes[HEADER_BYTES..HEADER_BYTES + 4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(parse(&bytes).unwrap_err(), MeshError::InvalidVertex);
    }
}
