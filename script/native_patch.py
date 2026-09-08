"""Apply repository patches to private native source snapshots."""
import os
from pathlib import Path
import subprocess


def apply_native_patch(directory, patch):
    directory = Path(directory).resolve()
    patch = Path(patch).resolve()
    environment = os.environ.copy()
    # Without a local repository, git apply works on paths relative to cwd.
    # Discovering the enclosing checkout instead can silently skip every path
    # in the patch, even for --check and --reverse --check.
    environment["GIT_CEILING_DIRECTORIES"] = str(directory.parent)
    environment.pop("GIT_DIR", None)
    environment.pop("GIT_WORK_TREE", None)
    options = {"cwd": directory, "env": environment}
    # Snapshot bytes already have their intended line endings. Inheriting the
    # runner's autocrlf/eol settings can rewrite LF files to CRLF during apply.
    command = ["git", "-c", "core.autocrlf=false", "-c", "core.eol=lf", "apply"]
    reverse = subprocess.run(
        [*command, "--reverse", "--check", str(patch)],
        capture_output=True, **options,
    )
    if reverse.returncode == 0:
        return
    subprocess.run([*command, "--check", str(patch)], check=True, **options)
    subprocess.run([*command, str(patch)], check=True, **options)
