use parse_cfg::Target;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

fn main() {
    #[cfg(feature = "dstdec")]
    build_dst();
}

fn is_msvc() -> bool {
    let target: Target = std::env::var("TARGET")
        .expect("Target not set.")
        .parse()
        .expect("Unable to parse target.");

    let target_env = match target {
        Target::Triple { env, .. } => env,
        Target::Cfg(_) => panic!("cfg targets not supported"),
    };

    if let Some(env) = target_env {
        env.contains("msvc")
    } else {
        false
    }
}
#[cfg(feature = "dstdec")]

fn build_dst() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("bad path"));
    println!("cargo:rerun-if-env-changed={}", "foob_dstdec");
    let mut lib_path = out_dir.clone();
    lib_path.push("libdstdec.a");

    if is_msvc() {
        invoke_vcvars_if_not_set();
    }
    let mut src_dir = PathBuf::from("foob_dstdec");
    create_lib(src_dir.as_path());
    create_bindings(src_dir.as_path());
    println!("cargo:rustc-link-search={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=dstdec");
    println!("cargo:rustc-cfg=dstdec");
}

#[cfg(feature = "dstdec")]
fn create_lib(base_dir: &Path) {
    let mut cpp_paths = Vec::new();
    let mut src_dir = base_dir.to_path_buf();
    let mut bind_dir = base_dir.to_path_buf();
    src_dir.push("sources");

    let walk_a_dir = |dir_to_walk, paths: &mut Vec<PathBuf>| {
        for entry in WalkDir::new(dir_to_walk).max_depth(1) {
            let entry = match entry {
                Err(e) => {
                    println!("error: {}", e);
                    continue;
                }
                Ok(entry) => entry,
            };
            match entry.path().extension().and_then(|s| s.to_str()) {
                None => continue,
                Some("cpp") => paths.push(entry.path().to_path_buf()),
                Some(_) => continue,
            };
        }
    };
    walk_a_dir(src_dir, &mut cpp_paths);
    walk_a_dir(bind_dir, &mut cpp_paths);
    cc::Build::new()
        .include(format!("{}/{}", base_dir.display(), "sources"))
        .files(cpp_paths)
        .cpp(true)
        .compile("libdstdec.a");
}
#[cfg(feature = "dstdec")]

fn create_bindings(base_dir: &Path) {


    let mut h_paths = Vec::new();

    let mut bind_dir = base_dir.to_path_buf();
    bind_dir.push("sources");
    bind_dir.push("dst_wrapper.h");
    h_paths.push(bind_dir);
    let mut bindings = bindgen::Builder::default()
        .clang_arg("-x")
        .clang_arg("c++")
        .clang_arg("-std=c++17")
        .clang_arg(format!("-I{}/{}", base_dir.display(), "binding"))
        .clang_arg(format!("-I{}/{}", base_dir.display(), "sources"))

        .allowlist_function("dst_decoder_new")
        .allowlist_function("dst_decoder_free")
        .allowlist_function("dst_decoder_decode");

    for x in h_paths.iter() {
        bindings = bindings.header(x.display().to_string().as_str());
    }
    let bindings = bindings.generate().expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").expect("bad path"));

    bindings
        .write_to_file(out_path.join("dst_bindings.rs"))
        .expect("Couldn't write bindings!");
}
#[cfg(feature = "dstdec")]

fn invoke_vcvars_if_not_set() {
    if vcvars_set() {
        return;
    }
    println!("VCINSTALLDIR is not set. Attempting to invoke vcvarsall.bat..");

    println!("Invoking vcvarsall.bat..");
    println!("Determining system architecture..");

    let arch_arg = determine_vcvarsall_bat_arch_arg();
    println!(
        "Host architecture is detected as {}.",
        std::env::consts::ARCH
    );
    println!("Architecture argument for vcvarsall.bat will be used as: {arch_arg}.");

    let vcvars_all_bat_path = search_vcvars_all_bat();

    println!(
        "Found vcvarsall.bat at {}. Initializing environment..",
        vcvars_all_bat_path.display()
    );

    // Invoke vcvarsall.bat
    let output = Command::new("cmd")
        .args([
            "/c",
            vcvars_all_bat_path.to_str().unwrap(),
            &arch_arg,
            "&&",
            "set",
        ])
        .output()
        .expect("Failed to execute command");

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        // Filters the output of vcvarsall.bat to only include lines of the form "VARNAME=VALUE"
        let parts: Vec<&str> = line.splitn(2, '=').collect();
        if parts.len() == 2 {
            unsafe {
                env::set_var(parts[0], parts[1]);
            }
            println!("{}={}", parts[0], parts[1]);
        }
    }
}
#[cfg(feature = "dstdec")]

fn vcvars_set() -> bool {
    env::var("VCINSTALLDIR").is_ok()
}

/// Searches for vcvarsall.bat in the default installation directories
///
/// If it is not found, it will search for it in the Program Files directories
///
/// If it is still not found, it will panic.
///
#[cfg(feature = "dstdec")]
fn search_vcvars_all_bat() -> PathBuf {
    if let Some(path) = guess_vcvars_all_bat() {
        return path;
    }

    // Define search paths for vcvarsall.bat based on architecture
    let paths = &[
        // Visual Studio 2022+
        "C:\\Program Files\\Microsoft Visual Studio\\",
        // <= Visual Studio 2019
        "C:\\Program Files (x86)\\Microsoft Visual Studio\\",
    ];

    // Search for vcvarsall.bat using walkdir
    println!("Searching for vcvarsall.bat in {paths:?}");

    let mut found = None;

    for path in paths.iter() {
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| !e.file_type().is_dir())
        {
            if entry.path().ends_with("vcvarsall.bat") {
                found.replace(entry.path().to_path_buf());
            }
        }
    }

    match found {
        Some(path) => path,
        None => panic!(
            "Could not find vcvarsall.bat. Please install the latest version of Visual Studio."
        ),
    }
}

