use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn build() {
    // Output directory for our compiled library
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Clone WavPack repository if not already present
    let wavpack_dir = out_dir.join("wavpack");
    if !wavpack_dir.exists() {
        println!("cargo:warning=Cloning WavPack repository...");
        let status = Command::new("git")
            .args(&[
                "clone",
                "https://github.com/dbry/WavPack.git",
                wavpack_dir.to_str().unwrap(),
            ])
            .status()
            .expect("Failed to clone WavPack repository");

        assert!(status.success(), "Failed to clone WavPack repository");
    }

    // Compile WavPack source
    let mut config = cc::Build::new();

    // Add include directories
    config.include(wavpack_dir.join("include"));

    // Add source files - adjust these based on what you need from WavPack
    let source_files = [
        "src/common_utils.c",
        "src/decorr_utils.c",
        "src/entropy_utils.c",
        "src/extra1.c",
        "src/extra2.c",
        "src/open_utils.c",
        "src/open_filename.c",
        "src/read_words.c",
        "src/unpack.c",
        "src/unpack_dsd.c",
        "src/unpack_floats.c",
        "src/unpack_seek.c",
        "src/unpack_utils.c",
        "src/write_words.c",
        "src/pack.c",
        "src/pack_dsd.c",
        "src/pack_floats.c",
        "src/pack_utils.c",
    ];

    // Add source files to the build
    for file in &source_files {
        config.file(wavpack_dir.join(file));
    }

    // Define compile flags
    config.define("PACKAGE_VERSION", "\"5.6.0\""); // Adjust version as needed
    config.define("_FILE_OFFSET_BITS", "64");

    // Compile the library
    config.compile("wavpack");

    // Tell cargo to link to the compiled library
    println!("cargo:rustc-link-lib=static=wavpack");

    // Tell cargo to invalidate the built crate whenever the build script changes
    println!("cargo:rerun-if-changed=build.rs");

    // Generate Rust bindings if needed
    generate_bindings(&wavpack_dir);
}

fn generate_bindings(wavpack_dir: &Path) {
    // Generate bindings for the WavPack C API
    let bindings = bindgen::Builder::default()
        .header(wavpack_dir.join("include/wavpack.h").to_str().unwrap())
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
