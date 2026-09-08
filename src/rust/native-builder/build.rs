use std::{env, fs, path::Path, path::PathBuf, process::Command};

mod build_support;

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
    let source = build_support::native_source_dir(&manifest);
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap();
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_support.rs");
    println!("cargo:rerun-if-env-changed=LIBCLANG_PATH");

    assert_eq!(
        env::var("HOST").unwrap(),
        target,
        "Build release artifacts on their target platform"
    );
    println!("cargo:rustc-env=DTLVNATIVE_BUILD_TARGET={target}");
    let mut info = format!(
        "target={target}\ndlmdb=true\ndlmdb_revision={}\nsource={}\n",
        revision(&source.join("lmdb")),
        source.display()
    );
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
        // Normalize checkout line endings so Windows and Unix provenance can
        // be compared. This identifies source changes, not binary integrity.
        let fingerprint = fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n")
            .bytes()
            .fold(0xcbf29ce484222325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            });
        info.push_str(&format!("fnv1a64:{file}={fingerprint:016x}\n"));
    }

    let windows = env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows";
    let mut native = cc::Build::new();
    native
        .include(source)
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
    info.push_str(
        "source_fingerprint_normalization=LF\nstorage_patches=none\nusearch=false\nllama=false\n",
    );
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
        bindings = bindings.clang_args(build_support::msvc_clang_args(compiler.args()));
    } else {
        bindings = bindings.clang_args(compiler.args().iter().map(|arg| arg.to_string_lossy()));
    }
    if windows {
        bindings = bindings.clang_arg(format!("-I{}", source.join("win32").display()));
    }
    bindings
        .generate()
        .expect("Could not generate DLMDB bindings; see the Clang diagnostics above")
        .write_to_file(out.join("bindings.rs"))
        .unwrap();
    native.compile("dtlvnative_storage");
    println!("cargo:include={}", source.display());

    let components: Vec<_> = ["usearch", "llama"]
        .into_iter()
        .filter(|name| {
            env::var_os(format!("CARGO_FEATURE_{}", name.to_ascii_uppercase())).is_some()
        })
        .collect();
    if !components.is_empty() {
        let script = source.parent().unwrap().join("script/build-rust-runtimes");
        println!("cargo:rerun-if-changed={}", script.display());
        println!(
            "cargo:rerun-if-changed={}",
            script.with_file_name("native_patch.py").display()
        );
        println!(
            "cargo:rerun-if-changed={}",
            source.join("CMakeLists.txt").display()
        );
        for file in [
            "dtlv_llama.c",
            "dtlv_llama.h",
            "rust/tests/vector_fixture.c",
        ] {
            println!("cargo:rerun-if-changed={}", source.join(file).display());
        }
        for component in &components {
            let directory = if *component == "llama" {
                "llama.cpp"
            } else {
                component
            };
            let _ = revision(&source.join(directory));
            // Watch tracked inputs, excluding native build output directories.
            let files = Command::new("git")
                .arg("-C")
                .arg(source.join(directory))
                .args(["ls-files", "--recurse-submodules", "-z"])
                .output()
                .unwrap();
            assert!(files.status.success());
            for file in files
                .stdout
                .split(|byte| *byte == 0)
                .filter(|file| !file.is_empty())
            {
                println!(
                    "cargo:rerun-if-changed={}",
                    source
                        .join(directory)
                        .join(String::from_utf8_lossy(file).as_ref())
                        .display()
                );
            }
        }
        println!(
            "cargo:rerun-if-changed={}",
            source.parent().unwrap().join("patches").display()
        );
        for name in [
            "LLVM_PREFIX",
            "LIBOMP_PREFIX",
            "VCToolsRedistDir",
            "CMAKE_BUILD_PARALLEL_LEVEL",
        ] {
            println!("cargo:rerun-if-env-changed={name}");
        }
        for component in components {
            let header = if component == "usearch" {
                "usearch/c/usearch.h"
            } else {
                "dtlv_llama.h"
            };
            let mut bindings = bindgen::Builder::default()
                .header(source.join(header).to_str().unwrap())
                .clang_arg(format!("--target={target}"))
                .clang_arg(format!("-I{}", source.display()))
                .clang_arg("-DUSEARCH_EXPORT=")
                .allowlist_function(if component == "usearch" {
                    "usearch_.*"
                } else {
                    "dtlv_llama_.*"
                })
                .allowlist_type(if component == "usearch" {
                    "usearch_.*"
                } else {
                    "dtlv_llama_.*"
                })
                .prepend_enum_name(false)
                .generate_comments(false)
                .rust_edition(bindgen::RustEdition::Edition2024)
                .wrap_unsafe_ops(true)
                .dynamic_library_name(if component == "usearch" {
                    "UsearchApi"
                } else {
                    "LlamaApi"
                })
                .dynamic_link_require_all(true);
            if compiler.is_like_msvc() {
                bindings = bindings.clang_args(build_support::msvc_clang_args(compiler.args()));
            } else {
                bindings =
                    bindings.clang_args(compiler.args().iter().map(|arg| arg.to_string_lossy()));
            }
            bindings
                .generate()
                .expect("Could not generate runtime bindings")
                .write_to_file(out.join(format!("bindings-{component}.rs")))
                .unwrap();
        }
    }
}
