import unittest

from affected import Workspace, lock_changes, plan, runs_deny, runs_fuzz, shared_module_warnings

CORE, PARSER, MAPPER, RESP = "moldette-core", "moldette-parser", "moldette-mapper", "moldette-resp"
CONFIG, FACADE, TEST_TRACING = "moldette-config", "moldette", "moldette-test-tracing"
EXAMPLE, BENCHES, CLI = "moldette-otlp-example", "moldette-bench-pipeline", "moldette-cli"

WORKSPACE = Workspace(
    dirs={**{name: f"crates/{name}" for name in (CORE, PARSER, MAPPER, RESP, CONFIG, FACADE, TEST_TRACING, CLI)},
          EXAMPLE: "examples/otlp", BENCHES: "benches/pipeline"},
    dependencies={
        CORE: {},
        PARSER: {CORE: True, TEST_TRACING: False},
        MAPPER: {CORE: True, TEST_TRACING: False},
        RESP: {CORE: True, TEST_TRACING: False},
        CONFIG: {CORE: True, PARSER: True, MAPPER: True, RESP: True},
        FACADE: {CORE: True, PARSER: True, MAPPER: True, RESP: True, CONFIG: True,
                 TEST_TRACING: False},
        TEST_TRACING: {},
        CLI: {FACADE: True},
        EXAMPLE: {FACADE: True},
        BENCHES: {FACADE: True},
    },
)

GRAPHS = {
    CORE: {"value": set(), "schema": set()},
    PARSER: {
        "custom": {"text"}, "delimited": {"text"}, "json": {"text"}, "kv": {"text"},
        "regex": {"text"}, "telemetry": set(), "text": set(),
        "spec": {"custom", "delimited", "json", "kv", "regex", "telemetry"},
    },
    MAPPER: {
        "coerce": set(), "op": set(), "recorder": {"op"}, "telemetry": set(),
        "rule": {"coerce", "op", "recorder", "telemetry"},
        "spec": {"op", "recorder", "rule", "telemetry"},
    },
    RESP: {"spec": {"telemetry"}, "telemetry": set()},
    CONFIG: {
        "position": set(), "diagnostic": {"position"}, "error": {"diagnostic", "position"},
        "model": {"error"}, "lineage": {"model"}, "lower": {"error", "lineage", "model"},
    },
    FACADE: {"pipeline": set()},
}

E2E_MODULES = {"delimited", "json", "kv", "regex"}


def entry(crate, scope, *tests):
    return {"crate": crate, "scope": scope, "tests": list(tests), "wasm": crate not in (EXAMPLE, BENCHES, CLI)}


ALL_FULL = [entry(name, "full") for name in sorted(WORKSPACE.dirs)]


def scoped(*files, lock_changed=frozenset()):
    return plan(list(files), WORKSPACE, GRAPHS, E2E_MODULES, set(lock_changed))


def lock(*packages):
    blocks = []
    for name, version, source, dependencies in packages:
        lines = ["[[package]]", f'name = "{name}"', f'version = "{version}"']
        if source:
            lines.append(f'source = "{source}"')
        if dependencies:
            lines += ["dependencies = ["] + [f' "{d}",' for d in dependencies] + ["]"]
        blocks.append("\n".join(lines))
    return "version = 4\n\n" + "\n\n".join(blocks) + "\n"


REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"


