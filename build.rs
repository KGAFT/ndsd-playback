fn main() {
    #[cfg(feature = "dst_decode")]
    let lib_path = cmake::build("dstdec");
    #[cfg(feature = "dst_decode")]
    println!("lib_path = {}", lib_path.display());

    #[cfg(feature = "dst_decode")]
    println!(
        "cargo:rustc-link-search=native={}",
        lib_path.join("lib").display()
    );
    #[cfg(feature = "dst_decode")]
    println!("cargo:rustc-link-lib=static=dstdec-rust-ffi")
}