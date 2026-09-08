use std::{ffi::OsString, path::Path};

/// Cargo supplies an absolute manifest directory. Keep its ordinary path syntax:
/// canonicalize() introduces Windows verbatim paths that Clang cannot reliably
/// combine with the forward slashes in nested #include directives.
pub fn native_source_dir(manifest: &Path) -> &Path {
    manifest
        .parent()
        .and_then(Path::parent)
        .expect("native-builder must be located under src/rust")
}

/// Convert MSVC preprocessor arguments while preserving separate operands and
/// their order. In particular, cc emits ["-I", "directory"], not just -Idirectory.
pub fn msvc_clang_args(args: &[OsString]) -> Vec<String> {
    let mut result = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let arg = arg.to_string_lossy();
        for flag in ["I", "D", "U"] {
            if let Some(value) = arg
                .strip_prefix(&format!("/{flag}"))
                .or_else(|| arg.strip_prefix(&format!("-{flag}")))
            {
                result.push(format!("-{flag}"));
                result.push(if value.is_empty() {
                    args.next()
                        .unwrap_or_else(|| panic!("Missing operand after MSVC flag {arg}"))
                        .to_string_lossy()
                        .into_owned()
                } else {
                    value.to_owned()
                });
                break;
            }
        }
    }
    result
}
