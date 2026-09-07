use std::{env, fs, path::Path, path::PathBuf, process::Command};

fn revision(source: &Path) -> String {
    // A revision can change without changing the compiled files. Watch the
    // submodule's real git directory as well as the source inputs below.
    for name in ["HEAD", "refs", "packed-refs"] {
        if let Some(output) = Command::new("git")
            .arg("-C")
            .arg(source)
            .args(["rev-parse", "--git-path", name])
            .output()
            .ok()
            .filter(|output| output.status.success())
        {
            let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
            let path = if path.is_absolute() {
                path
            } else {
                source.join(path)
            };
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
    Command::new("git")
        .args(["-C"])
        .arg(source)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unavailable (source archive)".to_owned())
}

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let source = manifest.join("../..").canonicalize().unwrap();
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap();
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=LIBCLANG_PATH");

    let enabled = env::var_os("CARGO_FEATURE_DLMDB").is_some();
    let mut info = format!(
        "target={target}\ndlmdb={enabled}\ndlmdb_revision={}\nsource={}\n",
        revision(&source.join("lmdb")),
        source.display()
    );
    if !enabled {
        fs::write(out.join("build-info.txt"), info).unwrap();
        return;
    }

    let files = [
        "dtlv.c",
        "dtlv_storage.h",
        "dtlv_common.h",
        "lmdb/libraries/liblmdb/mdb.c",
        "lmdb/libraries/liblmdb/midl.c",
        "lmdb/libraries/liblmdb/dlmdb.h",
        "lmdb/libraries/liblmdb/midl.h",
    ];
    for file in files {
        let path = source.join(file);
        assert!(
            path.is_file(),
            "Missing {}; initialize the native submodules",
            path.display()
        );
        println!("cargo:rerun-if-changed={}", path.display());
        // A reproducible content fingerprint also records local edits to a
        // submodule or source archive. This is provenance, not a security hash.
        let fingerprint = fs::read(&path)
            .unwrap()
            .iter()
            .fold(0xcbf29ce484222325_u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
        info.push_str(&format!("fnv1a64:{file}={fingerprint:016x}\n"));
    }

    let windows = env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows";
    let mut native = cc::Build::new();
    native
        .include(&source)
        .include(source.join("lmdb/libraries/liblmdb"))
        .file(source.join("dtlv.c"))
        .file(source.join("lmdb/libraries/liblmdb/mdb.c"))
        .file(source.join("lmdb/libraries/liblmdb/midl.c"))
        .opt_level(2)
        .pic(true);
    if windows {
        native.include(source.join("win32"));
        println!("cargo:rerun-if-changed={}", source.join("win32").display());
        println!("cargo:rustc-link-lib=Advapi32");
    } else {
        native.flag("-pthread");
        println!("cargo:rustc-link-lib=pthread");
    }

    let compiler = native.get_compiler();
    info.push_str(&format!(
        "compiler={}\ncompiler_args={:?}\n",
        compiler.path().display(),
        compiler.args()
    ));
    info.push_str(if windows {
        "runtime=Advapi32\n"
    } else {
        "runtime=pthread\n"
    });
    info.push_str("storage_patches=none\nusearch=false\nllama=false\n");
    fs::write(out.join("build-info.txt"), info).unwrap();

    let mut bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("--target={target}"))
        .clang_arg(format!("-I{}", source.display()))
        .allowlist_function("(mdb|dtlv)_.*")
        .allowlist_type("(MDB|mdb|dtlv)_.*")
        .allowlist_var("(MDB|DTLV)_.*")
        .prepend_enum_name(false)
        .generate_comments(false)
        .rust_edition(bindgen::RustEdition::Edition2024)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));
    // Reuse cc's target, SDK, include and preprocessor settings. In particular,
    // CFLAGS definitions must affect both the C compilation and generated ABI.
    if compiler.is_like_msvc() {
        for arg in compiler.args().iter().filter_map(|arg| arg.to_str()) {
            if let Some(value) = arg.strip_prefix("/D").or_else(|| arg.strip_prefix("-D")) {
                bindings = bindings.clang_arg(format!("-D{value}"));
            } else if let Some(value) = arg.strip_prefix("/I").or_else(|| arg.strip_prefix("-I")) {
                bindings = bindings.clang_arg(format!("-I{value}"));
            }
        }
    } else {
        bindings = bindings.clang_args(compiler.args().iter().map(|arg| arg.to_string_lossy()));
    }
    if windows {
        bindings = bindings.clang_arg(format!("-I{}", source.join("win32").display()));
    }
    bindings
        .generate()
        .expect("Could not generate DLMDB bindings; install libclang or set LIBCLANG_PATH")
        .write_to_file(out.join("bindings.rs"))
        .unwrap();
    native.compile("dtlvnative_storage");
    println!("cargo:include={}", source.display());
}
