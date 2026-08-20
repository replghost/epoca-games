use std::path::Path;

fn main() {
    let sources = [
        "Engine/src/cache.c",
        "Engine/src/display_of.c",
        "Engine/src/draw.c",
        "Engine/src/engine.c",
        "Engine/src/filesystem.c",
        "Engine/src/fixedPoint_math.c",
        "Engine/src/dummy_multi.c",
        "Engine/src/tiles.c",
        "Game/src/actors.c",
        "Game/src/animlib.c",
        "Game/src/config.c",
        "Game/src/control.c",
        "Game/src/game.c",
        "Game/src/gamedef.c",
        "Game/src/global.c",
        "Game/src/keyboard.c",
        "Game/src/menues.c",
        "Game/src/player.c",
        "Game/src/premap.c",
        "Game/src/rts.c",
        "Game/src/scriplib.c",
        "Game/src/sector.c",
        "Game/src/sounds.c",
        "Game/src/audiolib/dpmi.c",
        "Game/src/audiolib/fx_man.c",
        "Game/src/audiolib/ll_man.c",
        "Game/src/audiolib/multivoc.c",
        "Game/src/audiolib/mv_mix.c",
        "Game/src/audiolib/mvreverb.c",
        "Game/src/audiolib/pitch.c",
        "Game/src/console.c",
        "Game/src/cvar_defs.c",
        "Game/src/cvars.c",
        "Game/src/midi/midi_of.c",
        "d3d_gpu_stub.c",
        "d3d_save.c",
        "posix_shim.c",
        "libc_shim.c",
        "fileio_shim.c",
        "platform_pvm.c",
    ];
    let mut build = cc::Build::new();
    build
        .target("riscv32-unknown-elf")
        .compiler(std::env::var("PVM_CLANG").unwrap_or_else(|_| "clang".to_owned()))
        .include(".")
        .archiver(std::env::var("PVM_LLVM_AR").unwrap_or_else(|_| "llvm-ar".to_owned()))
        .ranlib(std::env::var("PVM_LLVM_RANLIB").unwrap_or_else(|_| "llvm-ranlib".to_owned()))
        .flag("-march=rv32emc")
        .flag("-mabi=ilp32e")
        .flag("-fno-builtin")
        .flag("-ffreestanding")
        .flag("-nostdinc")
        .flag("-fno-stack-protector")
        .flag("-fPIC")
        .flag("-fno-strict-aliasing")
        .flag("-fcommon")
        .flag("-std=gnu11")
        .warnings(false)
        .define("OPENFPGA", None)
        .define("EPOCA_PVM", None)
        .define("OF_PC", None)
        .flag("-Wno-int-conversion")
        .flag("-Wno-incompatible-pointer-types")
        .flag("-Wno-implicit-function-declaration")
        .define("alloca", Some("__builtin_alloca"))
        .define("UNIX", None)
        .define("SMP_MAX_VOICES", "20")
        .include("Engine/src")
        .include("Game/src")
        .include("Game/src/audiolib")
        .include("sdk_include")
        .include("include");
    for source in sources {
        assert!(Path::new(source).is_file(), "missing Duke source {source}");
        build.file(source);
    }
    build.compile("duke3d");
    println!("cargo:rustc-link-arg=--unresolved-symbols=ignore-all");
    println!("cargo:rerun-if-changed=Engine/");
    println!("cargo:rerun-if-changed=Game/");
    println!("cargo:rerun-if-changed=platform_pvm.c");
    println!("cargo:rerun-if-changed=d3d_gpu_stub.c");
    println!("cargo:rerun-if-changed=fileio_shim.c");
    println!("cargo:rerun-if-changed=libc_shim.c");
}
