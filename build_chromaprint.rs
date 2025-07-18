use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn build() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap_or_default();
    let is_windows = target.contains("windows");
    let is_macos = target.contains("apple");

    println!(
        "cargo:warning=FFCodex version: {}",
        env!("CARGO_PKG_VERSION")
    );
    println!("cargo:warning=Building chromaprint from source");

    // Try to use local vendor directory first, then fall back to downloading
    let local_chromaprint_dir = PathBuf::from("vendor/chromaprint");
    let chromaprint_dir = if local_chromaprint_dir.exists()
        && local_chromaprint_dir.join("src/chromaprint.h").exists()
    {
        println!("cargo:warning=Using local chromaprint source from vendor directory");
        local_chromaprint_dir
    } else {
        // Clone chromaprint repository if not already present
        let downloaded_dir = out_dir.join("chromaprint");
        if !downloaded_dir.exists() {
            println!("cargo:warning=Cloning chromaprint repository...");
            let status = Command::new("git")
                .args(&[
                    "clone",
                    "--depth", "1", // Shallow clone for faster downloads
                    "https://github.com/acoustid/chromaprint.git",
                    downloaded_dir.to_str().unwrap(),
                ])
                .status()
                .expect("Failed to clone chromaprint repository. Make sure git is installed and you have internet access.");

            assert!(status.success(), "Failed to clone chromaprint repository");

            // Verify the clone was successful by checking for key files
            let header_file = downloaded_dir.join("src/chromaprint.h");
            if !header_file.exists() {
                panic!("Chromaprint clone appears incomplete - missing src/chromaprint.h");
            }
        }
        downloaded_dir
    };

    // Build chromaprint using CMake
    let build_dir = out_dir.join("chromaprint_build");
    std::fs::create_dir_all(&build_dir).expect("Failed to create build directory");

    // Configure with CMake
    let mut cmake_config = cmake::Config::new(&chromaprint_dir);
    cmake_config
        .define("CMAKE_BUILD_TYPE", "Release")
        .define("BUILD_SHARED_LIBS", "OFF") // Build static library
        .define("BUILD_TOOLS", "OFF") // Don't build command line tools
        .define("BUILD_TESTS", "OFF") // Don't build tests
        .define("WITH_FFTW3", "OFF") // Use built-in FFT implementation
        .define("WITH_AVCODEC", "OFF") // We don't need AVCodec integration
        .define("WITH_AVFORMAT", "OFF") // We don't need AVFormat integration
        .define("CMAKE_C_FLAGS", "-w") // Suppress all C compiler warnings
        .define("CMAKE_CXX_FLAGS", "-w"); // Suppress all C++ compiler warnings

    // Platform-specific configuration
    if is_windows {
        cmake_config.define("CMAKE_MSVC_RUNTIME_LIBRARY", "MultiThreaded");
    }

    let chromaprint_lib_dir = cmake_config.build();

    // Tell cargo where to find the built library
    println!("cargo:rustc-link-search=native={}/lib", chromaprint_lib_dir.display());
    println!("cargo:rustc-link-lib=static=chromaprint");

    // Link against platform-specific C++ runtime libraries
    if is_macos {
        println!("cargo:rustc-link-lib=dylib=c++");
        println!("cargo:rustc-link-lib=framework=Accelerate");
    } else if is_windows {
        // On Windows with MSVC, link against the C++ runtime
        println!("cargo:rustc-link-lib=dylib=msvcrt");
    } else {
        println!("cargo:rustc-link-lib=dylib=stdc++");
        println!("cargo:rustc-link-lib=dylib=m"); // Math library
    }

    // Generate Rust bindings
    generate_bindings(&chromaprint_dir);

    println!("cargo:rerun-if-changed=build_chromaprint.rs");
    println!("cargo:rerun-if-changed=vendor/chromaprint");
}

fn generate_bindings(chromaprint_dir: &Path) {
    // Generate bindings for the chromaprint C API
    let header_path = chromaprint_dir.join("src/chromaprint.h");
    
    let bindings = bindgen::Builder::default()
        .header(header_path.to_str().unwrap())
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        // Allowlist only the functions we need
        .allowlist_function("chromaprint_.*")
        .allowlist_type("ChromaprintContext")
        .allowlist_var("CHROMAPRINT_.*")
        // Generate comments from header
        .generate_comments(true)
        // Use core instead of std for no_std compatibility if needed
        .use_core()
        .generate()
        .expect("Unable to generate chromaprint bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("chromaprint_bindings.rs"))
        .expect("Couldn't write chromaprint bindings!");
    
    println!("cargo:warning=Generated chromaprint bindings at {}/chromaprint_bindings.rs", out_path.display());
}
