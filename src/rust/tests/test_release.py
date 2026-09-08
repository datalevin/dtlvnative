"""Release assembly regression tests; fixtures are not executable binaries."""
import hashlib
import io
import json
from pathlib import Path
import runpy
import tarfile
import tempfile
import unittest

RELEASE = runpy.run_path(str(Path(__file__).resolve().parents[3] / "script/prepare-rust-release"))


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.inputs = []
        workspace = RELEASE["ROOT"] / "src/rust/Cargo.toml"
        version = RELEASE["re"].search(r'^version = "([^"]+)"', workspace.read_text(), RELEASE["re"].M)[1]
        for target in sorted(RELEASE["TARGETS"]):
            directory = self.root / target
            (directory / "bindings").mkdir(parents=True)
            manifest = {"schema": 2, "version": version, "release_tag": "test-release", "artifacts": {target: {}}}
            for component in ["dlmdb", "usearch", "llama"]:
                archive = directory / f"dtlvnative-{version}-{target}-{component}.tar.gz"
                if component == "dlmdb":
                    library = "dtlvnative_storage.lib" if target.endswith("msvc") else "libdtlvnative_storage.a"
                    files = [library, "DLMDB-LICENSE"]
                else:
                    suffix = ".dll" if target.endswith("msvc") else ".dylib" if target.endswith("darwin") else ".so"
                    prefix = "" if target.endswith("msvc") else "lib"
                    omp = "vcomp140.dll" if target.endswith("msvc") else "libomp.dylib" if target.endswith("darwin") else "libgomp.so.1"
                    files = [f"{prefix}dtlvnative_{component}{suffix}", omp, "OPENMP-LICENSE", f"{component.upper()}-LICENSE"]
                with tarfile.open(archive, "w:gz") as package:
                    for name in [*files, "build-info.txt", "LICENSE"]:
                        data = f"target={target}\ndlmdb_revision=test\nfnv1a64:dtlv.c=1234\n".encode() if name == "build-info.txt" else b"test fixture"
                        header = tarfile.TarInfo(name)
                        header.size = len(data)
                        package.addfile(header, io.BytesIO(data))
                bindings = directory / "bindings" / f"{target}-{component}.rs"
                bindings.write_text("// Generated binding fixture\n")
                manifest["artifacts"][target][component] = {
                    "archive_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
                    "bindings_sha256": hashlib.sha256(bindings.read_bytes()).hexdigest(),
                }
            (directory / "native-artifacts.json").write_text(json.dumps(manifest))
            self.inputs.append(directory)

    def test_complete_matrix_stages_only_source_crates(self):
        output = self.root / "release"
        RELEASE["prepare"](self.inputs, output, release_tag="test-release")
        workspace = (output / "crates/Cargo.toml").read_text()
        self.assertNotIn("native-builder", workspace)
        self.assertFalse(list((output / "crates").rglob("*.a")))
        self.assertFalse(list((output / "crates").rglob("*.c")))
        self.assertEqual(len(list((output / "crates/dtlvnative-sys/bindings").glob("*.rs"))), 12)
        self.assertEqual(len((output / "native/SHA256SUMS").read_text().splitlines()), 12)

    def test_partial_or_duplicate_matrix_is_rejected(self):
        for inputs in [self.inputs[:1], self.inputs + self.inputs[:1]]:
            with self.assertRaises(ValueError):
                RELEASE["prepare"](inputs, self.root / "release")
        self.assertFalse((self.root / "release").exists())

    def test_modified_bindings_and_archives_are_rejected(self):
        for pattern in ["bindings/*.rs", "*.tar.gz"]:
            path = next(self.inputs[0].glob(pattern))
            original = path.read_bytes()
            path.write_bytes(original + b"modified")
            with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
                RELEASE["prepare"](self.inputs, self.root / "release")
            path.write_bytes(original)

    def test_missing_optional_component_is_rejected(self):
        path = self.inputs[0] / "native-artifacts.json"
        manifest = json.loads(path.read_text())
        del next(iter(manifest["artifacts"].values()))["llama"]
        path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(ValueError, "all three components"):
            RELEASE["prepare"](self.inputs, self.root / "release")

    def test_wrong_release_tag_is_rejected_before_staging(self):
        with self.assertRaisesRegex(ValueError, "Expected release"):
            RELEASE["prepare"](self.inputs, self.root / "release", release_tag="wrong-release")
        self.assertFalse((self.root / "release").exists())


if __name__ == "__main__":
    unittest.main()
