use std::fs;

use super::moldette;

const EXAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/configs");

/// Each directory of `examples/configs`, its input and the options of
/// `moldette run`, as listed in its README.
const RUNS: [(&str, &str, &[&str]); 4] = [
    ("access-log", "input.log", &[]),
    ("csv-export", "input.csv", &[]),
    ("json-api", "input.json", &["--records", "json"]),
    ("syslog", "input.log", &[]),
];

#[test]
fn every_example_directory_is_run() {
    let mut directories: Vec<String> = fs::read_dir(EXAMPLES)
        .expect("examples directory")
        .map(|entry| entry.expect("directory entry"))
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().into_string().expect("UTF-8 name"))
        .collect();
    directories.sort();
    let listed: Vec<&str> = RUNS.iter().map(|(name, _, _)| *name).collect();
    assert_eq!(directories, listed);
}

#[test]
fn examples_check_cleanly_and_print_the_expected_output() {
    for (name, input, options) in RUNS {
        let dir = format!("{EXAMPLES}/{name}");
        let config = format!("{dir}/config.yaml");
        let check = moldette(&["check", &config]);
        assert!(check.status.success(), "{name}: check failed");
        assert!(
            check.stderr.is_empty(),
            "{name}: {}",
            String::from_utf8_lossy(&check.stderr)
        );
        let input = format!("{dir}/{input}");
        let mut args = vec!["run"];
        args.extend_from_slice(options);
        args.extend([config.as_str(), input.as_str()]);
        let run = moldette(&args);
        assert!(run.status.success(), "{name}: run failed");
        let expected =
            fs::read_to_string(format!("{dir}/expected.ndjson")).expect("expected output");
        assert_eq!(String::from_utf8_lossy(&run.stdout), expected, "{name}");
    }
}
