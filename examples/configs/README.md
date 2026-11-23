# Configuration examples

Each directory holds a configuration, an input and the output of `moldette
run` for that input. The keys are described in `docs/CONFIGURATION.md`. CI
runs every example and compares the output.

| Directory | Input | Command |
|---|---|---|
| `access-log` | nginx access log, `combined` format | `moldette run access-log/config.yaml access-log/input.log` |
| `syslog` | syslog lines in the BSD format (RFC 3164) | `moldette run syslog/config.yaml syslog/input.log` |
| `json-api` | a JSON array of orders returned by an API | `moldette run --records json json-api/config.yaml json-api/input.json` |
| `csv-export` | order lines exported as CSV | `moldette run csv-export/config.yaml csv-export/input.csv` |

The last input of each example shows a record that does not fit: a request
line that is not HTTP, a line without a priority, an amount that is not a
number, or a quantity that is not an integer.

BSD syslog lines carry no year and no offset, so the `syslog` configuration
gives both to the `timestamp` op. The CSV input has no header row, because a
header row would be read as a record.
