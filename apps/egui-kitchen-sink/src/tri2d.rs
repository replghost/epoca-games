/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use alloc::vec::Vec;
use egui::epaint::{ClippedPrimitive, ImageData, Primitive, TextureId};
use egui::{Rect, TexturesDelta};

use crate::abi::submit_tri2d;

const MAGIC: &[u8; 4] = b"ETD1";
const VERSION: u16 = 1;
const HEADER_BYTES: u16 = 24;
const VERTEX_BYTES: usize = 20;
const MAX_STREAM_BYTES: usize = 8 * 1024 * 1024;
const FIXED_SCALE: f32 = 65_536.0;
const OPCODE_TEXTURE_CREATE: u8 = 1;
const OPCODE_TEXTURE_UPDATE: u8 = 2;
const OPCODE_TEXTURE_DESTROY: u8 = 3;
const OPCODE_DRAW: u8 = 4;
const OPCODE_PRESENT: u8 = 5;
const CLEAR_RGBA: u32 = u32::from_le_bytes([16, 21, 31, 255]);

#[derive(Clone)]
struct TextureBinding {
    id: TextureId,
    handle: u32,
    size: [usize; 2],
}

#[derive(Clone, Copy, Default)]
pub struct MeshStats {
    pub bytes: usize,
    pub draws: u32,
    pub vertices: u32,
    pub indices: u32,
}

pub struct Tri2dRenderer {
    textures: Vec<TextureBinding>,
    next_handle: u32,
    last_stats: MeshStats,
}

impl Tri2dRenderer {
    pub const fn new() -> Self {
        Self {
            textures: Vec::new(),
            next_handle: 1,
            last_stats: MeshStats {
                bytes: 0,
                draws: 0,
                vertices: 0,
                indices: 0,
            },
        }
    }

    pub fn render(
        &mut self,
        textures_delta: TexturesDelta,
        primitives: &[ClippedPrimitive],
        pixels_per_point: f32,
        surface_size: (u32, u32),
    ) -> bool {
        let mut stream = Stream::new(surface_size.0, surface_size.1);
        let mut textures = self.textures.clone();
        let mut next_handle = self.next_handle;

        for (id, delta) in textures_delta.set {
            let ImageData::Color(image) = delta.image;
            let Some(pixel_bytes) = image
                .size
                .iter()
                .copied()
                .try_fold(4usize, usize::checked_mul)
            else {
                return false;
            };
            if pixel_bytes != image.pixels.len().saturating_mul(4) {
                return false;
            }

            if let Some(position) = delta.pos {
                let Some(binding) = textures.iter().find(|binding| binding.id == id) else {
                    return false;
                };
                if position[0]
                    .checked_add(image.size[0])
                    .is_none_or(|right| right > binding.size[0])
                    || position[1]
                        .checked_add(image.size[1])
                        .is_none_or(|bottom| bottom > binding.size[1])
                {
                    return false;
                }
                let payload = 24usize.saturating_add(pixel_bytes);
                stream.command(OPCODE_TEXTURE_UPDATE, payload, |stream| {
                    stream.u32(binding.handle);
                    stream.u32(position[0] as u32);
                    stream.u32(position[1] as u32);
                    stream.u32(image.size[0] as u32);
                    stream.u32(image.size[1] as u32);
                    stream.u32(pixel_bytes as u32);
                    stream.colors(&image.pixels);
                });
                continue;
            }

            if let Some(index) = textures.iter().position(|binding| binding.id == id) {
                let old = textures.swap_remove(index);
                stream.command(OPCODE_TEXTURE_DESTROY, 4, |stream| {
                    stream.u32(old.handle);
                });
            }
            let handle = next_handle;
            let Some(following_handle) = next_handle.checked_add(1).filter(|handle| *handle != 0)
            else {
                return false;
            };
            next_handle = following_handle;
            let payload = 20usize.saturating_add(pixel_bytes);
            stream.command(OPCODE_TEXTURE_CREATE, payload, |stream| {
                stream.u32(handle);
                stream.u32(image.size[0] as u32);
                stream.u32(image.size[1] as u32);
                stream.u32(1);
                stream.u32(pixel_bytes as u32);
                stream.colors(&image.pixels);
            });
            textures.push(TextureBinding {
                id,
                handle,
                size: image.size,
            });
        }

        for primitive in primitives {
            let Primitive::Mesh(mesh) = &primitive.primitive else {
                continue;
            };
            let Some(binding) = textures
                .iter()
                .find(|binding| binding.id == mesh.texture_id)
            else {
                return false;
            };
            let Some(clip) = clip_rect(
                primitive.clip_rect,
                pixels_per_point,
                surface_size.0,
                surface_size.1,
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
                return false;
            }
            let Some(payload) = mesh
                .vertices
                .len()
                .checked_mul(VERTEX_BYTES)
                .and_then(|bytes| {
                    mesh.indices
                        .len()
                        .checked_mul(4)
                        .and_then(|indices| bytes.checked_add(indices))
                })
                .and_then(|bytes| bytes.checked_add(28))
            else {
                return false;
            };
            stream.command(OPCODE_DRAW, payload, |stream| {
                stream.u32(binding.handle);
                stream.u32(clip[0]);
                stream.u32(clip[1]);
                stream.u32(clip[2]);
                stream.u32(clip[3]);
                stream.u32(mesh.vertices.len() as u32);
                stream.u32(mesh.indices.len() as u32);
                for vertex in &mesh.vertices {
                    stream.i32(fixed(vertex.pos.x * pixels_per_point));
                    stream.i32(fixed(vertex.pos.y * pixels_per_point));
                    stream.i32(fixed(vertex.uv.x));
                    stream.i32(fixed(vertex.uv.y));
                    stream.bytes.extend_from_slice(&vertex.color.to_array());
                }
                for index in &mesh.indices {
                    stream.u32(*index);
                }
            });
            stream.stats.draws = stream.stats.draws.saturating_add(1);
            stream.stats.vertices = stream
                .stats
                .vertices
                .saturating_add(mesh.vertices.len() as u32);
            stream.stats.indices = stream
                .stats
                .indices
                .saturating_add(mesh.indices.len() as u32);
        }

        for id in textures_delta.free {
            let Some(index) = textures.iter().position(|binding| binding.id == id) else {
                return false;
            };
            let binding = textures.swap_remove(index);
            stream.command(OPCODE_TEXTURE_DESTROY, 4, |stream| {
                stream.u32(binding.handle);
            });
        }
        stream.command(OPCODE_PRESENT, 0, |_| {});
        stream.finish();
        if stream.bytes.len() > MAX_STREAM_BYTES || !submit_tri2d(&stream.bytes) {
            return false;
        }

        stream.stats.bytes = stream.bytes.len();
        self.textures = textures;
        self.next_handle = next_handle;
        self.last_stats = stream.stats;
        true
    }