/// Guesses the location of vcvarsall.bat by searching it with certain heuristics.
///
/// It is meant to be executed before a top level search over Microsoft Visual Studio directories
/// to ensure faster execution in CI environments.
#[cfg(feature = "dstdec")]
fn guess_vcvars_all_bat() -> Option<PathBuf> {
    /// Checks if a string is a year
    fn is_year(s: Option<&str>) -> Option<String> {
        let Some(s) = s else {
            return None;
        };

        if s.len() == 4 && s.chars().all(|c| c.is_ascii_digit()) {
            Some(s.to_string())
        } else {
            None
        }
    }

    /// Checks if a string is an edition of Visual Studio
    #[cfg(feature = "dstdec")]
    fn is_edition(s: Option<&str>) -> Option<String> {
        let Some(s) = s else {
            return None;
        };

        let editions = ["Enterprise", "Professional", "Community", "Express"];
        if editions.contains(&s) {
            Some(s.to_string())
        } else {
            None
        }
    }

    /// Constructs a path to vcvarsall.bat based on a base path
    #[cfg(feature = "dstdec")]

    fn construct_path(base: &Path) -> Option<PathBuf> {
        let mut constructed = base.to_path_buf();
        for entry in WalkDir::new(&constructed).max_depth(1) {
            let entry = match entry {
                Err(_) => continue,
                Ok(entry) => entry,
            };
            if let Some(year) = is_year(entry.path().file_name().and_then(|s| s.to_str())) {
                constructed = constructed.join(year);
                for entry in WalkDir::new(&constructed).max_depth(1) {
                    let entry = match entry {
                        Err(_) => continue,
                        Ok(entry) => entry,
                    };
                    if let Some(edition) =
                        is_edition(entry.path().file_name().and_then(|s| s.to_str()))
                    {
                        constructed = constructed
                            .join(edition)
                            .join("VC")
                            .join("Auxiliary")
                            .join("Build")
                            .join("vcvarsall.bat");

                        return Some(constructed);
                    }
                }
            }
        }
        None
    }

    let vs_2022_and_onwards_base = PathBuf::from("C:\\Program Files\\Microsoft Visual Studio\\");
    let vs_2019_and_2017_base = PathBuf::from("C:\\Program Files (x86)\\Microsoft Visual Studio\\");

    construct_path(&vs_2022_and_onwards_base).map_or_else(
        || construct_path(&vs_2019_and_2017_base).map_or_else(|| None, Some),
        Some,
    )
}

/// Determines the right argument to pass to `vcvarsall.bat` based on the host and target architectures.
///
/// Windows on ARM is not supporting 32 bit arm processors.
/// Because of this there is no native or cross compilation is supported for 32 bit arm processors.
#[cfg(feature = "dstdec")]

fn determine_vcvarsall_bat_arch_arg() -> String {
    let host_architecture = std::env::consts::ARCH;
    let target_architecture = std::env::var("CARGO_CFG_TARGET_ARCH").expect("Target not set.");

    let arch_arg = if target_architecture == "x86_64" {
        if host_architecture == "x86" {
            // Arg for cross compilation from x86 to x64
            "x86_amd64"
        } else if host_architecture == "x86_64" {
            // Arg for native compilation from x64 to x64
            "amd64"
        } else if host_architecture == "aarch64" {
            // Arg for cross compilation from arm64 to amd64
            "arm64_amd64"
        } else {
            panic!("Unsupported host architecture {}", host_architecture);
        }
    } else if target_architecture == "x86" {
        if host_architecture == "x86" {
            // Arg for native compilation from x86 to x86
            "x86"
        } else if host_architecture == "x86_64" {
            // Arg for cross compilation from x64 to x86
            "amd64_x86"
        } else if host_architecture == "aarch64" {
            // Arg for cross compilation from arm64 to x86
            "arm64_x86"
        } else {
            panic!("Unsupported host architecture {}", host_architecture);
        }
    } else if target_architecture == "arm" {
        if host_architecture == "x86" {
            // Arg for cross compilation from x86 to arm
            "x86_arm"
        } else if host_architecture == "x86_64" {
            // Arg for cross compilation from x64 to arm
            "amd64_arm"
        } else if host_architecture == "aarch64" {
            // Arg for cross compilation from arm64 to arm
            "arm64_arm"
        } else {
            panic!("Unsupported host architecture {}", host_architecture);
        }
    } else if target_architecture == "aarch64" {
        if host_architecture == "x86" {
            // Arg for cross compilation from x86 to arm
            "x86_arm64"
        } else if host_architecture == "x86_64" {
            // Arg for cross compilation from x64 to arm
            "amd64_arm64"
        } else if host_architecture == "aarch64" {
            // Arg for native compilation from arm64 to arm64
            "arm64"
        } else {
            panic!("Unsupported host architecture {}", host_architecture);
        }
    } else {
        panic!("Unsupported target architecture.");
    };

    arch_arg.to_owned()
}
