"""Check native patches as checked out on Unix and Windows, without a C++ build."""
from itertools import product
import os
from pathlib import Path
import runpy
import subprocess
import tempfile
import unittest
from unittest.mock import patch as patch_environment

ROOT = Path(__file__).resolve().parents[3]
APPLY = runpy.run_path(str(ROOT / "script/native_patch.py"))["apply_native_patch"]
PATCHES = {
    "usearch-gcc-no-native.patch": "usearch",
    "llama-mtmd-build.patch": "llama.cpp",
}


class PatchTests(unittest.TestCase):
    def git(self, directory, *args, **options):
        result = subprocess.run(
            ["git", "-c", "core.autocrlf=false", "-c", "core.eol=lf", "-C", str(directory), *map(str, args)],
            capture_output=True, **options,
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
            configurations = [("false", "lf"), ("true", "crlf"), ("input", "crlf"), ("false", "crlf")]
            for ending, (autocrlf, eol) in product([b"\n", b"\r\n"], configurations):
                # Emulate runner/user settings without modifying real Git config.
                configuration = {
                    "GIT_CONFIG_COUNT": "2",
                    "GIT_CONFIG_KEY_0": "core.autocrlf",
                    "GIT_CONFIG_VALUE_0": autocrlf,
                    "GIT_CONFIG_KEY_1": "core.eol",
                    "GIT_CONFIG_VALUE_1": eol,
                }
                with (
                    self.subTest(patch=name, ending=ending, autocrlf=autocrlf, eol=eol),
                    patch_environment.dict(os.environ, configuration),
                    tempfile.TemporaryDirectory(prefix="dtlv patch apply ") as directory,
                ):
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
                        self.assertTrue(
                            (private / path).read_bytes() == applied[path].replace(b"\n", ending),
                            f"Patch changed line endings: {path}",
                        )
                    if applied_lf is None:
                        applied_lf = applied
                    else:
                        self.assertEqual(applied, applied_lf)
                    # The producer must detect an applied patch without changing it.
                    APPLY(private, patch)
                    for path in paths:
                        self.assertTrue((private / path).read_bytes() == applied[path].replace(b"\n", ending), path)
                    environment = dict(os.environ, GIT_CEILING_DIRECTORIES=str(directory.resolve()))
                    self.git(private, "apply", "--reverse", patch, env=environment)
                    for path, data in originals.items():
                        self.assertEqual((private / path).read_bytes(), data.replace(b"\n", ending))


if __name__ == "__main__":
    unittest.main()
