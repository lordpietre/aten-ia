use std::env;
use std::path::{Path, PathBuf};
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
    let Ok(src_str) = env::var("LLAMA_LOCAL_LIBS") else {
        return false;
    };
    let src = PathBuf::from(&src_str);
    if !src.is_dir() {
        println!(
            "cargo:warning=LLAMA_LOCAL_LIBS={} is not a directory, falling back",
            src_str
        );
        return false;
    }
    if let Ok(entries) = std::fs::read_dir(&src) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "a") {
                let file_name = path.file_name().unwrap();
                let dst = out_dir.join(file_name);
                match std::fs::copy(&path, &dst) {
                    Ok(bytes) => println!(
                        "cargo:warning=copied {} ({} bytes)",
                        file_name.to_string_lossy(),
                        bytes
                    ),
                    Err(e) => println!(
                        "cargo:warning=failed to copy {}: {}",
                        file_name.to_string_lossy(),
                        e
                    ),
                }
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

fn merge_ggml_libs(lib_dir: &Path) {
    let merged = lib_dir.join("libggml-merged.a");
    if merged.exists() {
        return;
    }
    let ggml_cpu = lib_dir.join("libggml-cpu.a");
    let ggml = lib_dir.join("libggml.a");
    let ggml_base = lib_dir.join("libggml-base.a");
    let mri_script = format!(
        "CREATE {merged}\nADDLIB {cpu}\nADDLIB {ggml}\nADDLIB {base}\nSAVE\nEND\n",
        merged = merged.display(),
        cpu = ggml_cpu.display(),
        ggml = ggml.display(),
        base = ggml_base.display(),
    );
    use std::io::Write;
    let mut child = Command::new("ar")
        .arg("-M")
        .current_dir(lib_dir)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn ar -M");
    {
        let stdin = child.stdin.as_mut().expect("stdin pipe");
        stdin
            .write_all(mri_script.as_bytes())
            .expect("write mri script");
    }
    let status = child.wait().expect("wait for ar -M");
    assert!(status.success(), "ar -M (MRI merge) failed");
}

fn find_gcc_static_lib_dir(target: &str) -> Option<String> {
    let compiler = if target.contains("aarch64") {
        "aarch64-linux-gnu-gcc"
    } else {
        "gcc"
    };
    if let Ok(output) = Command::new(compiler).args(["-print-search-dirs"]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Some(rest) = line.strip_prefix("install: ") {
                let install_dir = rest.trim().trim_end_matches('/');
                if Path::new(&install_dir).join("libstdc++.a").exists() {
                    return Some(install_dir.to_string());
                }
            }
        }
    }
    let fallbacks: Vec<String> = if target.contains("aarch64") {
        [
            "/usr/lib/gcc-cross/aarch64-linux-gnu",
            "/usr/lib/gcc/aarch64-linux-gnu/9",
            "/usr/lib/gcc/aarch64-linux-gnu/10",
            "/usr/lib/gcc/aarch64-linux-gnu/11",
            "/usr/lib/gcc/aarch64-linux-gnu/12",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    } else {
        [
            "/usr/lib/gcc/x86_64-linux-gnu/9",
            "/usr/lib/gcc/x86_64-linux-gnu/10",
            "/usr/lib/gcc/x86_64-linux-gnu/11",
            "/usr/lib/gcc/x86_64-linux-gnu/12",
            "/usr/lib/gcc/x86_64-linux-gnu/13",
            "/usr/lib/gcc/x86_64-linux-gnu/14",
            "/usr/lib/gcc/x86_64-linux-gnu/15",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    };
    for dir in &fallbacks {
        if Path::new(dir).join("libstdc++.a").exists() {
            return Some(dir.clone());
        }
    }
    None
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

    merge_ggml_libs(&lib_dir);

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=llama");
    println!("cargo:rustc-link-lib=static=llama-common");
    println!("cargo:rustc-link-lib=static=ggml-merged");

    let portable = env::var("ATEN_PORTABLE").unwrap_or_default() == "1";
    if portable {
        println!("cargo:warning=portable mode: static linking stdc++ and gomp");
        let gcc_lib_dir = find_gcc_static_lib_dir(&target).unwrap_or_else(|| {
            if target.contains("aarch64") {
                "/usr/lib/gcc-cross/aarch64-linux-gnu/9".to_string()
            } else {
                "/usr/lib/gcc/x86_64-linux-gnu/9".to_string()
            }
        });
        println!("cargo:warning=using gcc lib dir: {}", gcc_lib_dir);
        println!("cargo:rustc-link-search=native={}", gcc_lib_dir);
        println!("cargo:rustc-link-lib=static=stdc++");
        println!("cargo:rustc-link-lib=static=gomp");
    } else {
        println!("cargo:rustc-link-lib=stdc++");
        println!("cargo:rustc-link-lib=gomp");
    }
    println!("cargo:rustc-link-lib=pthread");
    println!("cargo:rustc-link-lib=m");
    println!("cargo:rustc-link-lib=dl");

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
