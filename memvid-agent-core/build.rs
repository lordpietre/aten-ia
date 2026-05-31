use std::env;
use std::path::PathBuf;
use std::process::Command;

fn repo_slug() -> String {
    if let Ok(val) = env::var("LLAMA_LIBS_REPO") {
        return val;
    }
    if let Ok(val) = env::var("CARGO_PKG_REPOSITORY")
        && let Some(slug) = val.strip_prefix("https://github.com/")
    {
        return slug.trim_end_matches(".git").to_string();
    }
    "lordpietre/aten-ia".to_string()
}

fn local_prebuilt(out_dir: &PathBuf) -> bool {
    let Some(src) = env::var("LLAMA_LOCAL_LIBS").ok() else {
        return false;
    };
    let src = PathBuf::from(src);
    if !src.is_dir() {
        return false;
    }
    let find = Command::new("find")
        .args([&src.to_string_lossy(), "-name", "*.a"])
        .output()
        .ok();
    if let Some(output) = find {
        for path in String::from_utf8_lossy(&output.stdout).lines() {
            let path = PathBuf::from(path);
            if path.exists() {
                let dst = out_dir.join(path.file_name().unwrap());
                let _ = std::fs::copy(&path, &dst);
            }
        }
    }
    true
}

fn download_prebuilt(out_dir: &PathBuf) -> bool {
    let target = match env::var("TARGET") {
        Ok(t) => t,
        Err(_) => return false,
    };

    let url = format!(
        "https://github.com/{}/releases/latest/download/llama-libs-{}.tar.gz",
        repo_slug(),
        target
    );

    let tarball = out_dir.join("llama-libs.tar.gz");

    let tarball_str = tarball.to_string_lossy();

    let fetched = Command::new("curl")
        .args(["-fL", "-o", &tarball_str, &url])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
        || Command::new("wget")
            .args(["-qO", &tarball_str, &url])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

    if !fetched {
        let _ = std::fs::remove_file(&tarball);
        return false;
    }

    let status = Command::new("tar")
        .arg("-xzf")
        .arg(&tarball)
        .arg("-C")
        .arg(out_dir)
        .status();

    match status {
        Ok(s) if s.success() => {
            let _ = std::fs::remove_file(&tarball);
            true
        }
        _ => {
            let _ = std::fs::remove_file(&tarball);
            false
        }
    }
}

fn cmake_build() -> PathBuf {
    // Limit to 1 job to avoid OOM on low-RAM machines
    unsafe { std::env::set_var("CMAKE_BUILD_PARALLEL_LEVEL", "1") };

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
    dst.join("lib")
}

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let lib_dir = if local_prebuilt(&out_dir) {
        println!(
            "cargo:warning=using local prebuilt llama libs from {}",
            env::var("LLAMA_LOCAL_LIBS").unwrap_or_default()
        );
        out_dir.clone()
    } else if download_prebuilt(&out_dir) {
        println!("cargo:warning=using prebuilt llama libs");
        out_dir.clone()
    } else {
        cmake_build()
    };

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=llama");
    println!("cargo:rustc-link-lib=static=llama-common");
    println!("cargo:rustc-link-lib=static=ggml-cpu");
    println!("cargo:rustc-link-lib=static=ggml");
    println!("cargo:rustc-link-lib=static=ggml-base");

    println!("cargo:rustc-link-lib=stdc++");
    println!("cargo:rustc-link-lib=pthread");
    println!("cargo:rustc-link-lib=m");
    println!("cargo:rustc-link-lib=dl");
    println!("cargo:rustc-link-lib=gomp");

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

    let ffi_path = out_dir.join("llama_ffi_raw.rs");
    bindings
        .write_to_file(&ffi_path)
        .expect("Couldn't write bindings!");

    let content = std::fs::read_to_string(&ffi_path).expect("Failed to read bindings");
    let content = content.replace("extern \"C\" {", "unsafe extern \"C\" {");
    std::fs::write(&ffi_path, content).expect("Failed to write patched bindings");

    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=build.rs");
}
