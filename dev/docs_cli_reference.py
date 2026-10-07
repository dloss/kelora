"""Render `kelora --help` as Markdown for docs/reference/cli-reference.md.

Run from the repository root with `kelora` on PATH (the docs build does this
through a markdown-exec block). The CLI help is the single source of truth:
this script only re-formats it — section headers become `##`, each option a
`###` with a stable anchor, indented example blocks become code blocks.
"""

import re
import subprocess

SECTION = re.compile(r"^([A-Z][A-Za-z /]+):$")
OPTION_INDENTS = (2, 6)
TEXT_INDENT = 10


def help_text():
    return subprocess.run(
        ["kelora", "--help"], capture_output=True, text=True, check=True
    ).stdout


def indent_of(line):
    return len(line) - len(line.lstrip(" "))


def escape(text):
    """Escape Markdown/HTML outside `code` spans."""
    parts = re.split(r"(`[^`]*`)", text)
    out = []
    for part in parts:
        if part.startswith("`") and part.endswith("`") and len(part) > 1:
            out.append(part)
        else:
            out.append(
                part.replace("&", "&amp;")
                .replace("<", "&lt;")
                .replace(">", "&gt;")
                .replace("*", r"\*")
                .replace("_", r"\_")
            )
    return "".join(out)


def anchor(signature):
    longs = re.findall(r"--([a-z0-9-]+)", signature)
    if longs:
        return longs[0]
    short = re.match(r"-([A-Za-z])", signature)
    return f"opt-{short.group(1)}" if short else re.sub(r"\W+", "-", signature).strip("-").lower()


def render_description(lines):
    """Turn an option's description lines into Markdown blocks."""
    out = []
    para, code = [], []

    def flush_para():
        if para:
            out.append(escape(" ".join(para)))
            out.append("")
            para.clear()

    def flush_code():
        if code:
            # Drop the common extra indentation of the block.
            base = min(indent_of(line) for line in code if line.strip())
            out.append("```text")
            out.extend(line[base:] if line.strip() else "" for line in code)
            out.append("```")
            out.append("")
            code.clear()

    i = 0
    while i < len(lines):
        line = lines[i]
        stripped = line.strip()
        ind = indent_of(line)
        if not stripped:
            flush_para()
            if code:
                code.append("")
            i += 1
            continue
        if stripped == "Possible values:":
            flush_para()
            flush_code()
            items = []
            i += 1
            while i < len(lines) and lines[i].strip().startswith("- "):
                items.append("- " + escape(lines[i].strip()[2:]))
                i += 1
            out.append("Possible values:")
            out.append("")
            out.extend(items)
            out.append("")
            continue
        m = re.fullmatch(r"\[(default|possible values): (.*)\]", stripped)
        if m:
            flush_para()
            flush_code()
            label = "Default" if m.group(1) == "default" else "Values"
            out.append(f"*{label}:* `{m.group(2)}`")
            out.append("")
            i += 1
            continue
        if ind > TEXT_INDENT:
            flush_para()
            code.append(line)
        else:
            flush_code()
            para.append(stripped)
        i += 1
    flush_para()
    flush_code()
    # Trim trailing blank lines inside code blocks were handled; trim end.
    while out and out[-1] == "":
        out.pop()
    return out


def render(text):
    lines = text.splitlines()
    start = next(i for i, line in enumerate(lines) if line == "Arguments:")
    lines = lines[start:]

    out = []
    i = 0
    while i < len(lines):
        line = lines[i]
        sec = SECTION.match(line)
        if sec:
            name = sec.group(1)
            title = "General" if name == "Options" else name
            out.append(f"## {title}")
            out.append("")
            if name == "Exit Codes":
                body = []
                i += 1
                while i < len(lines) and not SECTION.match(lines[i]):
                    body.append(lines[i][2:] if lines[i].startswith("  ") else lines[i])
                    i += 1
                while body and not body[-1].strip():
                    body.pop()
                out.append("```text")
                out.extend(body)
                out.append("```")
                out.append("")
                continue
            i += 1
            continue
        if line.strip() and indent_of(line) in OPTION_INDENTS:
            signature = line.strip()
            out.append(f"### `{signature}` {{ #{anchor(signature)} }}")
            out.append("")
            desc = []
            i += 1
            while i < len(lines):
                nxt = lines[i]
                if SECTION.match(nxt):
                    break
                if nxt.strip() and indent_of(nxt) in OPTION_INDENTS:
                    break
                desc.append(nxt)
                i += 1
            out.extend(render_description(desc))
            out.append("")
            continue
        i += 1
    return "\n".join(out)


if __name__ == "__main__":
    print(render(help_text()))
