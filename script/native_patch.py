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
    reverse = subprocess.run(
        ["git", "apply", "--reverse", "--check", str(patch)],
        capture_output=True, **options,
    )
    if reverse.returncode == 0:
        return
    subprocess.run(["git", "apply", "--check", str(patch)], check=True, **options)
    subprocess.run(["git", "apply", str(patch)], check=True, **options)
