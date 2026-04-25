use std::fs;

fn main() {
    #[cfg(feature = "dstdec")]
    build_dst();
}

#[cfg(feature = "dstdec")]
fn build_dst() {
    let mut build = cc::Build::new();

    build.cpp(true);
    build.flag_if_supported("-std=c++17");
    build.flag("-Wno-incompatible-pointer-types");
    build.include("foob_dstdec/sources");

    let mut files = Vec::new();
    collect_cpp_files("foob_dstdec/sources", &mut files);

    for file in &files {
        build.file(file);
        println!("cargo:rerun-if-changed={}", file);
    }

    build.compile("dstdec");

    // Cross-platform C++ runtime linking
    if cfg!(target_env = "msvc") {
        // MSVC toolchain
        println!("cargo:rustc-link-lib=dylib=msvcp140");
        println!("cargo:rustc-link-lib=dylib=vcruntime");
    } else {
        // GCC / Clang (Linux, MinGW)
        println!("cargo:rustc-link-arg=-lstdc++");
        println!("cargo:rustc-link-arg=-lgcc_eh");
    }
}

// Simple recursive file collector
#[cfg(feature = "dstdec")]
fn collect_cpp_files(dir: &str, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();

        if path.is_dir() {
            collect_cpp_files(path.to_str().unwrap(), out);
        } else if let Some(ext) = path.extension() {
            if ext == "cpp" {
                out.push(path.to_str().unwrap().to_string());
            }
        }
    }
}