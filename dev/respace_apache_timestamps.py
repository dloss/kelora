#!/usr/bin/env python3
"""Respace timestamps in an Apache/combined access log read from stdin.

flog's `-n COUNT` mode stamps every generated line with the single invocation
time, so the raw output shares one timestamp. That makes --discover report
Uniq 1 for the time field and makes --since/--until untestable on the file.

This filter rewrites each line's `[dd/Mon/yyyy:HH:MM:SS +ZZZZ]` timestamp with
an ascending value, using exponential inter-arrival gaps (Poisson traffic) so
1200 events span a realistic ~2 hour window. Only the timestamp is touched;
IPs, users, status codes, and every other field are preserved. A fixed seed
keeps regeneration reproducible.

Usage:
    flog -f apache_combined -n 1200 -t stdout | respace_apache_timestamps.py
"""

import random
import re
import sys
from datetime import datetime, timedelta, timezone

# +0200, matches the date/offset baked into the demo file.
TZ = timezone(timedelta(hours=2))
START = datetime(2025, 10, 4, 10, 0, 0, tzinfo=TZ)
MEAN_GAP_SECONDS = 6.0  # ~1200 events over ~2 hours
SEED = 20251004

TS_RE = re.compile(r"\[\d{2}/\w{3}/\d{4}:\d{2}:\d{2}:\d{2} [+-]\d{4}\]")


def main() -> int:
    random.seed(SEED)
    t = START
    for line in sys.stdin:
        line = line.rstrip("\n")
        if not line:
            continue
        stamp = t.strftime("[%d/%b/%Y:%H:%M:%S %z]")
        new_line, n = TS_RE.subn(stamp, line, count=1)
        if n == 0:
            print(f"no timestamp matched in: {line[:80]}", file=sys.stderr)
            return 1
        print(new_line)
        # Whole-second exponential gap (Apache logs have 1s resolution).
        t += timedelta(seconds=max(0, round(random.expovariate(1.0 / MEAN_GAP_SECONDS))))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except BrokenPipeError:
        # Downstream consumer (e.g. `head`) closed the pipe early.
        sys.exit(0)
