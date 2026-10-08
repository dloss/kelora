#!/usr/bin/env python3
"""Check (or update) the sample output shown in README.md.

The README is static Markdown, so unlike the docs its examples are not executed
at build time. A ```bash block holding one `kelora ...` command, followed by a
plain ``` block, is treated as command + output: the command runs from the
repository root on an 80-column pseudo-terminal (as in dev/docs_tty_hook.py),
colors are stripped, and the result must match the output block.

    dev/readme_check.py           # exit 1 and show a diff on mismatch
    dev/readme_check.py --update  # rewrite the output blocks in place

`kelora` must be on PATH. Commands without an output block are covered by
tests/help_examples_tests.rs (they must exit 0).
"""

import difflib
import fcntl
import os
import pathlib
import pty
import re
import struct
import subprocess
import sys
import termios

COLUMNS = 80
ROOT = pathlib.Path(__file__).resolve().parent.parent
README = ROOT / "README.md"
ANSI = re.compile(r"\x1b\[[0-9;]*m|\x1b\]8;;[^\x1b]*\x1b\\")
PAIR = re.compile(r"```bash\n(kelora [^\n]*)\n```\n\n```\n(.*?)```\n", re.S)


def run(command):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 50, COLUMNS, 0, 0))
    attrs = termios.tcgetattr(slave)
    attrs[1] &= ~termios.ONLCR  # keep "\n" as "\n"
    termios.tcsetattr(slave, termios.TCSANOW, attrs)
    env = dict(os.environ, COLUMNS=str(COLUMNS), KELORA_IGNORE_CONFIG="1")
    process = subprocess.Popen(  # noqa: S603
        ["bash", "-c", command],  # noqa: S607
        cwd=ROOT,
        stdin=subprocess.DEVNULL,
        stdout=slave,
        stderr=slave,
        env=env,
        close_fds=True,
    )
    os.close(slave)
    chunks = []
    while True:
        try:
            data = os.read(master, 65536)
        except OSError:  # EIO: all writers closed the pty
            break
        if not data:
            break
        chunks.append(data)
    os.close(master)
    process.wait()
    output = ANSI.sub("", b"".join(chunks).decode("utf-8", errors="replace"))
    lines = [line.rstrip() for line in output.strip("\n").split("\n")]
    if process.returncode != 0:
        sys.exit(f"README command failed ({process.returncode}): {command}\n" + "\n".join(lines))
    return "\n".join(lines) + "\n"


def main():
    update = "--update" in sys.argv[1:]
    text = README.read_text()
    pairs = list(PAIR.finditer(text))
    if not pairs:
        sys.exit("README.md: no command/output pairs found; did the layout change?")
    failed = False

    def replace(match):
        nonlocal failed
        command, shown = match.group(1), match.group(2)
        actual = run(command)
        if actual != shown:
            if not update:
                failed = True
                sys.stdout.writelines(
                    difflib.unified_diff(
                        shown.splitlines(keepends=True),
                        actual.splitlines(keepends=True),
                        f"README.md: {command}",
                        "actual output",
                    )
                )
        return f"```bash\n{command}\n```\n\n```\n{actual}```\n"

    new = PAIR.sub(replace, text)
    if update:
        if new != text:
            README.write_text(new)
            print(f"README.md: updated {len(pairs)} output block(s)")
        return
    if failed:
        sys.exit("README.md output is stale; run `just readme-update` and review the diff")
    print(f"README.md: {len(pairs)} output block(s) match")


if __name__ == "__main__":
    main()
