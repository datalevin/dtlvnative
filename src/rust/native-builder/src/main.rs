use std::{collections::BTreeMap, env, fs, path::Path, path::PathBuf, process::Command};

use flate2::{Compression, write::GzEncoder};

#[allow(dead_code)]
#[path = "../../dtlvnative-sys/artifact.rs"]
mod artifact;

fn append(
    archive: &mut tar::Builder<GzEncoder<fs::File>>,
    name: &str,
    path: &Path,
) -> artifact::Result<()> {
    let bytes = fs::read(path)?;
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_cksum();
    archive.append_data(&mut header, name, bytes.as_slice())?;
    Ok(())
}

fn main() -> artifact::Result<()> {
    let destination = PathBuf::from(
        env::args_os()
            .nth(1)
            .expect("Usage: cargo run -p dtlvnative-build -- OUTPUT_DIRECTORY [RELEASE_TAG]"),
    );
    let version = env!("CARGO_PKG_VERSION");
    let release_tag = env::args()
        .nth(2)
        .unwrap_or_else(|| format!("rust-v{version}"));
    let target = env!("DTLVNATIVE_BUILD_TARGET");
    assert!(
        artifact::TARGETS.contains(&target),
        "Unsupported release target: {target}"
    );
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let native = Path::new(env!("OUT_DIR"));
    let runtimes: Vec<_> = [
        ("usearch", cfg!(feature = "usearch")),
        ("llama", cfg!(feature = "llama")),
    ]
    .into_iter()
    .filter_map(|(name, enabled)| enabled.then_some(name))
    .collect();
    if !runtimes.is_empty() {
        // Build native runtimes when producing artifacts, not while Clippy or
        // Rust's test profiles compile this private producer crate.
        let status = Command::new(if cfg!(windows) { "python" } else { "python3" })
            .arg(source.parent().unwrap().join("script/build-rust-runtimes"))
            .arg(native)
            .args(&runtimes)
            .status()?;
        if !status.success() {
            return Err(format!("Could not build optional runtimes: {status}").into());
        }
    }
    fs::create_dir_all(destination.join("bindings"))?;
    let mut manifest = artifact::Manifest {
        schema: 2,
        version: version.into(),
        release_tag,
        artifacts: BTreeMap::new(),
    };
    let mut checksums = String::new();
    let mut components = BTreeMap::new();
    for component in ["dlmdb", "usearch", "llama"] {
        if (component == "usearch" && !cfg!(feature = "usearch"))
            || (component == "llama" && !cfg!(feature = "llama"))
        {
            continue;
        }
        let name = manifest.archive_name(target, component);
        let mut archive = tar::Builder::new(GzEncoder::new(
            fs::File::create(destination.join(&name))?,
            Compression::default(),
        ));
        let mut info = fs::read_to_string(native.join("build-info.txt"))?;
        if component != "dlmdb" {
            info = format!("target={target}\n");
            info.push_str(&fs::read_to_string(
                native.join(format!("{component}-provenance.txt")),
            )?);
        }
        let info_path = native.join(format!("{component}-build-info.txt"));
        fs::write(&info_path, info)?;
        for file in artifact::files(target, component) {
            let path = match file.as_str() {
                "LICENSE" => source.parent().unwrap().join("LICENSE"),
                "DLMDB-LICENSE" => source.join("lmdb/libraries/liblmdb/LICENSE"),
                "build-info.txt" => info_path.clone(),
                "USEARCH-LICENSE" | "LLAMA-LICENSE" | "OPENMP-LICENSE" => native.join(&file),
                _ if component == "dlmdb" => native.join(&file),
                _ => native.join("runtime-build/runtime").join(&file),
            };
            append(&mut archive, &file, &path)?;
        }
        archive.into_inner()?.finish()?;
        let bindings = fs::read(native.join(if component == "dlmdb" {
            "bindings.rs".into()
        } else {
            format!("bindings-{component}.rs")
        }))?;
        fs::write(
            destination
                .join("bindings")
                .join(format!("{target}-{component}.rs")),
            &bindings,
        )?;
        let digest = artifact::sha256(&fs::read(destination.join(&name))?);
        components.insert(
            component.into(),
            artifact::Artifact {
                archive_sha256: digest.clone(),
                bindings_sha256: artifact::sha256(&bindings),
            },
        );
        checksums.push_str(&format!("{digest}  {name}\n"));
    }
    manifest.artifacts.insert(target.into(), components);
    let manifest_path = destination.join("native-artifacts.json");
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest)? + "\n",
    )?;
    artifact::Manifest::read(&manifest_path, version)?;
    fs::write(destination.join("SHA256SUMS"), checksums)?;
    if cfg!(feature = "usearch") {
        let fixture = if target.ends_with("windows-msvc") {
            "rust_vector_fixture.exe"
        } else {
            "rust_vector_fixture"
        };
        fs::copy(
            native.join("runtime-build/runtime").join(fixture),
            destination.join(fixture),
        )?;
        // The standalone C fixture needs its shared runtime next to it.
        for file in [
            artifact::runtime_name(target, "usearch"),
            artifact::openmp_name(target).into(),
        ] {
            let output = destination.join(&file);
            if output.is_file() {
                fs::remove_file(&output)?;
            }
            fs::copy(native.join("runtime-build/runtime").join(&file), output)?;
        }
    }
    println!("Built native artifacts in {}", destination.display());
    Ok(())
}
