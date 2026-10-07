# CLI Options

Every command-line option, grouped as in `kelora --help`. This page is
generated from the help text of the Kelora version it documents, so the two
always agree. In the terminal, `kelora --help KEYWORD` shows only the options
matching a keyword, and `kelora -h` prints a one-screen summary.

Topic references in the terminal: `--help-formats`, `--help-time`,
`--help-multiline`, `--help-regex`, `--help-rhai`, `--help-functions`,
`--help-examples`.

```python exec="on" idprefix=""
import sys

sys.path.insert(0, "dev")
from docs_cli_reference import help_text, render

print(render(help_text()))
```
