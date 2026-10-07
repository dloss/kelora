"""MkDocs hook: run markdown-exec bash/sh blocks with stdout on a pseudo-terminal.

Kelora picks output details by whether stdout is a terminal: summary tables vs.
TSV for -m/--freq/--span-summary/--drain-diff, map legends, word wrapping. The
docs should show what a reader sees when they type the command, so each block
runs with stdout and stderr attached to a pty (80 columns). Pipes *inside* a
block stay real pipes, so `kelora ... | head` still shows the piped behavior.
"""

import fcntl
import os
import pty
import struct
import subprocess
import termios

from markdown_exec._internal.formatters import bash as _bash
from markdown_exec._internal.formatters import sh as _sh
from markdown_exec._internal.formatters.base import ExecutionError
from markdown_exec._internal.rendering import code_block

COLUMNS = 80


def _run_in_pty(code, returncode=None, session=None, id=None, **extra):  # noqa: A002, ARG001
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 50, COLUMNS, 0, 0))
    attrs = termios.tcgetattr(slave)
    attrs[1] &= ~termios.ONLCR  # keep "\n" as "\n"
    termios.tcsetattr(slave, termios.TCSANOW, attrs)
    env = dict(os.environ, COLUMNS=str(COLUMNS))
    process = subprocess.Popen(  # noqa: S603
        ["bash", "-c", code],  # noqa: S607
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
    output = b"".join(chunks).decode("utf-8", errors="replace")
    if process.returncode != (returncode or 0):
        raise ExecutionError(code_block("sh", output, **extra), process.returncode)
    return output


_bash._run_bash = _run_in_pty
_sh._run_sh = _run_in_pty