class PlanTest(unittest.TestCase):
    def test_parser_format_runs_lowering_of_the_format_and_spec_and_its_e2e_module(self):
        self.assertEqual(scoped("crates/moldette-parser/src/kv.rs"), [
            entry(FACADE, "edge", "--test e2e -- kv::"),
            entry(BENCHES, "edge", "--test allocations"),
            entry(CLI, "check"),
            entry(CONFIG, "edge", "--test lowering -- lower::parser::kv:: lower::parser::spec::"),
            entry(EXAMPLE, "check"),
            entry(PARSER, "full"),
        ])

    def test_parser_format_without_e2e_module_runs_whole_e2e(self):
        self.assertIn(entry(FACADE, "edge", "--test e2e"), scoped("crates/moldette-parser/src/custom.rs"))

    def test_mapper_op_runs_lowering_of_op_and_its_users_and_whole_e2e(self):
        self.assertEqual(scoped("crates/moldette-mapper/src/op.rs"), [
            entry(FACADE, "edge", "--test e2e"),
            entry(BENCHES, "edge", "--test allocations"),
            entry(CLI, "check"),
            entry(CONFIG, "edge", "--test lowering -- lower::mapper::op:: lower::mapper::recorder:: "
                                  "lower::mapper::rule:: lower::mapper::spec::"),
            entry(MAPPER, "full"),
            entry(EXAMPLE, "check"),
        ])

    def test_config_lineage_runs_config_in_full_and_whole_e2e(self):
        self.assertEqual(scoped("crates/moldette-config/src/lineage.rs"), [
            entry(FACADE, "edge", "--test e2e"),
            entry(BENCHES, "edge", "--test allocations"),
            entry(CLI, "check"),
            entry(CONFIG, "full"),
            entry(EXAMPLE, "check"),
        ])

    def test_core_runs_every_library_crate_that_uses_it_in_full(self):
        self.assertEqual(scoped("crates/moldette-core/src/value.rs"),
                         [entry(e["crate"], "check") if e["crate"] in (EXAMPLE, CLI) else
                          entry(BENCHES, "edge", "--test allocations") if e["crate"] == BENCHES else e
                          for e in ALL_FULL if e["crate"] != TEST_TRACING])

    def test_test_tracing_runs_tracing_tests_of_dev_dependents_only(self):
        tracing = "--test tracing --features tracing"
        self.assertEqual(scoped("crates/moldette-test-tracing/src/lib.rs"), [
            entry(FACADE, "edge", tracing),
            entry(MAPPER, "edge", tracing),
            entry(PARSER, "edge", tracing),
            entry(RESP, "edge", tracing),
            entry(TEST_TRACING, "full"),
        ])

    def test_lock_change_runs_only_crates_whose_tree_changed(self):
        self.assertEqual(scoped("Cargo.lock", lock_changed={PARSER, CONFIG, FACADE}), [
            entry(FACADE, "full"), entry(CONFIG, "full"), entry(PARSER, "full"),
        ])
        self.assertTrue(runs_deny(["Cargo.lock"]))

    def test_docs_only_change_runs_no_crate(self):
        self.assertEqual(scoped("docs/PLAN.md", "README.md"), [])
        self.assertFalse(runs_deny(["docs/PLAN.md"]))

    def test_configuration_document_runs_its_examples(self):
        self.assertEqual(scoped("docs/CONFIGURATION.md"), [entry(CONFIG, "edge", "--test docs")])
        self.assertIn(entry(CONFIG, "full"), scoped("docs/CONFIGURATION.md", "crates/moldette-config/src/model.rs"))

    def test_configuration_examples_run_their_cli_tests(self):
        self.assertEqual(scoped("examples/configs/syslog/input.log"),
                         [entry(CLI, "edge", "--test cli -- examples::")])

    def test_parser_shared_kernel_counts_as_change_to_every_parser_module(self):
        self.assertEqual(scoped("crates/moldette-parser/src/text.rs"), [
            entry(FACADE, "edge", "--test e2e"),
            entry(BENCHES, "edge", "--test allocations"),
            entry(CLI, "check"),
            entry(CONFIG, "edge", "--test lowering -- lower::parser::"),
            entry(EXAMPLE, "check"),
            entry(PARSER, "full"),
        ])

    def test_crate_root_and_files_outside_src_count_as_change_to_every_module(self):
        expected = entry(CONFIG, "edge", "--test lowering -- lower::parser::")
        self.assertIn(expected, scoped("crates/moldette-parser/src/lib.rs"))
        self.assertIn(expected, scoped("crates/moldette-parser/tests/contract.rs"))

    def test_workspace_and_ci_files_run_every_crate_in_full(self):
        for path in ("Cargo.toml", "deny.toml", ".github/workflows/ci.yml", "scripts/affected.py"):
            self.assertEqual(scoped(path), ALL_FULL, path)

    def test_facade_change_compiles_example_and_benches_for_the_host(self):
        self.assertEqual(scoped("crates/moldette/src/pipeline.rs"), [
            entry(FACADE, "full"),
            entry(BENCHES, "edge", "--test allocations"),
            entry(CLI, "check"),
            entry(EXAMPLE, "check"),
        ])

    def test_example_change_runs_only_the_example_in_full_for_the_host(self):
        self.assertEqual(scoped("examples/otlp/src/main.rs"), [entry(EXAMPLE, "full")])
        self.assertEqual(scoped("Cargo.lock", lock_changed={EXAMPLE}), [entry(EXAMPLE, "full")])
        self.assertTrue(runs_deny(["examples/otlp/Cargo.toml"]))

    def test_benches_change_runs_only_the_benches_in_full_for_the_host(self):
        self.assertEqual(scoped("benches/pipeline/src/lib.rs"), [entry(BENCHES, "full")])

    def test_cli_change_runs_only_the_cli_in_full_for_the_host(self):
        self.assertEqual(scoped("crates/moldette-cli/src/check.rs"), [entry(CLI, "full")])
        self.assertEqual(scoped("crates/moldette-cli/tests/check.rs"), [entry(CLI, "full")])

    def test_coupling_tests_of_several_changes_are_merged(self):
        self.assertEqual(scoped("crates/moldette-parser/src/kv.rs", "crates/moldette-parser/src/json.rs",
                                "crates/moldette-mapper/src/coerce.rs")[:4], [
            entry(FACADE, "edge", "--test e2e"),
            entry(BENCHES, "edge", "--test allocations"),
            entry(CLI, "check"),
            entry(CONFIG, "edge", "--test lowering -- lower::mapper::coerce:: lower::mapper::rule:: "
                                  "lower::parser::json:: lower::parser::kv:: lower::parser::spec::"),
        ])

    def test_indirect_dependents_are_compiled(self):
        workspace = Workspace(
            dirs={"app": "crates/app", CONFIG: "crates/config", PARSER: "crates/parser"},
            dependencies={"app": {CONFIG: True}, CONFIG: {PARSER: True}, PARSER: {}},
        )
        self.assertEqual(plan(["crates/parser/src/kv.rs"], workspace, GRAPHS, E2E_MODULES), [
            entry("app", "check"),
            entry(CONFIG, "edge", "--test lowering -- lower::parser::kv:: lower::parser::spec::"),
            entry(PARSER, "full"),
        ])


