"""Print the CI scope of each crate a change affects (#9, #56).

A changed crate runs in full. A crate that depends on it directly runs its
coupling tests for the changed modules and the modules that use them (edge),
and a crate that depends on it through another crate is compiled (check).
A change to core, to a shared kernel module or to a crate root counts as a
change to every module of that crate, and core runs every crate that uses it
in full. A `Cargo.lock` change runs in full the crates whose resolved
dependency tree changed. `moldette-cli` and a crate under `examples/` or
`benches/` skip the wasm32 build and, unless they changed themselves, are only
compiled (#84); a benchmark crate with allocation tests runs those instead
(#64).

Usage: affected.py BASE_REF HEAD_REF
       affected.py --full
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

from module_graph import Graph, crate_graphs, file_module, strip_comments_and_literals

FULL_PATHS = ("Cargo.toml", "deny.toml", "rust-toolchain.toml", ".github/", "scripts/")
DENY_FILES = ("Cargo.toml", "Cargo.lock", "deny.toml")
SHARED_KERNEL_CRATES = ("moldette-core",)
SHARED_KERNEL_MODULES = {
    "moldette-config": {"diagnostic", "error", "node", "position"},
    "moldette-parser": {"text"},
}
# Hubs reviewed in #60: widely used, but a change keeps its narrower scope.
REVIEWED_HUB_MODULES = {
    "moldette-config": {"arguments", "model"},
    "moldette-mapper": {"op"},
}
LOWERING_CRATE = "moldette-config"
FACADE_CRATE = "moldette"
FORMAT_CRATE = "moldette-parser"
TEST_TRACING_CRATE = "moldette-test-tracing"
HOST_ONLY_DIRS = ("examples/", "benches/", "crates/moldette-cli/")
LOWERING_TESTS = "--test lowering"
E2E_TESTS = "--test e2e"
ALLOCATION_TESTS = "--test allocations"
ALLOCATION_CRATES = (
    "moldette-bench-parser",
    "moldette-bench-mapper",
    "moldette-bench-resp",
    "moldette-bench-pipeline",
)
TRACING_TESTS = "--test tracing --features tracing"
SHARED_USE_WARNING = 3
SCOPES = ("check", "edge", "full")

Modules = Optional[set[str]]
"""Changed modules of a crate, or None when every module counts as changed."""
Filters = Optional[set[str]]
"""Test name filters of a test target, or None to run the whole target."""


@dataclass
class Workspace:
    dirs: dict[str, str]
    """Crate directory relative to the workspace root, by package name."""
    dependencies: dict[str, dict[str, bool]]
    """Workspace dependencies of each package, True unless dev-only."""

    def dependents(self, crate: str) -> dict[str, bool]:
        return {name: deps[crate] for name, deps in self.dependencies.items() if crate in deps}

    def users(self, crate: str, dev: bool) -> set[str]:
        """Return the crates that depend on `crate` directly or transitively,
        through dev-dependencies only when `dev` is set."""
        found: set[str] = set()
        pending = [crate]
        while pending:
            for dependent, normal in self.dependents(pending.pop()).items():
                if (normal or dev) and dependent not in found:
                    found.add(dependent)
                    pending.append(dependent)
        return found

    def is_host_only(self, crate: str) -> bool:
        return (self.dirs[crate] + "/").startswith(HOST_ONLY_DIRS)


def is_full_path(path: str) -> bool:
    return any(path.startswith(p) if p.endswith("/") else path == p for p in FULL_PATHS)


def runs_deny(files: list[str]) -> bool:
    return any(Path(f).name in DENY_FILES for f in files)


def module_of(path: Path, graph: Graph, kernels: set[str]) -> Optional[str]:
    """Return the module a file under a crate directory belongs to, or None
    when the file counts as a change to every module of the crate."""
    if path.parts[0] != "src" or path.suffix != ".rs":
        return None
    parts = file_module(Path("src"), path)
    if not parts or parts[0] not in graph or parts[0] in kernels:
        return None
    return parts[0]


def changed_modules(files: list[str], workspace: Workspace,
                    graphs: dict[str, Graph]) -> dict[str, Modules]:
    changes: dict[str, Modules] = {}
    for crate, crate_dir in workspace.dirs.items():
        for f in files:
            if not f.startswith(crate_dir + "/"):
                continue
            module = module_of(Path(f).relative_to(crate_dir), graphs.get(crate, {}),
                               SHARED_KERNEL_MODULES.get(crate, set()))
            current = changes.get(crate, set())
            changes[crate] = None if module is None or current is None else current | {module}
    return changes


def coupling_tests(crate: str, modules: Modules, dependent: str, graph: Graph,
                   e2e_modules: set[str]) -> Optional[dict[str, Filters]]:
    """Return the tests of `dependent` that cover its coupling to `crate`, or
    None when no coupling target is known and `dependent` runs in full."""
    if crate == TEST_TRACING_CRATE:
        return {TRACING_TESTS: None}
    if dependent == LOWERING_CRATE:
        layer = crate.removeprefix("moldette-")
        if modules is None:
            return {LOWERING_TESTS: {f"lower::{layer}::"}}
        users = {user for user, used in graph.items() if used & modules}
        return {LOWERING_TESTS: {f"lower::{layer}::{m}::" for m in modules | users}}
    if dependent == FACADE_CRATE:
        if crate == FORMAT_CRATE and modules is not None and modules <= e2e_modules:
            return {E2E_TESTS: {f"{m}::" for m in modules}}
        return {E2E_TESTS: None}
    return None


def raise_scope(scopes: dict[str, str], crate: str, scope: str) -> None:
    if crate not in scopes or SCOPES.index(scope) > SCOPES.index(scopes[crate]):
        scopes[crate] = scope


def merge_tests(into: dict[str, Filters], tests: dict[str, Filters]) -> None:
    for target, filters in tests.items():
        current = into.get(target, set())
        into[target] = None if filters is None or current is None else current | filters


def render_tests(tests: dict[str, Filters]) -> list[str]:
    return [target if filters is None else f"{target} -- {' '.join(sorted(filters))}"
            for target, filters in sorted(tests.items())]


def matrix(scopes: dict[str, str], tests: dict[str, dict[str, Filters]],
           workspace: Workspace) -> list[dict]:
    return [{"crate": crate, "scope": scope,
             "tests": render_tests(tests[crate]) if scope == "edge" else [],
             "wasm": not workspace.is_host_only(crate)}
            for crate, scope in sorted(scopes.items())]


def full_plan(workspace: Workspace) -> list[dict]:
    return matrix({crate: "full" for crate in workspace.dirs}, {}, workspace)


def plan(files: list[str], workspace: Workspace, graphs: dict[str, Graph],
         e2e_modules: set[str], lock_changed: set[str] = frozenset()) -> list[dict]:
    """Return the CI matrix entries `{crate, scope, tests}` for changed files."""
    if any(is_full_path(f) for f in files):
        return full_plan(workspace)
    scopes = {crate: "full" for crate in lock_changed}
    tests: dict[str, dict[str, Filters]] = {}
    changes = changed_modules(files, workspace, graphs)
    for crate, modules in changes.items():
        raise_scope(scopes, crate, "full")
        if crate in SHARED_KERNEL_CRATES:
            for user in workspace.users(crate, dev=True):
                raise_scope(scopes, user, "full")
            continue
        for dependent, normal in workspace.dependents(crate).items():
            coupling = coupling_tests(crate, modules, dependent, graphs.get(crate, {}), e2e_modules)
            if coupling is None:
                raise_scope(scopes, dependent, "full")
            else:
                raise_scope(scopes, dependent, "edge")
                merge_tests(tests.setdefault(dependent, {}), coupling)
            if normal:
                for indirect in workspace.users(dependent, dev=False):
                    raise_scope(scopes, indirect, "check")
    for crate in scopes:
        if workspace.is_host_only(crate) and crate not in changes and crate not in lock_changed:
            if crate in ALLOCATION_CRATES:
                scopes[crate] = "edge"
                tests[crate] = {ALLOCATION_TESTS: None}
            else:
                scopes[crate] = "check"
    return matrix(scopes, tests, workspace)


def lock_packages(text: str) -> dict[tuple, list[str]]:
    """Map each `(name, version, source)` package in a lock file to its
    dependency entries."""
    packages = {}
    for block in text.split("[[package]]")[1:]:
        fields = dict(re.findall(r'^(name|version|source) = "([^"]*)"', block, re.MULTILINE))
        listed = re.search(r"^dependencies = \[(.*?)^\]", block, re.MULTILINE | re.DOTALL)
        key = (fields["name"], fields["version"], fields.get("source"))
        packages[key] = re.findall(r'"([^"]+)"', listed.group(1)) if listed else []
    return packages


def lock_trees(text: str, crates: set[str]) -> dict[str, set[tuple]]:
    """Return the packages each crate reaches in a lock file."""
    packages = lock_packages(text)
    by_name: dict[str, list[tuple]] = {}
    for key in packages:
        by_name.setdefault(key[0], []).append(key)

    def resolve(entry: str) -> list[tuple]:
        name, *rest = entry.split(" ")
        return [k for k in by_name.get(name, []) if not rest or k[1] == rest[0]]

    trees = {}
    for crate in crates:
        reached: set[tuple] = set()
        pending = list(by_name.get(crate, []))
        while pending:
            key = pending.pop()
            if key not in reached:
                reached.add(key)
                pending += [dep for entry in packages[key] for dep in resolve(entry)]
        trees[crate] = reached
    return trees


def lock_changes(base: str, head: str, crates: set[str]) -> set[str]:
    """Return the crates whose resolved dependency tree differs between two
    lock files."""
    before, after = lock_trees(base, crates), lock_trees(head, crates)
    return {crate for crate in crates if before[crate] != after[crate]}


def shared_module_warnings(graphs: dict[str, Graph]) -> list[str]:
    warnings = []
    for crate, graph in sorted(graphs.items()):
        known = SHARED_KERNEL_MODULES.get(crate, set()) | REVIEWED_HUB_MODULES.get(crate, set())
        for module in sorted(graph):
            users = sorted(user for user, used in graph.items() if module in used)
            if module not in known and len(users) >= SHARED_USE_WARNING:
                warnings.append(f"{crate}: module `{module}` is used by {', '.join(users)} "
                                "and is neither a shared kernel nor a reviewed hub in scripts/affected.py")
    return warnings


def e2e_modules(path: Path) -> set[str]:
    if not path.is_file():
        return set()
    code = strip_comments_and_literals(path.read_text(encoding="utf-8"))
    return set(re.findall(r"^mod\s+(\w+)\s*\{", code, re.MULTILINE))


def git(*args: str) -> str:
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def load_workspace() -> Workspace:
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        check=True, capture_output=True, text=True).stdout)
    root = Path(meta["workspace_root"])
    names = {pkg["name"] for pkg in meta["packages"]}
    dirs, dependencies = {}, {}
    for pkg in meta["packages"]:
        dirs[pkg["name"]] = Path(pkg["manifest_path"]).parent.relative_to(root).as_posix()
        deps: dict[str, bool] = {}
        for dep in pkg["dependencies"]:
            if dep["name"] in names:
                deps[dep["name"]] = deps.get(dep["name"], False) or dep["kind"] != "dev"
        dependencies[pkg["name"]] = deps
    return Workspace(dirs, dependencies)


def main() -> None:
    workspace = load_workspace()
    if sys.argv[1:] == ["--full"]:
        entries, deny = full_plan(workspace), True
    else:
        base, head = sys.argv[1], sys.argv[2]
        root = Path(__file__).resolve().parent.parent
        files = [line for line in git("diff", "--name-only", f"{base}...{head}").splitlines() if line]
        graphs = crate_graphs(root)
        lock_changed: set[str] = set()
        if "Cargo.lock" in files:
            merge_base = git("merge-base", base, head).strip()
            lock_changed = lock_changes(git("show", f"{merge_base}:Cargo.lock"),
                                        git("show", f"{head}:Cargo.lock"), set(workspace.dirs))
        facade_e2e = root / workspace.dirs.get(FACADE_CRATE, "") / "tests" / "e2e.rs"
        entries = plan(files, workspace, graphs, e2e_modules(facade_e2e), lock_changed)
        deny = runs_deny(files)
        prefix = "::warning::" if os.environ.get("GITHUB_ACTIONS") == "true" else "warning: "
        for warning in shared_module_warnings(graphs):
            print(prefix + warning, file=sys.stderr)
    print(f"matrix={json.dumps(entries)}")
    print(f"deny={'true' if deny else 'false'}")


if __name__ == "__main__":
    main()
