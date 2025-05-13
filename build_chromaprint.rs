use std::env;
use std::path::PathBuf;

pub fn build() {
    let root_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    // Check if we're on Windows
    let target = env::var("TARGET").unwrap_or_default();
    let is_windows = target.contains("windows");
    let is_macos = target.contains("apple");

    println!(
        "cargo:warning=FFCodex version: {}",
        env!("CARGO_PKG_VERSION")
    );
    println!("cargo:warning=Using pre-compiled chromaprint static library");

    // Set the search path for the chromaprint library
    if is_windows {
        // Use the Windows-specific library path
        println!(
            "cargo:rustc-link-search=native={}",
            root_path
                .join("resources")
                .join("lib")
                .join("windows")
                .join("x64")
                .display()
        );
    } else {
        // For other platforms
        println!(
            "cargo:rustc-link-search=native={}",
            root_path
                .join("resources")
                .join("lib")
                .join("macos")
                .display()
        );
    }

    // Link against the static library
    println!("cargo:rustc-link-lib=static=chromaprint");

    // Add the include directory to the include path
    println!(
        "cargo:include={}",
        root_path.join("resources").join("include").display()
    );

    // Link against platform-specific C++ runtime libraries
    if is_macos {
        println!("cargo:rustc-link-lib=dylib=c++");
        println!("cargo:rustc-link-lib=framework=Accelerate");
    } else if is_windows {
        // On Windows with MSVC, let the C runtime handle it (no explicit linking)
        // This relies on the correct runtime being available at runtime
        println!("cargo:warning=On Windows, using default C++ runtime");
    } else {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }

    println!("cargo:rerun-if-changed=build_chromaprint.rs");
}
