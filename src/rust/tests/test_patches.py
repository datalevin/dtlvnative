"""Check native patches as checked out on Unix and Windows, without a C++ build."""
from pathlib import Path
import os
import runpy
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
APPLY = runpy.run_path(str(ROOT / "script/native_patch.py"))["apply_native_patch"]
PATCHES = {
    "usearch-gcc-no-native.patch": "usearch",
    "llama-mtmd-build.patch": "llama.cpp",
}


class PatchTests(unittest.TestCase):
    def git(self, directory, *args):
        result = subprocess.run(
            ["git", "-c", "core.autocrlf=false", "-C", str(directory), *map(str, args)],
            capture_output=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
        return result.stdout

    def test_patch_syntax_accepts_lf_and_crlf(self):
        with tempfile.TemporaryDirectory(prefix="dtlv patch syntax ") as directory:
            directory = Path(directory)
            for name in PATCHES:
                data = (ROOT / "patches" / name).read_bytes().replace(b"\r\n", b"\n")
                for ending in [b"\n", b"\r\n"]:
                    with self.subTest(patch=name, ending=ending):
                        patch = directory / name
                        patch.write_bytes(data.replace(b"\n", ending))
                        self.git(directory, "apply", "--numstat", patch)

    def test_patches_apply_and_reverse_without_changing_line_endings(self):
        for name, component in PATCHES.items():
            patch_data = (ROOT / "patches" / name).read_bytes().replace(b"\r\n", b"\n")
            paths = [line[6:] for line in patch_data.decode().splitlines() if line.startswith("+++ b/")]
            # Use committed submodule inputs, regardless of existing local patches.
            originals = {path: self.git(ROOT / "src" / component, "show", f"HEAD:{path}") for path in paths}
            applied_lf = None
            for ending in [b"\n", b"\r\n"]:
                with self.subTest(patch=name, ending=ending), tempfile.TemporaryDirectory(prefix="dtlv patch apply ") as directory:
                    directory = Path(directory)
                    self.git(directory, "init", "--quiet")
                    private = directory / "private source"
                    for path, data in originals.items():
                        destination = private / path
                        destination.parent.mkdir(parents=True, exist_ok=True)
                        destination.write_bytes(data.replace(b"\n", ending))
                    patch = directory / name
                    patch.write_bytes(patch_data.replace(b"\n", ending))
                    APPLY(private, patch)
                    applied = {path: (private / path).read_bytes().replace(b"\r\n", b"\n") for path in paths}
                    for path in paths:
                        self.assertTrue(applied[path] != originals[path], f"Patch skipped {path}")
                    if applied_lf is None:
                        applied_lf = applied
                    else:
                        self.assertEqual(applied, applied_lf)
                    # The producer must detect an applied patch without changing it.
                    APPLY(private, patch)
                    for path in paths:
                        self.assertTrue((private / path).read_bytes() == applied[path].replace(b"\n", ending), path)
                    environment = dict(os.environ, GIT_CEILING_DIRECTORIES=str(directory.resolve()))
                    result = subprocess.run(["git", "apply", "--reverse", str(patch)], cwd=private, env=environment, capture_output=True)
                    self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
                    for path, data in originals.items():
                        self.assertEqual((private / path).read_bytes(), data.replace(b"\n", ending))


if __name__ == "__main__":
    unittest.main()
