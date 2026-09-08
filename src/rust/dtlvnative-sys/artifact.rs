use std::{collections::BTreeMap, fs, io, path::Path};

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const TARGETS: [&str; 4] = [
    "aarch64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
];

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub version: String,
    pub release_tag: String,
    pub artifacts: BTreeMap<String, BTreeMap<String, Artifact>>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub archive_sha256: String,
    pub bindings_sha256: String,
}

impl Manifest {
    pub fn read(path: &Path, version: &str) -> Result<Self> {
        let manifest: Self = serde_json::from_slice(&fs::read(path)?)?;
        if manifest.schema != 2 || manifest.version != version {
            return Err("Native artifact manifest schema or crate version mismatch".into());
        }
        for value in [&manifest.version, &manifest.release_tag] {
            if value.is_empty()
                || !value
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
            {
                return Err("Invalid native artifact version or release tag".into());
            }
        }
        for (target, components) in &manifest.artifacts {
            if !TARGETS.contains(&target.as_str()) {
                return Err(format!("Unsupported native artifact target: {target}").into());
            }
            for (component, artifact) in components {
                if !["dlmdb", "usearch", "llama"].contains(&component.as_str()) {
                    return Err(format!("Unsupported native component: {component}").into());
                }
                for hash in [&artifact.archive_sha256, &artifact.bindings_sha256] {
                    if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
                        return Err("Invalid SHA-256 in native artifact manifest".into());
                    }
                }
            }
        }
        Ok(manifest)
    }

    pub fn artifact(&self, target: &str, component: &str) -> Result<&Artifact> {
        self.artifacts.get(target).and_then(|components| components.get(component)).ok_or_else(|| {
            format!("No native {component} artifact for {target} in dtlvnative-sys {}. In a checkout, run script/test-rust to build and test local artifacts. Supported release targets: {}", self.version, TARGETS.join(", ")).into()
        })
    }

    pub fn archive_name(&self, target: &str, component: &str) -> String {
        format!("dtlvnative-{}-{target}-{component}.tar.gz", self.version)
    }

    pub fn url(&self, target: &str, component: &str) -> String {
        format!(
            "https://github.com/datalevin/dtlvnative/releases/download/{}/{}",
            self.release_tag,
            self.archive_name(target, component)
        )
    }
}

pub fn library_name(target: &str) -> &'static str {
    if target == "x86_64-pc-windows-msvc" {
        "dtlvnative_storage.lib"
    } else {
        "libdtlvnative_storage.a"
    }
}

pub fn runtime_name(target: &str, component: &str) -> String {
    if target.ends_with("windows-msvc") {
        format!("dtlvnative_{component}.dll")
    } else if target.ends_with("apple-darwin") {
        format!("libdtlvnative_{component}.dylib")
    } else {
        format!("libdtlvnative_{component}.so")
    }
}

pub fn openmp_name(target: &str) -> &'static str {
    if target.ends_with("windows-msvc") {
        "vcomp140.dll"
    } else if target.ends_with("apple-darwin") {
        "libomp.dylib"
    } else {
        "libgomp.so.1"
    }
}

pub fn files(target: &str, component: &str) -> Vec<String> {
    let mut files = vec!["build-info.txt".into(), "LICENSE".into()];
    if component == "dlmdb" {
        files.extend([library_name(target).into(), "DLMDB-LICENSE".into()]);
    } else {
        files.extend([
            runtime_name(target, component),
            openmp_name(target).into(),
            "OPENMP-LICENSE".into(),
            format!("{}-LICENSE", component.to_ascii_uppercase()),
        ]);
    }
    files
}

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn verify(bytes: &[u8], expected: &str) -> Result<()> {
    if sha256(bytes) != expected.to_ascii_lowercase() {
        return Err("Native artifact SHA-256 mismatch".into());
    }
    Ok(())
}

/// Check the digest before interpreting the archive, and accept only the expected
/// regular files produced by our builder. Never follow archive links or paths.
pub fn extract(
    bytes: &[u8],
    expected: &str,
    target: &str,
    component: &str,
    out: &Path,
) -> Result<()> {
    verify(bytes, expected)?;
    let mut archive = tar::Archive::new(GzDecoder::new(bytes));
    let mut required = files(target, component);
    fs::create_dir_all(out)?;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let Some(index) = required
            .iter()
            .position(|name| path == Path::new(name.as_str()))
        else {
            return Err(format!(
                "Unexpected or duplicate native archive entry: {}",
                path.display()
            )
            .into());
        };
        if !entry.header().entry_type().is_file() || entry.size() > 256 * 1024 * 1024 {
            return Err("Invalid native archive entry type or size".into());
        }
        let mut file = fs::File::create(out.join(required.remove(index)))?;
        io::copy(&mut entry, &mut file)?;
    }
    if !required.is_empty() {
        return Err(format!("Native archive missing files: {}", required.join(", ")).into());
    }
    let info = fs::read_to_string(out.join("build-info.txt"))?;
    if !info.lines().any(|line| line == format!("target={target}")) {
        return Err("Native archive target mismatch".into());
    }
    Ok(())
}
