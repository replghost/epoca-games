/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![no_std]
#![allow(static_mut_refs)]
#![feature(alloc_error_handler)]

extern crate alloc;

mod abi;
mod tri2d;

use alloc::format;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::NonNull;
use egui::{Color32, Context, Stroke};
use egui_demo_lib::DemoWindows;
use linked_list_allocator::Heap;

use abi::{log, time_ms, InputState};
use tri2d::Tri2dRenderer;

const INITIAL_HEAP_BYTES: usize = 64 * 1024;
const HEAP_GROWTH_BYTES: usize = 64 * 1024;
const MAX_HEAP_BYTES: usize = 32 * 1024 * 1024;

polkavm_derive::min_stack_size!(4 * 1024 * 1024);

#[global_allocator]
static ALLOCATOR: GrowableHeap = GrowableHeap::empty();

struct HeapState {
    heap: Heap,
    reserved: usize,
}

struct GrowableHeap {
    state: UnsafeCell<HeapState>,
}

// PolkaVM guests are single-threaded. The allocator is only entered
// synchronously from the current guest call.
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

    fn used(&self) -> usize {
        unsafe { (&*self.state.get()).heap.used() }
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
static mut LAB: Option<EguiLab> = None;

struct EguiLab {
    context: Context,
    input: InputState,
    renderer: Tri2dRenderer,
    demo: DemoWindows,
    frame: u64,
    layout_ms: u64,
    encode_ms: u64,
}

impl EguiLab {
    fn new() -> Self {
        let context = Context::default();
        Self {
            context,
            input: InputState::new(),
            renderer: Tri2dRenderer::new(),
            demo: DemoWindows::default(),
            frame: 0,
            layout_ms: 0,
            encode_ms: 0,
        }
    }

    fn update(&mut self) {
        self.apply_light_contrast();
        let input = self.input.gather();
        let surface_size = self.input.surface_size();
        let context = self.context.clone();
        let layout_started = time_ms();
        let output = context.run(input, |context| self.demo.ui(context));
        let primitives = context.tessellate(output.shapes, output.pixels_per_point);
        let encode_started = time_ms();
        self.layout_ms = encode_started.saturating_sub(layout_started);
        if !self.renderer.render(
            output.textures_delta,
            &primitives,
            output.pixels_per_point,
            surface_size,
        ) {
            log(b"egui-lab: tri2d frame rejected");
        }
        self.encode_ms = time_ms().saturating_sub(encode_started);
        self.frame += 1;
        if self.frame == 2 || self.frame % 120 == 0 {
            self.log_metrics();
        }
    }

    fn apply_light_contrast(&self) {
        let mut style = (*self.context.style()).clone();
        if style.visuals.dark_mode {
            return;
        }
        style.visuals.override_text_color = Some(Color32::from_gray(24));
        style.visuals.weak_text_alpha = 0.82;
        style.visuals.window_stroke = Stroke::new(1.0_f32, Color32::from_gray(125));
        style.visuals.widgets.noninteractive.fg_stroke.color = Color32::from_gray(34);
        style.visuals.widgets.inactive.fg_stroke.color = Color32::from_gray(42);
        self.context.set_style(style);
    }

    fn log_metrics(&self) {
        let mesh = self.renderer.last_stats();
        let message = format!(
            "egui-lab: frame={} heap-used={} layout-ms={} encode-ms={} mesh-bytes={} draws={} vertices={} indices={}",
            self.frame,
            heap_used(),
            self.layout_ms,
            self.encode_ms,
            mesh.bytes,
            mesh.draws,
            mesh.vertices,
            mesh.indices,
        );
        log(message.as_bytes());
    }
}

fn initialize_heap() {
    if !unsafe { ALLOCATOR.initialize() } {
        log(b"egui-lab: heap reservation failed");
        trap();
    }
}

fn heap_used() -> usize {
    ALLOCATOR.used()
}

#[polkavm_derive::polkavm_export]
extern "C" fn init() {
    initialize_heap();
    log(b"egui-lab: init");
}

#[polkavm_derive::polkavm_export]
extern "C" fn update() {
    unsafe {
        if LAB.is_none() {
            LAB = Some(EguiLab::new());
            log(b"egui-lab: ready");
            return;
        }
        LAB.as_mut().unwrap().update();
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    log(b"egui-lab: panic");
    trap()
}

#[alloc_error_handler]
fn allocation_error(_layout: core::alloc::Layout) -> ! {
    log(b"egui-lab: allocation failed");
    trap()
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
