use std::env;
use std::fs;
use std::path::PathBuf;

pub fn build() {
    let root_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let chromaprint_path = root_path.join("vendor").join("chromaprint");

    println!(
        "cargo:warning=Building chromaprint from {}",
        chromaprint_path.display()
    );

    // Determine the target platform
    let target = env::var("TARGET").unwrap_or_default();
    let is_windows = target.contains("windows");
    let is_macos = target.contains("apple");

    // For Windows, prepare source to use KissFFT
    if is_windows {
        // Simple approach - just edit CMakeLists.txt to make KissFFT the default
        let cmake_file = chromaprint_path.join("CMakeLists.txt");
        if let Ok(content) = fs::read_to_string(&cmake_file) {
            // Change options to force KissFFT for Windows
            let modified = content
                .replace(
                    "option(WITH_AVFFT \"Use FFmpeg for FFT calculations\" ON)",
                    "option(WITH_AVFFT \"Use FFmpeg for FFT calculations\" OFF)",
                )
                .replace(
                    "option(WITH_KISSFFT \"Use KissFFT for FFT calculations\" OFF)",
                    "option(WITH_KISSFFT \"Use KissFFT for FFT calculations\" ON)",
                );
            fs::write(&cmake_file, modified).expect("Failed to update CMakeLists.txt");
            println!("cargo:warning=Updated CMakeLists.txt to use KissFFT on Windows");
        }
    }

    // Configure build based on platform
    let mut config = cmake::Config::new(&chromaprint_path);
    config
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("BUILD_TOOLS", "OFF")
        .define("BUILD_TESTS", "OFF");

    // FFT implementation selection
    if is_windows {
        config
            .define("WITH_AVFFT", "OFF")
            .define("WITH_FFTW3", "OFF")
            .define("WITH_KISSFFT", "ON");
    } else {
        config
            .define("WITH_AVFFT", "OFF")
            .define("WITH_FFTW3", "OFF");
    }

    let dst = config.build();

    // Link directories
    println!("cargo:rustc-link-search=native={}/lib", dst.display());
    println!("cargo:rustc-link-lib=static=chromaprint");

    // Link against the C++ standard library - platform specific
    if is_macos {
        println!("cargo:rustc-link-lib=dylib=c++");
        // ONLY link Accelerate framework on macOS
        println!("cargo:rustc-link-lib=framework=Accelerate");
    } else if is_windows {
        // MinGW uses libstdc++
        println!("cargo:rustc-link-lib=dylib=stdc++");
    } else {
        // On Linux/Unix, link against libstdc++
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }

    println!("cargo:warning=Library directory: {}/lib", dst.display());
    println!("cargo:rerun-if-changed=vendor/chromaprint");

    // Check for library files with different possible filenames
    let lib_path = dst.join("lib");
    let possible_lib_files = [
        lib_path.join("libchromaprint.a"), // Unix/MinGW style
        lib_path.join("chromaprint.lib"),  // MSVC style
    ];

    let mut found = false;
    for lib_file in &possible_lib_files {
        if lib_file.exists() {
            println!(
                "cargo:warning=Library file exists at: {}",
                lib_file.display()
            );
            found = true;
            break;
        }
    }

    if !found {
        println!("cargo:warning=Library file NOT FOUND!");
        if let Ok(entries) = std::fs::read_dir(&lib_path) {
            println!("cargo:warning=Directory contents:");
            for entry in entries.flatten() {
                println!("cargo:warning=  {}", entry.path().display());
            }
        }
    }
}
