use std::{env, fs, path::PathBuf, process::Command};

mod artifact;

fn run() -> artifact::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=artifact.rs");
    for name in [
        "DTLVNATIVE_ARTIFACT_MANIFEST",
        "DTLVNATIVE_NATIVE_DIR",
        "DTLVNATIVE_OFFLINE",
        "DOCS_RS",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let target = env::var("TARGET")?;
    let components: Vec<_> = ["dlmdb", "usearch", "llama"]
        .into_iter()
        .filter(|component| {
            env::var_os(format!("CARGO_FEATURE_{}", component.to_ascii_uppercase())).is_some()
        })
        .collect();
    if components.is_empty() {
        fs::write(
            out.join("build-info.txt"),
            format!("target={target}\ndlmdb=false\n"),
        )?;
        return Ok(());
    }
    if target == "x86_64-pc-windows-msvc"
        && env::var("CARGO_CFG_TARGET_FEATURE")
            .unwrap_or_default()
            .split(',')
            .any(|f| f == "crt-static")
    {
        return Err(
            "Windows native artifacts use the dynamic MSVC runtime; crt-static is unsupported"
                .into(),
        );
    }
    let crate_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let manifest_path = env::var_os("DTLVNATIVE_ARTIFACT_MANIFEST")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate_dir.join("native-artifacts.json"));
    println!("cargo:rerun-if-changed={}", manifest_path.display());
    let manifest = artifact::Manifest::read(&manifest_path, &env::var("CARGO_PKG_VERSION")?)?;
    let mut build_info = String::new();
    let native_dir = out.join("native");
    println!(
        "cargo:rustc-env=DTLVNATIVE_RUNTIME_DIR={}",
        native_dir.display()
    );
    for component in components {
        let selected = manifest.artifact(&target, component)?;
        let bindings = manifest_path
            .parent()
            .unwrap()
            .join("bindings")
            .join(format!("{target}-{component}.rs"));
        println!("cargo:rerun-if-changed={}", bindings.display());
        let bindings = fs::read(&bindings)?;
        artifact::verify(&bindings, &selected.bindings_sha256)?;
        fs::write(out.join(format!("bindings-{component}.rs")), bindings)?;
        if component != "dlmdb" {
            println!(
                "cargo:rustc-env=DTLVNATIVE_{}_LIBRARY={}",
                component.to_ascii_uppercase(),
                artifact::runtime_name(&target, component)
            );
        }
        // rustdoc does not link executables. docs.rs builds with networking disabled.
        if env::var_os("DOCS_RS").is_some() {
            build_info.push_str(&format!(
                "target={target}\ncomponent={component}\nmode=documentation\n"
            ));
            continue;
        }

        let archive_name = manifest.archive_name(&target, component);
        let local = env::var_os("DTLVNATIVE_NATIVE_DIR").map(PathBuf::from);
        let archive_path = local.as_ref().unwrap_or(&out).join(&archive_name);
        if local.is_some() {
            println!("cargo:rerun-if-changed={}", archive_path.display());
        }
        if !archive_path.is_file() {
            if local.is_some() || env::var_os("DTLVNATIVE_OFFLINE").is_some() {
                return Err(format!("Missing {}. Supply the release archive using DTLVNATIVE_NATIVE_DIR, or allow its initial download.", archive_path.display()).into());
            }
            let partial = out.join(format!("{archive_name}.part"));
            let url = manifest.url(&target, component);
            println!("cargo:warning=Downloading native artifact {url}");
            let status = Command::new("curl")
                .args([
                    "--fail",
                    "--location",
                    "--silent",
                    "--show-error",
                    "--proto",
                    "=https",
                    "--proto-redir",
                    "=https",
                    "--connect-timeout",
                    "30",
                    "--max-time",
                    "180",
                    "--output",
                ])
                .arg(&partial)
                .arg(&url)
                .status()
                .map_err(|e| {
                    format!(
                        "Could not run curl: {e}. Install curl or supply DTLVNATIVE_NATIVE_DIR."
                    )
                })?;
            if !status.success() {
                return Err(format!("Could not download {url}: {status}").into());
            }
            artifact::verify(&fs::read(&partial)?, &selected.archive_sha256)?;
            fs::rename(partial, &archive_path)?;
        }
        artifact::extract(
            &fs::read(&archive_path)?,
            &selected.archive_sha256,
            &target,
            component,
            &native_dir,
        )?;
        let info = fs::read_to_string(native_dir.join("build-info.txt"))?;
        build_info.push_str(&format!(
            "component={component}\n{info}artifact_version={}\nartifact_sha256={}\n",
            manifest.version, selected.archive_sha256
        ));
        if component != "dlmdb" {
            continue;
        }
        println!("cargo:rustc-link-search=native={}", native_dir.display());
        println!("cargo:rustc-link-lib=static=dtlvnative_storage");
        println!(
            "cargo:rustc-link-lib={}",
            if target == "x86_64-pc-windows-msvc" {
                "Advapi32"
            } else {
                "pthread"
            }
        );
    }
    fs::write(out.join("build-info.txt"), build_info)?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        panic!("dtlvnative native artifact: {error}");
    }
}
