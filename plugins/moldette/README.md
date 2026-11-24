# moldette plugin for Claude Code

This plugin adds one skill, `moldette`, which helps write, check, debug and
deploy configurations for [moldette](https://github.com/CharmingGroot/moldette).
moldette turns raw records, such as JSON, CSV, nginx or syslog lines and
`key=value` pairs, into records of a schema declared in a YAML or JSON file.

## Install

    /plugin marketplace add CharmingGroot/moldette
    /plugin install moldette@moldette

## What the skill does

The skill is used when a conversation is about moldette: writing or editing a
configuration from sample records, finding why a field comes out `emptied`,
`missing_required` or `malformed`, reading an error from `moldette check`, or
running `moldette-sidecar` with several configurations, metrics or Kubernetes.

It works from the moldette documents copied in `skills/moldette/references`,
and it runs the `moldette` command (`check`, `run`, `explain`) when the command
is installed. It does not install software, does not fetch anything from the
network and sends no data anywhere; installing the `moldette` command is left
to the user.

The plugin follows the version of moldette. Where an older release behaves
differently, the skill says so.

## License

Apache-2.0, as moldette.
