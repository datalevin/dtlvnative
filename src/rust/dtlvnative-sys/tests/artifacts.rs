use std::{collections::BTreeMap, fs};

use flate2::{Compression, write::GzEncoder};

#[allow(dead_code)]
#[path = "../artifact.rs"]
mod artifact;

const TARGET: &str = "aarch64-apple-darwin";

fn archive(target: &str, symlink: bool, omit_library: bool) -> Vec<u8> {
    let mut archive = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    let info = format!("target={target}\n");
    for (name, bytes) in [
        (artifact::library_name(TARGET), b"archive".as_slice()),
        ("build-info.txt", info.as_bytes()),
        ("LICENSE", b"license"),
        ("DLMDB-LICENSE", b"license"),
    ] {
        if omit_library && name == artifact::library_name(TARGET) {
            continue;
        }
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        if symlink && name == "LICENSE" {
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_link_name("../outside").unwrap();
        }
        header.set_cksum();
        archive.append_data(&mut header, name, bytes).unwrap();
    }
    archive.into_inner().unwrap().finish().unwrap()
}

#[test]
fn valid_archive_is_checked_and_extracted() {
    let out = tempfile::tempdir().unwrap();
    let bytes = archive(TARGET, false, false);
    artifact::extract(
        &bytes,
        &artifact::sha256(&bytes),
        TARGET,
        "dlmdb",
        out.path(),
    )
    .unwrap();
    assert_eq!(
        fs::read(out.path().join(artifact::library_name(TARGET))).unwrap(),
        b"archive"
    );
}

#[test]
fn damaged_archive_fails_before_writing_files() {
    let out = tempfile::tempdir().unwrap();
    let bytes = archive(TARGET, false, false);
    assert!(artifact::extract(&bytes, &"0".repeat(64), TARGET, "dlmdb", out.path()).is_err());
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
}

#[test]
fn wrong_target_links_and_missing_libraries_are_rejected() {
    for (target, symlink, omit_library) in [
        ("x86_64-pc-windows-msvc", false, false),
        (TARGET, true, false),
        (TARGET, false, true),
    ] {
        let out = tempfile::tempdir().unwrap();
        let bytes = archive(target, symlink, omit_library);
        assert!(
            artifact::extract(
                &bytes,
                &artifact::sha256(&bytes),
                TARGET,
                "dlmdb",
                out.path()
            )
            .is_err()
        );
    }
}

#[test]
fn manifest_rejects_mismatched_versions_and_unsupported_targets() {
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("native-artifacts.json");
    let mut manifest = artifact::Manifest {
        schema: 2,
        version: "0.1.0".into(),
        release_tag: "rust-v0.1.0".into(),
        artifacts: BTreeMap::new(),
    };
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(artifact::Manifest::read(&path, "0.2.0").is_err());
    assert!(
        artifact::Manifest::read(&path, "0.1.0")
            .unwrap()
            .artifact(TARGET, "dlmdb")
            .is_err()
    );
    manifest.artifacts.insert(
        "x86_64-unknown-linux-musl".into(),
        BTreeMap::from([(
            "dlmdb".into(),
            artifact::Artifact {
                archive_sha256: "0".repeat(64),
                bindings_sha256: "0".repeat(64),
            },
        )]),
    );
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(artifact::Manifest::read(&path, "0.1.0").is_err());
}

#[test]
fn optional_archives_require_their_own_library_and_openmp() {
    for component in ["usearch", "llama"] {
        for omit_openmp in [false, true] {
            let out = tempfile::tempdir().unwrap();
            let mut archive = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
            let library = format!("libdtlvnative_{component}.dylib");
            let license = format!("{}-LICENSE", component.to_ascii_uppercase());
            for name in [
                library.as_str(),
                "libomp.dylib",
                "build-info.txt",
                "LICENSE",
                &license,
                "OPENMP-LICENSE",
            ] {
                if omit_openmp && name == "libomp.dylib" {
                    continue;
                }
                let data = if name == "build-info.txt" {
                    format!("target={TARGET}\n")
                } else {
                    "fixture".into()
                };
                let mut header = tar::Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                archive
                    .append_data(&mut header, name, data.as_bytes())
                    .unwrap();
            }
            let bytes = archive.into_inner().unwrap().finish().unwrap();
            let checksum = artifact::sha256(&bytes);
            let result = artifact::extract(&bytes, &checksum, TARGET, component, out.path());
            assert_eq!(result.is_err(), omit_openmp);
            if !omit_openmp {
                assert_eq!(fs::read(out.path().join(&library)).unwrap(), b"fixture");
            }
            let other = if component == "llama" {
                "usearch"
            } else {
                "llama"
            };
            assert!(artifact::extract(&bytes, &checksum, TARGET, other, out.path()).is_err());
        }
    }
}
