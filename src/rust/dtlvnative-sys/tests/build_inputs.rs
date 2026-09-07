use std::{ffi::OsString, fs};

#[path = "../build_support.rs"]
mod build_support;

#[test]
fn msvc_inputs_resolve_nested_headers_and_preserve_macro_options() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("checkout with spaces/src");
    let manifest = source.join("rust/dtlvnative-sys");
    let dependency = source.join("lmdb/libraries/liblmdb");
    let extra = source.join("extra headers");
    fs::create_dir_all(&manifest).unwrap();
    fs::create_dir_all(&dependency).unwrap();
    fs::create_dir_all(&extra).unwrap();
    fs::write(manifest.join("wrapper.h"), "#include <dtlv_storage.h>\n").unwrap();
    fs::write(
        source.join("dtlv_storage.h"),
        "#include \"lmdb/libraries/liblmdb/dlmdb.h\"\n#include <extra.h>\n",
    )
    .unwrap();
    fs::write(
        dependency.join("dlmdb.h"),
        "#if FIRST != 17 || SECOND != 25 || defined(REMOVED)\n#error incorrect macro arguments\n#endif\nenum { NESTED_VALUE = FIRST + SECOND };\n",
    )
    .unwrap();
    fs::write(extra.join("extra.h"), "enum { EXTRA_VALUE = 9 };\n").unwrap();

    let native_source = build_support::native_source_dir(&manifest);
    assert_eq!(native_source, source);
    #[cfg(windows)]
    assert!(
        !native_source.to_string_lossy().starts_with(r"\\?\"),
        "native tool paths must not acquire a verbatim prefix"
    );

    // Exercise cc's separate -I/path form and joined MSVC flags, with SDK-style
    // spaces in the paths. Unrelated MSVC driver switches must not reach Clang.
    let args = [
        OsString::from("-nologo"),
        OsString::from("-MD"),
        OsString::from("-I"),
        native_source.as_os_str().to_owned(),
        OsString::from(format!("/I{}", extra.display())),
        OsString::from("/D"),
        OsString::from("FIRST=17"),
        OsString::from("-DSECOND=25"),
        OsString::from("/DREMOVED=1"),
        OsString::from("-U"),
        OsString::from("REMOVED"),
        OsString::from("-Z7"),
    ];
    let bindings = bindgen::Builder::default()
        .header(manifest.join("wrapper.h").to_str().unwrap())
        .clang_args(build_support::msvc_clang_args(&args))
        .allowlist_var("(NESTED|EXTRA)_VALUE")
        .generate()
        .expect("Clang must resolve both nested and explicit include directories")
        .to_string();
    for (name, value) in [("NESTED_VALUE", 42), ("EXTRA_VALUE", 9)] {
        assert!(
            bindings
                .lines()
                .any(|line| line.starts_with(&format!("pub const {name}:"))
                    && line.ends_with(&format!("= {value};"))),
            "{bindings}"
        );
    }
}

#[test]
#[should_panic(expected = "Missing operand after MSVC flag -I")]
fn missing_include_operand_is_reported_before_clang_runs() {
    build_support::msvc_clang_args(&[OsString::from("-I")]);
}
