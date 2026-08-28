/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

const DEFAULT_PIXELS_PER_POINT: f32 = 2.0;
const DEFAULT_WIDTH: u32 = 1_920;
const DEFAULT_HEIGHT: u32 = 1_280;
const MAX_SURFACE_SIZE: u32 = 4_096;
const PIXELS_PER_POINT_UNITS: f32 = 32.0;
const INPUT_EVENT_BYTES: usize = 8;
const INPUT_BATCH_BYTES: usize = INPUT_EVENT_BYTES * 64;
const MODIFIER_CTRL_LEFT: u8 = 1 << 0;
const MODIFIER_CTRL_RIGHT: u8 = 1 << 1;
const MODIFIER_SHIFT_LEFT: u8 = 1 << 2;
const MODIFIER_SHIFT_RIGHT: u8 = 1 << 3;
const MODIFIER_ALT_LEFT: u8 = 1 << 4;
const MODIFIER_ALT_RIGHT: u8 = 1 << 5;

#[polkavm_derive::polkavm_import]
extern "C" {
    fn host_tri2d_submit(pointer: u32, length: u32) -> u32;
    fn host_poll_input(pointer: u32, capacity: u32) -> u32;
    fn host_time_ms() -> u64;
    fn host_log(pointer: u32, length: u32);
}

pub struct InputState {
    modifiers: Modifiers,
    modifier_keys: u8,
    pointer: Pos2,
    surface_width: u32,
    surface_height: u32,
    pixels_per_point: f32,
}

impl InputState {
    pub const fn new() -> Self {
        Self {
            modifiers: Modifiers::NONE,
            modifier_keys: 0,
            pointer: Pos2::ZERO,
            surface_width: DEFAULT_WIDTH,
            surface_height: DEFAULT_HEIGHT,
            pixels_per_point: DEFAULT_PIXELS_PER_POINT,
        }
    }

    pub fn gather(&mut self) -> RawInput {
        let mut input = RawInput {
            time: Some(time_ms() as f64 / 1_000.0),
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
                self.push_event(event, &mut input.events);
            }
            if written < bytes.len() {
                break;
            }
        }
        input.screen_rect = Some(Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(
                self.surface_width as f32 / self.pixels_per_point,
                self.surface_height as f32 / self.pixels_per_point,
            ),
        ));
        input
            .viewports
            .get_mut(&input.viewport_id)
            .expect("the root egui viewport exists")
            .native_pixels_per_point = Some(self.pixels_per_point);
        input.modifiers = self.modifiers;
        input
    }

    pub fn surface_size(&self) -> (u32, u32) {
        (self.surface_width, self.surface_height)
    }

    fn push_event(&mut self, bytes: &[u8], events: &mut alloc::vec::Vec<Event>) {
        let event_type = bytes[0];
        let code = bytes[1];
        let position = Pos2::new(
            u16::from_le_bytes([bytes[2], bytes[3]]) as f32 / self.pixels_per_point,
            u16::from_le_bytes([bytes[4], bytes[5]]) as f32 / self.pixels_per_point,
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
            7 => {
                let width = u16::from_le_bytes([bytes[2], bytes[3]]) as u32;
                let height = u16::from_le_bytes([bytes[4], bytes[5]]) as u32;
                let pixels_per_point = code as f32 / PIXELS_PER_POINT_UNITS;
                if width > 0
                    && height > 0
                    && width <= MAX_SURFACE_SIZE
                    && height <= MAX_SURFACE_SIZE
                    && (1.0 / PIXELS_PER_POINT_UNITS..=3.0).contains(&pixels_per_point)
                {
                    self.surface_width = width;
                    self.surface_height = height;
                    self.pixels_per_point = pixels_per_point;
                }
            }
            _ => {}
        }
    }

    fn update_modifier(&mut self, code: u8, pressed: bool) {
        let key = match code {
            0xe0 => MODIFIER_CTRL_LEFT,
            0xe4 => MODIFIER_CTRL_RIGHT,
            0xe1 => MODIFIER_SHIFT_LEFT,
            0xe5 => MODIFIER_SHIFT_RIGHT,
            0xe2 => MODIFIER_ALT_LEFT,
            0xe6 => MODIFIER_ALT_RIGHT,
            _ => return,
        };
        if pressed {
            self.modifier_keys |= key;
        } else {
            self.modifier_keys &= !key;
        }
        self.modifiers.ctrl = self.modifier_keys & (MODIFIER_CTRL_LEFT | MODIFIER_CTRL_RIGHT) != 0;
        self.modifiers.shift =
            self.modifier_keys & (MODIFIER_SHIFT_LEFT | MODIFIER_SHIFT_RIGHT) != 0;
        self.modifiers.alt = self.modifier_keys & (MODIFIER_ALT_LEFT | MODIFIER_ALT_RIGHT) != 0;
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

pub fn time_ms() -> u64 {
    unsafe { host_time_ms() }
}

pub fn submit_tri2d(bytes: &[u8]) -> bool {
    unsafe { host_tri2d_submit(bytes.as_ptr() as u32, bytes.len() as u32) == 0 }
}

pub fn log(message: &[u8]) {
    unsafe { host_log(message.as_ptr() as u32, message.len() as u32) }
}
