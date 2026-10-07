# Exit Codes

| Code | Meaning |
|---|---|
| 0 | the run did its job |
| 1 | the run failed (see below), or any error with `--strict` |
| 2 | invalid command line or config file: unknown option, bad value, most conflicting options |
| 130 | interrupted (Ctrl-C) |
| 134 | internal error (a bug — please report it) |
| 141 | broken pipe: the reader went away, e.g. `head` closed the pipe |
| 143 | terminated (SIGTERM) |

## When is a run successful?

Kelora is resilient by default: problems with individual lines or events are
reported on stderr and counted, but do not fail the run. The exit code is 1
only when the run as a whole couldn't do its job:

| Situation | Exit code |
|---|---|
| some lines don't parse | 0 |
| an `--exec` script fails on some or all events (each failure is rolled back) | 0 |
| a filter matches nothing | 0 |
| **no** line parses — the format is wrong | 1 |
| a `--filter` fails on **every** event — a broken filter selected nothing | 1 |
| a named input file can't be opened (even if other files worked) | 1 |
| an `--assert` fails for any event | 1 |
| a forbidden operation, e.g. changing `conf` outside `--begin` | 1 |
| `--merge-sorted` finds a file out of order | 1 |

With `--strict`, the first parse, filter, or script error stops the run with
exit code 1.

## Using exit codes

Matching nothing is not an error, so the exit code doesn't tell you whether
events matched. Assert what must hold instead:

```bash
kelora ci.log -q --assert 'e.level != "ERROR"' || echo "log contains errors"
```

To test whether anything matched, look at the output:

```bash
if kelora app.jsonl -l error -n 1 | grep -q .; then echo "errors found"; fi
```

`141` after `| head` is normal: `head` closed the pipe once it had enough. In
scripts with `set -o pipefail`, check for it explicitly or avoid the early
exit.

See [How It Works](../how-it-works.md#when-something-goes-wrong) for what
Kelora does with each kind of error.
