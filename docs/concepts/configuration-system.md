# Configuration System

Kelora reads an INI-style configuration file before parsing CLI flags. The goal
is to capture everyday defaults, share reusable command snippets, and support
teams that maintain project-specific log pipelines.

## File Locations and Precedence

Kelora searches for configuration files in the following order:

1. **Project config** – the nearest `.kelora.ini` walking up from the current
   directory.

2. **User config** – `$XDG_CONFIG_HOME/kelora/kelora.ini` on Unix, or
   `%APPDATA%\kelora\kelora.ini` on Windows.

Project files override user files. You can override both by passing
`--config-file <path>` or ignore them entirely with `--ignore-config`.

Inspect what Kelora currently sees with `--show-config`:

=== "Command"

    ```bash
    kelora --show-config
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora --show-config
    ```

If no file exists you will see an example template and the paths that were
searched.

## INI Structure

Kelora recognizes two sections:

- `defaults` (root level) – prepended to every command before user-supplied
  flags.

- `[aliases]` – reusable snippets invoked with `--alias <name>` (short flag
  `-a`). Aliases can reference other aliases recursively up to a depth of 10.

Example `.kelora.ini`:

```
defaults = -f auto --stats

[aliases]
errors = -l error --stats
slow-json = -f json --filter 'to_int_or(e.duration_ms, 0) > 1000'
```

Running `kelora app.log` with this file automatically adds `-f auto --stats` to
the invocation. `kelora --alias errors app.log` expands to
`kelora -l error --stats app.log`.

## Creating and Editing Config Files

- `--edit-config` opens the active config in `$EDITOR` (default `vi` on Unix,
  `notepad.exe` on Windows). Kelora creates parent directories if needed.

- `--config-file path/to/file.ini` scopes edits to a specific file. Combine
  this with `--save-alias` to keep examples alongside project code.

### Saving an Alias Programmatically

`--save-alias` writes (or updates) an entry without leaving your editor. The
example below stores an alias in a temporary file under `dev/` and then removes
it so the repository stays clean.

=== "Command"

    ```bash
    kelora --save-alias stacktrace \
      --config-file dev/kelora-demo.ini \
      -f raw --multiline timestamp --filter 'e.raw.contains("Traceback")'

    cat dev/kelora-demo.ini
    rm dev/kelora-demo.ini
    ```

=== "Output"

    ```bash exec="on" source="above" result="ansi"
    kelora --save-alias stacktrace \
      --config-file dev/kelora-demo.ini \
      -f raw --multiline timestamp --filter 'e.raw.contains("Traceback")'

    cat dev/kelora-demo.ini
    rm dev/kelora-demo.ini
    ```

Kelora reports whether the alias replaced an existing value. Alias names must
match the regex `^[a-zA-Z_][a-zA-Z0-9_-]{0,63}$`.

## How Argument Expansion Works

1. **Defaults applied** – the `defaults` string is parsed with shell-style
   quoting (`shell_words`) and inserted immediately after the executable name.

2. **Alias expansion** – each `--alias name` (or `-a name`) is replaced with the
   corresponding argument list. Aliases can reference other aliases by using
   `--alias other-alias` inside their definition.

3. **CLI parsing** – the augmented argument vector is passed to Clap, and those
   resolved flags are what the pipeline sees.

Implications:

- User-provided flags take precedence over defaults if they appear later on the
  command line (standard CLI precedence rules). For example, `defaults = --stats`
  and `kelora --no-stats` results in stats being disabled.

- Quote arguments that contain spaces inside the config file just as you would
  in a shell.

- Recursive aliases are allowed but guarded by a maximum depth of 10 to prevent
  infinite loops.

## Ignoring or Switching Configurations

- `--ignore-config` bypasses both defaults and aliases, useful for clean-room
  troubleshooting or one-off commands.

- `--config-file` works with every config-aware flag (`--show-config`,
  `--edit-config`, `--save-alias`). This makes it easy to ship presets within a
  project directory while keeping personal settings separate.

## Configuration vs CLI Flags

| Scenario | Recommended Approach |
|----------|---------------------|
| Global formatting or stats | `defaults = -f json --stats` |
| Reusable command recipes | `[aliases]` entries; call with `--alias` |
| One-time experimentation | Plain CLI flags (`--ignore-config` if needed) |
| Team-shared profiles | Commit `.kelora.ini` to the repo root |
| Environment-specific tweaks | Use project `.kelora.ini` plus per-user overrides |

## Best Practices

- **Keep aliases small** – Compose pipelines by chaining multiple aliases via
  `--alias` rather than stuffing every flag into a single macro.

- **Document heavy aliases** – Use the repository README or comments in
  `.kelora.ini` to explain when to use each alias.

- **Pair with `just` or shell scripts** – For multi-step workflows (downloading
  logs, running Kelora, archiving output) keep orchestration in a script and
  have the script call the alias.

- **Review before committing** – Project-level `.kelora.ini` affects every run
  from that directory. Make sure defaults remain sensible for newcomers.

## Related Topics

- [CLI Reference – Configuration Options](../reference/cli-reference.md#configuration-options)
  lists every config-aware flag.

- [Scripting Stages](scripting-stages.md) covers the Rhai pipeline, which you
  can drive via aliases for complex transformations.

- [Tutorial: Metrics and Tracking](../tutorials/metrics-and-tracking.md) shows
  how to package common analytics commands as aliases.
