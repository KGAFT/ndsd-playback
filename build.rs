fn main() {
    #[cfg(dstdec)]
    build_dst();
}
#[cfg(feature = "dstdec")]
fn build_dst() {
    let lib_path = cmake::build("foob_dstdec");

    let candidates = [
        lib_path.join("lib").join("libdstdec.a"),
        lib_path.join("build").join("libdstdec.a"),
        lib_path.join("libdstdec.a"),
    ];

    let lib_file = candidates
        .iter()
        .find(|p| p.exists())
        .unwrap_or_else(|| {
            panic!(
                "Could not find libdstdec.a. Searched:\n{}",
                candidates.iter()
                    .map(|p| format!("  {}", p.display()))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        });

    // Pass .a as direct linker arg to avoid -Bstatic/-Bdynamic ordering issues
    println!("cargo:rustc-link-arg={}", lib_file.display());

    // C++ runtime — must also be a direct linker arg because -nodefaultlibs
    // is set and cargo:rustc-link-lib=dylib= gets added too early in the
    // command before --nodefaultlibs takes effect.
    println!("cargo:rustc-link-arg=-lstdc++");
    println!("cargo:rustc-link-arg=-lgcc_eh");

    println!("cargo:rerun-if-changed=dstdec/sources/dst_wrapper.cpp");
    println!("cargo:rerun-if-changed=dstdec/sources/dst_wrapper.h");
    println!("cargo:rerun-if-changed=dstdec/CMakeLists.txt");
    println!("cargo:rerun-if-changed=build.rs");
}