class RunsFuzzTest(unittest.TestCase):
    def test_fuzz_targets_build_on_changes_to_the_facade_and_its_dependencies(self):
        for path in ("fuzz/fuzz_targets/kv.rs", "crates/moldette-core/src/value.rs",
                     "crates/moldette-parser/src/kv.rs", "crates/moldette/src/pipeline.rs",
                     ".github/workflows/fuzz.yml"):
            self.assertTrue(runs_fuzz([path], WORKSPACE), path)

    def test_fuzz_targets_skip_changes_outside_the_facade_tree(self):
        for path in ("crates/moldette-cli/src/run.rs", "crates/moldette-test-tracing/src/lib.rs",
                     "examples/otlp/src/main.rs", "benches/pipeline/src/lib.rs", "docs/PLAN.md"):
            self.assertFalse(runs_fuzz([path], WORKSPACE), path)


class LockChangesTest(unittest.TestCase):
    def test_reports_crates_that_reach_a_changed_package(self):
        def workspace_lock(serde_json):
            return lock(
                (CORE, "0.0.0", None, []),
                (PARSER, "0.0.0", None, [CORE, "serde_json"]),
                (MAPPER, "0.0.0", None, [CORE, "regex"]),
                (CONFIG, "0.0.0", None, [PARSER, MAPPER]),
                ("regex", "1.13.1", REGISTRY, []),
                ("serde_json", serde_json, REGISTRY, []),
            )

        self.assertEqual(lock_changes(workspace_lock("1.0.154"), workspace_lock("1.0.155"),
                                      {CORE, PARSER, MAPPER, CONFIG}), {PARSER, CONFIG})

    def test_resolves_entries_that_name_a_version(self):
        def workspace_lock(chosen):
            return lock(
                (PARSER, "0.0.0", None, [f"hashbrown {chosen}"]),
                ("hashbrown", "0.14.5", REGISTRY, []),
                ("hashbrown", "0.15.2", REGISTRY, []),
            )

        self.assertEqual(lock_changes(workspace_lock("0.14.5"), workspace_lock("0.15.2"), {PARSER}),
                         {PARSER})
        self.assertEqual(lock_changes(workspace_lock("0.15.2"), workspace_lock("0.15.2"), {PARSER}),
                         set())


class SharedModuleWarningsTest(unittest.TestCase):
    def test_skips_shared_kernels_and_reviewed_hubs(self):
        self.assertEqual(shared_module_warnings({MAPPER: GRAPHS[MAPPER], PARSER: GRAPHS[PARSER]}), [])

    def test_warns_on_other_modules_used_by_three_modules(self):
        graph = {module: {"codec" if used == "op" else used for used in uses}
                 for module, uses in GRAPHS[MAPPER].items() if module != "op"}
        graph["codec"] = set(GRAPHS[MAPPER]["op"])
        self.assertEqual(shared_module_warnings({MAPPER: graph}), [
            "moldette-mapper: module `codec` is used by recorder, rule, spec "
            "and is neither a shared kernel nor a reviewed hub in scripts/affected.py",
        ])

if __name__ == "__main__":
    unittest.main()