    pub fn last_stats(&self) -> MeshStats {
        self.last_stats
    }
}

fn clip_rect(
    rect: Rect,
    pixels_per_point: f32,
    surface_width: u32,
    surface_height: u32,
) -> Option<[u32; 4]> {
    let min_x = to_pixel_floor(rect.min.x * pixels_per_point).clamp(0, surface_width as i32);
    let min_y = to_pixel_floor(rect.min.y * pixels_per_point).clamp(0, surface_height as i32);
    let max_x = to_pixel_ceil(rect.max.x * pixels_per_point).clamp(min_x, surface_width as i32);
    let max_y = to_pixel_ceil(rect.max.y * pixels_per_point).clamp(min_y, surface_height as i32);
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

fn fixed(value: f32) -> i32 {
    let scaled = value * FIXED_SCALE;
    if scaled <= i32::MIN as f32 {
        i32::MIN
    } else if scaled >= i32::MAX as f32 {
        i32::MAX
    } else {
        scaled as i32
    }
}

struct Stream {
    bytes: Vec<u8>,
    command_count: u32,
    stats: MeshStats,
}

impl Stream {
    fn new(width: u32, height: u32) -> Self {
        let mut stream = Self {
            bytes: Vec::new(),
            command_count: 0,
            stats: MeshStats::default(),
        };
        stream.bytes.extend_from_slice(MAGIC);
        stream.u16(VERSION);
        stream.u16(HEADER_BYTES);
        stream.u32(width);
        stream.u32(height);
        stream.u32(0);
        stream.u32(CLEAR_RGBA);
        stream
    }

    fn command(&mut self, opcode: u8, payload_length: usize, write: impl FnOnce(&mut Self)) {
        self.bytes.push(opcode);
        self.bytes.push(0);
        self.u16(0);
        self.u32(payload_length as u32);
        let payload_start = self.bytes.len();
        write(self);
        if self.bytes.len() != payload_start.saturating_add(payload_length) {
            self.bytes
                .truncate(payload_start.saturating_add(payload_length));
        }
        self.command_count = self.command_count.saturating_add(1);
    }

    fn finish(&mut self) {
        self.bytes[16..20].copy_from_slice(&self.command_count.to_le_bytes());
    }

    fn colors(&mut self, colors: &[egui::Color32]) {
        for color in colors {
            self.bytes.extend_from_slice(&color.to_array());
        }
    }

    fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }
}
