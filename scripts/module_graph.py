"""Check that the modules of each crate form no cycle (#56).

A node is a top-level module of a crate; files in its subdirectory belong to
it. An edge is a path through `crate::`, `super::` or `self::`, in a `use`
item or in code. A path to a name re-exported by the crate root counts as an
edge to the module the name comes from. The crate root itself is not a node.

Usage: module_graph.py [--print]
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

Graph = dict[str, set[str]]

ROOT_FILES = ("lib.rs", "main.rs")
USE_ITEM = re.compile(r"\buse\s+([^;]+);")
INLINE_PATH = re.compile(r"\b(?:crate|super|self)(?:\s*::\s*\w+)+")
INLINE_MOD = re.compile(r"\bmod\s+(\w+)\s*\{")
PACKAGE_NAME = re.compile(r'^\s*name\s*=\s*"([^"]+)"', re.MULTILINE)


def strip_comments_and_literals(source: str) -> str:
    """Blank out comments, string and character literals, keeping offsets."""
    out = list(source)
    i, n = 0, len(source)

    def blank(start: int, end: int) -> None:
        for k in range(start, min(end, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        if source.startswith("//", i):
            end = source.find("\n", i)
            end = n if end == -1 else end
            blank(i, end)
            i = end
        elif source.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if source.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif source.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            blank(i, j)
            i = j
        elif raw := re.match(r'b?r(#*)"', source[i:i + 260]):
            closing = '"' + raw.group(1)
            end = source.find(closing, i + raw.end())
            end = n if end == -1 else end + len(closing)
            blank(i, end)
            i = end
        elif source[i] == '"':
            j = i + 1
            while j < n and source[j] != '"':
                j += 2 if source[j] == "\\" else 1
            blank(i, j + 1)
            i = j + 1
        elif char := re.match(r"'(?:\\'|\\[^']{1,10}|[^\\'])'", source[i:i + 14]):
            blank(i, i + char.end())
            i += char.end()
        else:
            i += 1
    return "".join(out)


def expand_use_tree(tree: str) -> list[list[str]]:
    """Expand `a::{b, c::{d}}` into `[[a, b], [a, c, d]]`."""
    tree = re.sub(r"\s+", "", re.sub(r"\s+as\s+\w+", "", tree))
    paths: list[list[str]] = []

    def walk(text: str, prefix: list[str]) -> None:
        brace = text.find("{")
        if brace == -1:
            segments = [s for s in text.split("::") if s]
            paths.append(prefix + segments)
            return
        head = [s for s in text[:brace].split("::") if s]
        body = text[brace + 1:text.rfind("}")]
        depth, start = 0, 0
        for k, c in enumerate(body + ","):
            if c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
            elif c == "," and depth == 0:
                if body[start:k]:
                    walk(body[start:k], prefix + head)
                start = k + 1

    walk(tree, [])
    return paths


def file_module(src: Path, file: Path) -> list[str]:
    parts = list(file.relative_to(src).with_suffix("").parts)
    if parts[-1] == "mod" or (len(parts) == 1 and file.name in ROOT_FILES):
        parts = parts[:-1]
    return parts


def inline_modules(code: str) -> list[tuple[int, int, str]]:
    """Return (start, end, name) offsets of each `mod name { ... }` block."""
    blocks = []
    for found in INLINE_MOD.finditer(code):
        depth, k = 0, found.end() - 1
        while k < len(code):
            depth += {"{": 1, "}": -1}.get(code[k], 0)
            if depth == 0:
                break
            k += 1
        blocks.append((found.start(), k, found.group(1)))
    return blocks


def scope_at(base: list[str], blocks: list[tuple[int, int, str]], offset: int) -> list[str]:
    return base + [name for start, end, name in blocks if start < offset < end]


def resolve(path: list[str], scope: list[str]) -> list[str] | None:
    """Return the absolute module path of `path` used in `scope`."""
    if not path or path[0] not in ("crate", "super", "self"):
        return None
    current = list(scope)
    for k, segment in enumerate(path):
        if segment == "crate":
            current = []
        elif segment == "super":
            if not current:
                return None
            current.pop()
        elif segment != "self":
            return current + path[k:]
    return current


def root_reexports(root_code: str) -> dict[str, str]:
    """Map each name the crate root re-exports to the module it comes from."""
    names = {}
    for found in re.finditer(r"\bpub\s+use\s+([^;]+);", root_code):
        for path in expand_use_tree(found.group(1)):
            if len(path) >= 2 and path[0] not in ("crate", "self", "super"):
                names[path[-1]] = path[0]
            elif len(path) >= 3 and path[0] in ("crate", "self"):
                names[path[-1]] = path[1]
    return names


def module_graph(src: Path) -> Graph:
    """Build the graph of the top-level modules under the `src` directory."""
    files = [f for f in sorted(src.rglob("*.rs")) if f.relative_to(src).parts[0] != "bin"]
    sources = {f: strip_comments_and_literals(f.read_text(encoding="utf-8")) for f in files}
    modules = {tuple(file_module(src, f)) for f in files}
    graph: Graph = {m[0]: set() for m in modules if m}
    reexports = {}
    for f, code in sources.items():
        if not file_module(src, f):
            reexports.update(root_reexports(code))

    for f, code in sources.items():
        base = file_module(src, f)
        blocks = inline_modules(code)
        found_paths: list[tuple[int, list[str]]] = []
        for item in USE_ITEM.finditer(code):
            found_paths += [(item.start(), p) for p in expand_use_tree(item.group(1))]
        code_only = USE_ITEM.sub(lambda m: " " * len(m.group(0)), code)
        for found in INLINE_PATH.finditer(code_only):
            found_paths.append((found.start(), re.sub(r"\s+", "", found.group(0)).split("::")))

        for offset, path in found_paths:
            target = resolve(path, scope_at(base, blocks, offset))
            if not target:
                continue
            if target[0] not in graph and target[0] in reexports:
                target = [reexports[target[0]]]
            if base and target[0] in graph and target[0] != base[0]:
                graph[base[0]].add(target[0])
    return graph


def strongly_connected_components(graph: Graph) -> list[list[str]]:
    """Return the strongly connected components of `graph` (Tarjan)."""
    index: dict[str, int] = {}
    low: dict[str, int] = {}
    stack: list[str] = []
    on_stack: set[str] = set()
    components: list[list[str]] = []

    def visit(node: str) -> None:
        index[node] = low[node] = len(index)
        stack.append(node)
        on_stack.add(node)
        for target in sorted(graph.get(node, ())):
            if target not in index:
                visit(target)
                low[node] = min(low[node], low[target])
            elif target in on_stack:
                low[node] = min(low[node], index[target])
        if low[node] == index[node]:
            component = []
            while True:
                member = stack.pop()
                on_stack.discard(member)
                component.append(member)
                if member == node:
                    break
            components.append(sorted(component))

    for node in sorted(graph):
        if node not in index:
            visit(node)
    return components


def crate_graphs(workspace: Path) -> dict[str, Graph]:
    """Return the module graph of each crate under `crates/`, by package name."""
    graphs = {}
    for manifest in sorted(workspace.glob("crates/*/Cargo.toml")):
        found = PACKAGE_NAME.search(manifest.read_text(encoding="utf-8"))
        src = manifest.parent / "src"
        if found and src.is_dir():
            graphs[found.group(1)] = module_graph(src)
    return graphs


def main() -> None:
    graphs = crate_graphs(Path(__file__).resolve().parent.parent)
    failed = False
    for name, graph in graphs.items():
        if "--print" in sys.argv[1:]:
            print(name)
            for module in sorted(graph):
                print(f"  {module} -> {', '.join(sorted(graph[module])) or '-'}")
        for component in strongly_connected_components(graph):
            if len(component) > 1:
                failed = True
                print(f"{name}: module cycle: {', '.join(component)}", file=sys.stderr)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
