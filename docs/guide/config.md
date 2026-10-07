# Configuration and Aliases

A configuration file holds two things: **defaults** added to every command,
and **aliases** — named sets of options you call with `-a NAME`. Use it for
the flags you always type and for commands you run again and again.

```ini
# .kelora.ini
defaults = --no-emoji --input-tz Europe/Berlin

[aliases]
errors = -l error,critical -k timestamp,service,message
nginx  = -f combined
5xx    = --filter 'e.status >= 500' -k ts,status,path
```

```bash
kelora -a errors app.jsonl
kelora -a nginx -a 5xx access.log       # aliases combine
```

## Where Kelora looks

| File | Scope |
|---|---|
| `.kelora.ini` in the current directory or the nearest parent | project — commit it so the team shares the aliases |
| `~/.config/kelora/kelora.ini` (or `$XDG_CONFIG_HOME/kelora/kelora.ini`; `%APPDATA%\kelora\kelora.ini` on Windows) | personal |

Kelora reads both. Aliases from both files are available, the project's
version winning when a name exists in both; the project's `defaults` line
replaces the personal one.

| Option | Effect |
|---|---|
| `--show-config` | print the config file in effect |
| `--edit-config` | open it in `$EDITOR` |
| `--save-alias NAME …` | save the rest of the command line as an alias |
| `--config-file FILE` | use this file instead (also with `--save-alias`, `--edit-config`) |
| `--ignore-config` | ignore all config files (also: `KELORA_IGNORE_CONFIG=1`) |
| `-v` | show which file, defaults, and aliases were applied |

`--save-alias` is the easiest way to build an alias: refine a command until it
does what you want, then add `--save-alias NAME` and run it once more.

## How defaults and aliases combine with the command line

Defaults are inserted at the start of the command, and each alias is replaced
by its options where it appears; then the command line is parsed as usual.
So options you type yourself come later and win: with `defaults = --stats`,
`kelora --no-stats …` turns stats off. Most on/off options have a `--no-…`
form for exactly this (`--no-stats`, `--no-strict`, `--no-parallel`,
`--no-silent`, …).

Input and output formats are the exception: if you pass `-f`/`-j` or
`-F`/`-J`, the same option is removed from `defaults` instead of conflicting
with it.

Aliases can use other aliases (`-a` inside an alias), up to 10 levels deep.
Quote values with spaces as you would in the shell.

## Beyond aliases

For longer logic, keep the script in a file and reference it from an alias:

```ini
[aliases]
enrich = -I helpers.rhai -E enrich.rhai
```

For multi-step jobs — fetch logs, run Kelora, archive the output — use a shell
script or a `justfile` that calls Kelora with an alias.
