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

fn local_prebuilt(out_dir: &Path) -> bool {
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

fn is_cross_compiling(target: &str, host: &str) -> bool {
    target != host
}

fn cmake_toolchain_path(out_dir: &PathBuf, target: &str) -> PathBuf {
    let toolchain = out_dir.join("cross-toolchain.cmake");
    let (triplet, arch) = if target.contains("aarch64") {
        ("aarch64-linux-gnu", "arm64")
    } else {
        ("x86_64-linux-gnu", "x86_64")
    };
    let content = format!(
        r#"
set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR {arch})
set(CMAKE_C_COMPILER {triplet}-gcc)
set(CMAKE_CXX_COMPILER {triplet}-g++)
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
"#,
        arch = arch,
        triplet = triplet
    );
    std::fs::write(&toolchain, content).expect("Failed to write cross-toolchain file");
    toolchain
}

fn cmake_build() -> PathBuf {
    let target = env::var("TARGET").unwrap_or_default();
    let host = env::var("HOST").unwrap_or_default();
    let cross = is_cross_compiling(&target, &host);

    let parallel = env::var("CMAKE_BUILD_PARALLEL_LEVEL").unwrap_or_else(|_| "2".to_string());
    unsafe { std::env::set_var("CMAKE_BUILD_PARALLEL_LEVEL", &parallel) };

    let mut config = cmake::Config::new("llama-cpp-turboquant");
    config.define("LLAMA_BUILD_TOOLS", "OFF");
    config.define("LLAMA_BUILD_EXAMPLES", "OFF");
    config.define("LLAMA_BUILD_TESTS", "OFF");
    config.define("LLAMA_BUILD_SERVER", "OFF");
    config.define("BUILD_SHARED_LIBS", "OFF");
    config.define("GGML_CUDA", "OFF");
    config.define("GGML_METAL", "OFF");
    config.define("GGML_VULKAN", "OFF");
    config.define("GGML_NATIVE", "OFF");

    if target.contains("aarch64") {
        config.define("GGML_CPU_ARM_ARCH", "armv8-a+dotprod");
    }

    if cross {
        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        let toolchain_path = cmake_toolchain_path(&out_dir, &target);
        config.define(
            "CMAKE_TOOLCHAIN_FILE",
            toolchain_path.to_string_lossy().to_string(),
        );
    }

    let dst = config.profile("Release").build();
    dst.join("lib")
}

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap_or_default();
    let host = env::var("HOST").unwrap_or_default();
    let cross = is_cross_compiling(&target, &host);

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

    let mut bindgen_builder = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg("-I./llama-cpp-turboquant/include")
        .clang_arg("-I./llama-cpp-turboquant/ggml/include")
        .allowlist_function("llama_.*")
        .allowlist_type("llama_.*")
        .allowlist_var("LLAMA_.*")
        .allowlist_var("GGML_TYPE_.*")
        .allowlist_type("ggml_type")
        .allowlist_type("llama_.*")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

    if cross {
        bindgen_builder = bindgen_builder.clang_arg(format!("--target={}", target));
    }

    let bindings = bindgen_builder
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
