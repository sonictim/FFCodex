// mod build_chromaprint;

// fn main() {
//     // build_chromaprint::build();
// }
use std::env;
use std::path::PathBuf;

fn main() {
    // Determine the target platform
    let target = env::var("TARGET").unwrap_or_default();
    let is_windows = target.contains("windows");
    let is_macos = target.contains("apple");

    // Get the project root directory
    let root_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    // Path to the resources directory containing prebuilt libraries
    let resources_path = root_path.join("resources");

    // Print some debug info
    println!("cargo:warning=Building for target: {}", target);
    println!(
        "cargo:warning=Using resources from: {}",
        resources_path.display()
    );

    // Select the appropriate library directory based on target platform
    let lib_dir = if is_windows {
        if target.contains("x86_64") {
            resources_path.join("lib").join("windows").join("x64")
        } else {
            resources_path.join("lib").join("windows").join("x86")
        }
    } else if is_macos {
        resources_path.join("lib").join("macos")
    } else {
        // Linux or other Unix
        resources_path.join("lib").join("linux")
    };

    println!("cargo:warning=Using libraries from: {}", lib_dir.display());

    // Tell cargo to look for static libraries in that directory
    println!("cargo:rustc-link-search=native={}", lib_dir.display());

    // Link against the static chromaprint library
    println!("cargo:rustc-link-lib=static=chromaprint");

    // Add system-specific dependencies
    if is_macos {
        println!("cargo:rustc-link-lib=dylib=c++");
        println!("cargo:rustc-link-lib=framework=Accelerate");
    } else if is_windows {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    } else {
        // Linux/Unix
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }

    // Include any headers that might be needed
    let include_dir = resources_path.join("include");
    println!("cargo:include={}", include_dir.display());

    // Tell cargo to invalidate the built crate whenever the resources change
    println!("cargo:rerun-if-changed=resources");
}
