use std::path::{Path, PathBuf};

fn c_files(directory: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "c"))
        .collect();
    files.sort();
    files
}

fn main() {
    let core = PathBuf::from("vendor/fceumm/src");
    let common = core.join("drivers/libretro/libretro-common");
    let mut sources = Vec::new();
    sources.extend(c_files(&core.join("boards")));
    sources.extend(c_files(&core.join("input")));
    for source in [
        "drivers/libretro/libretro.c",
        "drivers/libretro/libretro_dipswitch.c",
        "cart.c",
        "cheat.c",
        "crc32.c",
        "fceu-endian.c",
        "fceu-memory.c",
        "fceu.c",
        "fds.c",
        "fds_apu.c",
        "file.c",
        "filter.c",
        "general.c",
        "input.c",
        "md5.c",
        "nsf.c",
        "palette.c",
        "ppu.c",
        "sound.c",
        "state.c",
        "video.c",
        "vsuni.c",
        "ines.c",
        "unif.c",
        "x6502.c",
    ] {
        sources.push(core.join(source));
    }
    for source in [
        "streams/memory_stream.c",
        "streams/file_stream.c",
        "file/file_path_io.c",
        "compat/compat_posix_string.c",
        "compat/compat_snprintf.c",
        "compat/compat_strcasestr.c",
        "compat/compat_strl.c",
        "string/stdstring.c",
    ] {
        sources.push(common.join(source));
    }

    let mut build = cc::Build::new();
    build
        .target("riscv32-unknown-elf")
        .compiler(std::env::var("PVM_CLANG").unwrap_or_else(|_| "clang".to_owned()))
        .archiver(std::env::var("PVM_LLVM_AR").unwrap_or_else(|_| "llvm-ar".to_owned()))
        .ranlib(std::env::var("PVM_LLVM_RANLIB").unwrap_or_else(|_| "llvm-ranlib".to_owned()))
        .flag("-march=rv32emc")
        .flag("-mabi=ilp32e")
        .flag("-std=gnu99")
        .flag("-include")
        .flag("c_src/nes_compat.h")
        .flag("-fno-builtin")
        .flag("-ffreestanding")
        .flag("-nostdinc")
        .flag("-fno-stack-protector")
        .flag("-fPIC")
        .warnings(false)
        .define("__LIBRETRO__", None)
        .define("PATH_MAX", "1024")
        .define("FCEU_VERSION_NUMERIC", "9900")
        .define("FRONTEND_SUPPORTS_RGB888", None)
        .define("LSB_FIRST", None)
        .define("LOCAL_LE", "1")
        .define("STATIC_LINKING", "1")
        .include(&core)
        .include(core.join("drivers/libretro"))
        .include(common.join("include"))
        .include(core.join("input"))
        .include(core.join("boards"))
        .include("../doom/c_src/include")
        .files(&sources)
        .file("../doom/c_src/libc_shim.c")
        .file("c_src/libretro_polkavm.c");
    build.compile("fceumm_polkavm");

    println!("cargo:rustc-link-arg=--unresolved-symbols=ignore-all");
    println!("cargo:rerun-if-changed=src/");
    println!("cargo:rerun-if-changed=c_src/");
    println!("cargo:rerun-if-changed=vendor/fceumm/src/");
    println!("cargo:rerun-if-changed=../doom/c_src/libc_shim.c");
    println!("cargo:rerun-if-changed=../doom/c_src/include/");
}
