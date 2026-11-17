"""Print the crates a change affects and whether cargo-deny runs (#9).

Usage: affected.py BASE_REF HEAD_REF
"""

import json
import subprocess
import sys
from pathlib import Path

ALL_CRATES_PATHS = ("Cargo.toml", "Cargo.lock", "deny.toml", "rust-toolchain.toml",
                    ".github/workflows/", "scripts/")
DENY_FILES = ("Cargo.toml", "Cargo.lock", "deny.toml")


def affects_all_crates(path: str) -> bool:
    return any(path.startswith(p) if p.endswith("/") else path == p for p in ALL_CRATES_PATHS)


def changed_files(base: str, head: str) -> list[str]:
    out = subprocess.run(["git", "diff", "--name-only", f"{base}...{head}"],
                         check=True, capture_output=True, text=True).stdout
    return [line for line in out.splitlines() if line]


def workspace() -> tuple[dict[str, str], dict[str, set[str]]]:
    """Return {crate directory: package name} and {package: workspace dependents}."""
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        check=True, capture_output=True, text=True).stdout)
    root = Path(meta["workspace_root"])
    dirs, names = {}, set()
    for pkg in meta["packages"]:
        dirs[Path(pkg["manifest_path"]).parent.relative_to(root).as_posix()] = pkg["name"]
        names.add(pkg["name"])
    dependents: dict[str, set[str]] = {name: set() for name in names}
    for pkg in meta["packages"]:
        for dep in pkg["dependencies"]:
            if dep["name"] in names:
                dependents[dep["name"]].add(pkg["name"])
    return dirs, dependents


def main() -> None:
    base, head = sys.argv[1], sys.argv[2]
    files = changed_files(base, head)
    dirs, dependents = workspace()

    if any(affects_all_crates(f) for f in files):
        affected = set(dependents)
    else:
        affected = {name for d, name in dirs.items()
                    for f in files if f.startswith(d + "/")}
        pending = list(affected)
        while pending:
            for dependent in dependents[pending.pop()]:
                if dependent not in affected:
                    affected.add(dependent)
                    pending.append(dependent)

    deny = any(Path(f).name in DENY_FILES for f in files)
    print(f"crates={json.dumps(sorted(affected))}")
    print(f"deny={'true' if deny else 'false'}")


if __name__ == "__main__":
    main()
