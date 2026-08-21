#![no_std]
#![no_main]

extern crate alloc;

#[global_allocator]
static ALLOCATOR: polkavm_derive::LeakingAllocator = polkavm_derive::LeakingAllocator;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe { core::arch::asm!("unimp") }
    }
}

#[polkavm_derive::polkavm_import]
extern "C" {
    fn pvm_set_palette(ptr: u32);
    fn pvm_display(width: u32, height: u32, ptr: u32);
    fn pvm_fetch_epoca_inputs(ptr: u32, capacity: u32) -> u32;
    fn host_audio_submit(ptr: u32, sample_count: u32) -> u32;
    fn pvm_time_ms() -> u64;
    fn pvm_asset_read(
        name_ptr: u32,
        name_len: u32,
        offset: u32,
        dst_ptr: u32,
        max_len: u32,
    ) -> u32;
    fn host_log(ptr: u32, len: u32);
    fn pvm_yield();
}

#[no_mangle]
pub unsafe extern "C" fn pvm_set_palette_wrapper(ptr: u32) {
    pvm_set_palette(ptr);
}

#[no_mangle]
pub unsafe extern "C" fn pvm_display_wrapper(width: u32, height: u32, ptr: u32) {
    pvm_display(width, height, ptr);
}

#[no_mangle]
pub unsafe extern "C" fn pvm_fetch_epoca_inputs_wrapper(ptr: u32, capacity: u32) -> u32 {
    pvm_fetch_epoca_inputs(ptr, capacity)
}
#[no_mangle]
pub unsafe extern "C" fn host_audio_submit_wrapper(ptr: u32, sample_count: u32) -> u32 {
    host_audio_submit(ptr, sample_count)
}


#[no_mangle]
pub unsafe extern "C" fn pvm_time_ms_wrapper() -> u64 {
    pvm_time_ms()
}

#[no_mangle]
pub unsafe extern "C" fn host_asset_read_wrapper(
    name_ptr: u32,
    name_len: u32,
    offset: u32,
    dst_ptr: u32,
    max_len: u32,
) -> u32 {
    pvm_asset_read(name_ptr, name_len, offset, dst_ptr, max_len)
}

#[no_mangle]
pub unsafe extern "C" fn host_log_wrapper(ptr: *const u8, len: usize) {
    host_log(ptr as u32, len as u32);
}

#[no_mangle]
pub unsafe extern "C" fn pvm_yield_wrapper() {
    pvm_yield();
}

extern "C" {
    fn main(argc: i32, argv: *const *const u8) -> i32;
}

fn is_libre_pack() -> bool {
    const ASSET: &[u8] = b"game/duke3d.grp";
    const MARKER: &[u8] = b"LIBRE.PACK";
    let mut header = [0u8; 16];
    let read = unsafe {
        pvm_asset_read(
            ASSET.as_ptr() as u32,
            ASSET.len() as u32,
            0,
            header.as_mut_ptr() as u32,
            header.len() as u32,
        )
    };
    if read != header.len() as u32 || &header[..12] != b"KenSilverman" {
        return false;
    }
    let count = u32::from_le_bytes(header[12..16].try_into().unwrap());
    if count > 4_096 {
        return false;
    }
    let mut entry = [0u8; 16];
    for index in 0..count {
        let read = unsafe {
            pvm_asset_read(
                ASSET.as_ptr() as u32,
                ASSET.len() as u32,
                16 + index * 16,
                entry.as_mut_ptr() as u32,
                entry.len() as u32,
            )
        };
        if read != entry.len() as u32 {
            return false;
        }
        let end = entry[..12].iter().position(|byte| *byte == 0).unwrap_or(12);
        if &entry[..end] == MARKER {
            return true;
        }
    }
    false
}

#[polkavm_derive::polkavm_export]
extern "C" fn _pvm_start() {
    static ARG0: &[u8] = b"duke3d\0";
    static SHAREWARE_ARG1: &[u8] = b"/v1\0";
    static SHAREWARE_ARG2: &[u8] = b"/l1\0";
    static SHAREWARE_ARG3: &[u8] = b"/s2\0";
    static LIBRE_ARG1: &[u8] = b"-map\0";
    static LIBRE_ARG2: &[u8] = b"E1L1.MAP\0";
    static LIBRE_ARG3: &[u8] = b"/ns\0";
    static LIBRE_ARG4: &[u8] = b"/nm\0";
    static mut SHAREWARE_ARGV: [*const u8; 4] = [core::ptr::null(); 4];
    static mut LIBRE_ARGV: [*const u8; 5] = [core::ptr::null(); 5];
    unsafe {
        if is_libre_pack() {
            LIBRE_ARGV[0] = ARG0.as_ptr();
            LIBRE_ARGV[1] = LIBRE_ARG1.as_ptr();
            LIBRE_ARGV[2] = LIBRE_ARG2.as_ptr();
            LIBRE_ARGV[3] = LIBRE_ARG3.as_ptr();
            LIBRE_ARGV[4] = LIBRE_ARG4.as_ptr();
            main(5, core::ptr::addr_of!(LIBRE_ARGV).cast());
        } else {
            SHAREWARE_ARGV[0] = ARG0.as_ptr();
            SHAREWARE_ARGV[1] = SHAREWARE_ARG1.as_ptr();
            SHAREWARE_ARGV[2] = SHAREWARE_ARG2.as_ptr();
            SHAREWARE_ARGV[3] = SHAREWARE_ARG3.as_ptr();
            main(4, core::ptr::addr_of!(SHAREWARE_ARGV).cast());
        }
    }
}
