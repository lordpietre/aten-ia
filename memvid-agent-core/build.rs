use std::env;
use std::path::PathBuf;

fn main() {
    // Build llama-cpp-turboquant with CMake
    let dst = cmake::Config::new("llama-cpp-turboquant")
        .define("LLAMA_BUILD_TOOLS", "OFF")
        .define("LLAMA_BUILD_EXAMPLES", "OFF")
        .define("LLAMA_BUILD_TESTS", "OFF")
        .define("LLAMA_BUILD_SERVER", "OFF")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("GGML_CUDA", "OFF")
        .define("GGML_METAL", "OFF")
        .define("GGML_VULKAN", "OFF")
        .profile("Release")
        .build();

    let lib_dir = dst.join("lib");

    // Link to llama + ggml static libraries
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=llama");
    println!("cargo:rustc-link-lib=static=llama-common");
    println!("cargo:rustc-link-lib=static=ggml-cpu");
    println!("cargo:rustc-link-lib=static=ggml");
    println!("cargo:rustc-link-lib=static=ggml-base");

    // Link system dependencies needed by llama.cpp
    println!("cargo:rustc-link-lib=stdc++");
    println!("cargo:rustc-link-lib=pthread");
    println!("cargo:rustc-link-lib=m");
    println!("cargo:rustc-link-lib=dl");
    println!("cargo:rustc-link-lib=gomp");

    // Generate Rust FFI bindings from llama.h
    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg("-I./llama-cpp-turboquant/include")
        .clang_arg("-I./llama-cpp-turboquant/ggml/include")
        .allowlist_function("llama_.*")
        .allowlist_type("llama_.*")
        .allowlist_var("LLAMA_.*")
        .allowlist_var("GGML_TYPE_.*")
        .allowlist_type("ggml_type")
        .allowlist_type("llama_.*")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    let ffi_path = out_path.join("llama_ffi_raw.rs");
    bindings
        .write_to_file(&ffi_path)
        .expect("Couldn't write bindings!");

    // Patch: add `unsafe` before `extern "C"` blocks (needed for Rust 2024 edition)
    let content = std::fs::read_to_string(&ffi_path).expect("Failed to read bindings");
    let content = content.replace("extern \"C\" {", "unsafe extern \"C\" {");
    std::fs::write(&ffi_path, content).expect("Failed to write patched bindings");

    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=build.rs");
}
