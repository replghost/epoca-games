#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

#[global_allocator]
static ALLOCATOR: polkavm_derive::LeakingAllocator = polkavm_derive::LeakingAllocator;

const CARTRIDGE_PATH: &[u8] = b"game/cartridge.nes";
const SAVE_PATH: &[u8] = b"save/cartridge.sav";
const MAX_ROM_BYTES: usize = 8 * 1024 * 1024;
const READ_CHUNK_BYTES: usize = 64 * 1024;
const INPUT_EVENT_BYTES: usize = 8;
const MAX_SAVE_BYTES: usize = 1024 * 1024;

const EVENT_KEY_DOWN: u8 = 1;
const EVENT_KEY_UP: u8 = 2;
const EVENT_BUTTON_DOWN: u8 = 3;
const EVENT_BUTTON_UP: u8 = 4;

const BUTTON_B: u16 = 1 << 0;
const BUTTON_SELECT: u16 = 1 << 2;
const BUTTON_START: u16 = 1 << 3;
const BUTTON_UP: u16 = 1 << 4;
const BUTTON_DOWN: u16 = 1 << 5;
const BUTTON_LEFT: u16 = 1 << 6;
const BUTTON_RIGHT: u16 = 1 << 7;
const BUTTON_A: u16 = 1 << 8;

static mut ROM: Option<Vec<u8>> = None;
static mut CONTROLLERS: [u16; 2] = [0; 2];
static mut SAVE_FRAMES: u8 = 0;
static mut SAVE_HASH: u32 = 0;

#[polkavm_derive::polkavm_import]
extern "C" {
    fn host_present_frame(ptr: u32, width: u32, height: u32, stride: u32) -> u32;
    fn host_poll_input(buf_ptr: u32, buf_len: u32) -> u32;
    fn host_audio_submit(ptr: u32, sample_count: u32) -> u32;
    fn host_asset_read(
        name_ptr: u32,
        name_len: u32,
        offset: u32,
        dst_ptr: u32,
        max_len: u32,
    ) -> u32;
    fn host_log(ptr: u32, len: u32);
    fn host_save_submit(ptr: u32, len: u32) -> u32;
}

extern "C" {
    fn nes_core_init(rom: *const u8, rom_size: u32) -> i32;
    fn nes_core_set_buttons(player: u32, buttons: u16);
    fn nes_core_run_frame();
    fn nes_core_save_ram() -> *mut u8;
    fn nes_core_save_ram_size() -> u32;
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    log(b"nes: guest panic\n");
    unsafe { core::arch::asm!("unimp", options(noreturn)) }
}

fn log(message: &[u8]) {
    unsafe { host_log(message.as_ptr() as u32, message.len() as u32) }
}

fn fail(message: &[u8]) -> ! {
    log(message);
    unsafe { core::arch::asm!("unimp", options(noreturn)) }
}

fn read_asset(path: &[u8], maximum: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        if bytes.len() == maximum {
            fail(b"nes: cartridge exceeds size limit\n");
        }
        let capacity = READ_CHUNK_BYTES.min(maximum - bytes.len());
        let offset = bytes.len();
        bytes.resize(offset + capacity, 0);
        let read = unsafe {
            host_asset_read(
                path.as_ptr() as u32,
                path.len() as u32,
                offset as u32,
                bytes[offset..].as_mut_ptr() as u32,
                capacity as u32,
            ) as usize
        };
        if read > capacity {
            fail(b"nes: host returned invalid cartridge length\n");
        }
        bytes.truncate(offset + read);
        if read < capacity {
            return bytes;
        }
    }
}

fn nes2_rom_size(lsb: u8, msb: u8, unit: usize) -> Option<usize> {
    if msb != 0x0f {
        return ((usize::from(msb) << 8) | usize::from(lsb)).checked_mul(unit);
    }
    let exponent = u32::from(lsb >> 2);
    let multiplier = usize::from((lsb & 0x03) * 2 + 1);
    1usize.checked_shl(exponent)?.checked_mul(multiplier)
}

fn validate_cartridge(rom: &[u8]) {
    if rom.len() < 16 || &rom[..4] != b"NES\x1a" {
        fail(b"nes: invalid iNES header\n");
    }
    let trainer = if rom[6] & 0x04 != 0 { 512usize } else { 0 };
    let nes2 = rom[7] & 0x0c == 0x08;
    let (prg, chr) = if nes2 {
        (
            nes2_rom_size(rom[4], rom[9] & 0x0f, 16 * 1024),
            nes2_rom_size(rom[5], rom[9] >> 4, 8 * 1024),
        )
    } else {
        (
            usize::from(rom[4]).checked_mul(16 * 1024),
            usize::from(rom[5]).checked_mul(8 * 1024),
        )
    };
    let expected = 16usize
        .checked_add(trainer)
        .and_then(|size| size.checked_add(prg.unwrap_or(usize::MAX)))
        .and_then(|size| size.checked_add(chr.unwrap_or(usize::MAX)))
        .unwrap_or(usize::MAX);
    if expected > rom.len() || expected > MAX_ROM_BYTES {
        fail(b"nes: truncated or oversized cartridge\n");
    }
}

