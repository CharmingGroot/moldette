"""Print the workspace crates a change affects, for the CI matrix.

Usage: affected.py BASE_REF HEAD_REF

Output (one line each, for $GITHUB_OUTPUT):
    crates=<JSON list of package names>
    deny=<true|false>

A crate is affected when a file under its directory changed, or when it
depends, directly or not, on an affected crate. Testing dependents as well
catches breakage caused by a contract change in moldette-core.
"""

import json
import subprocess
import sys
from pathlib import Path

# Changes to these paths can affect every crate, so all crates are checked.
GLOBAL = ("Cargo.toml", "Cargo.lock", "deny.toml", "rust-toolchain.toml",
          ".github/workflows/", "scripts/")
# Changes to these paths alter the dependency graph, so cargo-deny runs.
DEPENDENCY = ("Cargo.toml", "Cargo.lock", "deny.toml")


def is_global(path: str) -> bool:
    return any(path.startswith(g) if g.endswith("/") else path == g for g in GLOBAL)


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

    if any(is_global(f) for f in files):
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

    deny = any(Path(f).name in DEPENDENCY for f in files)
    print(f"crates={json.dumps(sorted(affected))}")
    print(f"deny={'true' if deny else 'false'}")


if __name__ == "__main__":
    main()