fn key_binding(code: u8) -> Option<(usize, u16)> {
    Some(match code {
        0x1a => (0, BUTTON_UP),
        0x16 => (0, BUTTON_DOWN),
        0x04 => (0, BUTTON_LEFT),
        0x07 => (0, BUTTON_RIGHT),
        0x0d => (0, BUTTON_B),
        0x0e => (0, BUTTON_A),
        0x28 => (0, BUTTON_START),
        0xe5 => (0, BUTTON_SELECT),
        0x52 => (1, BUTTON_UP),
        0x51 => (1, BUTTON_DOWN),
        0x50 => (1, BUTTON_LEFT),
        0x4f => (1, BUTTON_RIGHT),
        0x11 => (1, BUTTON_B),
        0x10 => (1, BUTTON_A),
        0x2b => (1, BUTTON_START),
        0x2a => (1, BUTTON_SELECT),
        _ => return None,
    })
}

fn button_binding(code: u8) -> Option<(usize, u16)> {
    Some(match code {
        1 => (0, BUTTON_A),
        2 => (0, BUTTON_B),
        8 => (0, BUTTON_SELECT),
        9 => (0, BUTTON_START),
        _ => return None,
    })
}

fn poll_input() {
    let mut events = [0u8; INPUT_EVENT_BYTES * 32];
    let length =
        unsafe { host_poll_input(events.as_mut_ptr() as u32, events.len() as u32) } as usize;
    if length > events.len() || length % INPUT_EVENT_BYTES != 0 {
        fail(b"nes: host returned invalid input data\n");
    }
    unsafe {
        for event in events[..length].chunks_exact(INPUT_EVENT_BYTES) {
            let binding = match event[0] {
                EVENT_KEY_DOWN | EVENT_KEY_UP => key_binding(event[1]),
                EVENT_BUTTON_DOWN | EVENT_BUTTON_UP => button_binding(event[1]),
                _ => None,
            };
            let Some((player, button)) = binding else {
                continue;
            };
            if event[0] == EVENT_KEY_DOWN || event[0] == EVENT_BUTTON_DOWN {
                CONTROLLERS[player] |= button;
            } else {
                CONTROLLERS[player] &= !button;
            }
        }
        nes_core_set_buttons(0, CONTROLLERS[0]);
        nes_core_set_buttons(1, CONTROLLERS[1]);
    }
}

fn restore_save_ram() {
    unsafe {
        let pointer = nes_core_save_ram();
        let size = nes_core_save_ram_size() as usize;
        if pointer.is_null() || size == 0 || size > MAX_SAVE_BYTES {
            return;
        }
        let destination = core::slice::from_raw_parts_mut(pointer, size);
        let read = host_asset_read(
            SAVE_PATH.as_ptr() as u32,
            SAVE_PATH.len() as u32,
            0,
            destination.as_mut_ptr() as u32,
            size as u32,
        ) as usize;
        if read != 0 && read != size {
            fail(b"nes: invalid battery RAM length\n");
        }
        SAVE_HASH = save_hash(destination);
    }
}

#[no_mangle]
pub unsafe extern "C" fn host_log_wrapper(ptr: u32, len: u32) {
    host_log(ptr, len);
}

#[no_mangle]
pub unsafe extern "C" fn host_present_frame_wrapper(
    ptr: *const u32,
    width: u32,
    height: u32,
    stride: u32,
) -> u32 {
    host_present_frame(ptr as u32, width, height, stride)
}

#[no_mangle]
pub unsafe extern "C" fn host_audio_submit_wrapper(ptr: *const i16, sample_count: u32) -> u32 {
    host_audio_submit(ptr as u32, sample_count)
}

fn save_hash(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c9dc5u32;
    for &byte in bytes {
        hash = hash.wrapping_mul(0x01000193) ^ u32::from(byte);
    }
    hash
}

fn submit_save_ram() {
    unsafe {
        SAVE_FRAMES = SAVE_FRAMES.wrapping_add(1);
        if SAVE_FRAMES != 60 {
            return;
        }
        SAVE_FRAMES = 0;
        let pointer = nes_core_save_ram();
        let size = nes_core_save_ram_size() as usize;
        if pointer.is_null() || size == 0 || size > MAX_SAVE_BYTES {
            return;
        }
        let bytes = core::slice::from_raw_parts(pointer, size);
        let hash = save_hash(bytes);
        if hash != SAVE_HASH && host_save_submit(bytes.as_ptr() as u32, bytes.len() as u32) == 0 {
            SAVE_HASH = hash;
        }
    }
}

#[polkavm_derive::polkavm_export]
extern "C" fn init() {
    let rom = read_asset(CARTRIDGE_PATH, MAX_ROM_BYTES);
    validate_cartridge(&rom);
    if unsafe { nes_core_init(rom.as_ptr(), rom.len() as u32) } == 0 {
        fail(b"nes: emulator rejected cartridge\n");
    }
    unsafe { ROM = Some(rom) };
    restore_save_ram();
    log(b"nes: ready\n");
}

#[polkavm_derive::polkavm_export]
extern "C" fn update() {
    poll_input();
    unsafe { nes_core_run_frame() };
    submit_save_ram();
}
