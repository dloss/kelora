"""Render `kelora --help` as Markdown for docs/reference/cli-reference.md.

Registered as an MkDocs hook: on that page it replaces the `<!-- kelora --help -->`
marker before Markdown conversion, so the option anchors are real page anchors
that cross-page links can be validated against. Needs `kelora` on PATH (the
docs recipes put the debug or release binary there). Run it directly to print
the Markdown. The CLI help is the single source of truth:
this script only re-formats it — section headers become `##`, options a
definition list with stable anchors, indented example blocks code blocks.
Flag mentions link to their entries; quoted literals and metavars become code.
"""

import re
import subprocess

SECTION = re.compile(r"^([A-Z][A-Za-z /]+):$")
OPTION_INDENTS = (2, 6)
TEXT_INDENT = 10
DD = "    "  # continuation indent inside a definition

# Inline tokens rendered as code, in priority order: existing `code` spans,
# 'quoted' literals, flags (optionally with =value or a 'quoted' argument),
# metavars like <FORMAT> or cols:<spec>.
TOKEN = re.compile(
    r"(?P<code>`[^`]+`)"
    r"|(?<![\w'])'(?P<quoted>[^'\n]+?)'(?![\w'])"
    r"|(?<![\w`-])(?P<flag>--[a-z][a-z0-9-]*\*?|-[A-Za-z])"
    r"(?P<value>=[\w,.:-]+| '[^'\n]+')?(?![\w-])"
    r"|(?P<metavar>(?:[a-z0-9-]+:)?<[A-Za-z_]+>)"
)

# Flag name (e.g. "--take", "-f") -> anchor; filled by render().
FLAGS = {}


def help_text():
    return subprocess.run(
        ["kelora", "--help"], capture_output=True, text=True, check=True
    ).stdout


def indent_of(line):
    return len(line) - len(line.lstrip(" "))


def escape_plain(text):
    return (
        text.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("*", r"\*")
        .replace("_", r"\_")
    )


def code(text):
    fence = "``" if "`" in text else "`"
    pad = " " if fence == "``" else ""
    return f"{fence}{pad}{text}{pad}{fence}"


def inline(text, current=None):
    """Escape prose and turn flags, quoted literals and metavars into code.

    Flags with an entry on this page link to it, except on their own entry.
    """
    def link(span, flag):
        target = FLAGS.get(flag) or FLAGS.get(re.sub(r"^--no-", "--", flag))
        return f"[{span}](#{target})" if target and target != current else span

    out, pos = [], 0
    for m in TOKEN.finditer(text):
        out.append(escape_plain(text[pos : m.start()]))
        pos = m.end()
        if m.group("code"):
            bare = re.fullmatch(r"`(--?[A-Za-z][a-z0-9-]*)(?:[= ][^`]*)?`", m.group("code"))
            out.append(link(m.group("code"), bare.group(1)) if bare else m.group("code"))
        elif m.group("quoted") is not None:
            out.append(code(m.group("quoted")))
        elif m.group("flag"):
            flag = m.group("flag")
            out.append(link(code(flag + (m.group("value") or "")), flag))
        else:
            out.append(code(m.group("metavar")))
    out.append(escape_plain(text[pos:]))
    return "".join(out)


def anchor(signature):
    longs = re.findall(r"--([a-z0-9-]+)", signature)
    if longs:
        return longs[0]
    short = re.match(r"-([A-Za-z])", signature)
    return f"opt-{short.group(1)}" if short else re.sub(r"\W+", "-", signature).strip("-").lower()


def quote_list(text):
    """Quote each name in 'Available formats: a, b (default), c.' so it renders as code."""

    def repl(m):
        names = re.sub(r"(?:^|(?<=, ))([^\s,()]+)", r"'\1'", m.group(2))
        return m.group(1) + names + "."

    return re.sub(r"(Available formats: )([^.]+)\.", repl, text)


def split_examples(text):
    """Split a trailing 'Examples: a, b, c' off a paragraph."""
    m = re.search(r"(?:^|(?<=\. ))Examples: (.+)$", text)
    if not m:
        return text, []
    # Split on ", " outside quotes.
    items, buf, quoted = [], "", False
    for ch in m.group(1).removesuffix("."):
        if ch == "'":
            quoted = not quoted
        buf += ch
        if not quoted and buf.endswith(", "):
            items.append(buf[:-2])
            buf = ""
    items.append(buf)
    return text[: m.start()].rstrip(), [i.strip() for i in items if i.strip()]


def render_description(lines, current):
    """Turn an option's description lines into Markdown blocks."""
    out = []
    para, block, meta = [], [], []

    def flush_para():
        if para:
            text, examples = split_examples(quote_list(" ".join(para)))
            if text:
                out.append(inline(text, current))
                out.append("")
            if examples:
                out.append("Examples:")
                out.append("")
                for item in examples:
                    # Whole commands stay as typed; bare quoted values lose quotes.
                    bare = re.fullmatch(r"'([^']*)'", item)
                    out.append("- " + code(bare.group(1) if bare else item))
                out.append("")
            para.clear()

    def flush_code():
        if block:
            # Drop the common extra indentation of the block.
            base = min(indent_of(line) for line in block if line.strip())
            while block and not block[-1].strip():
                block.pop()
            out.append("```text")
            out.extend(line[base:] if line.strip() else "" for line in block)
            out.append("```")
            out.append("")
            block.clear()

    i = 0
    while i < len(lines):
        line = lines[i]
        stripped = line.strip()
        ind = indent_of(line)
        if not stripped:
            flush_para()
            if block:
                block.append("")
            i += 1
            continue
        if stripped == "Possible values:":
            flush_para()
            flush_code()
            items = []
            i += 1
            while i < len(lines) and lines[i].strip().startswith("- "):
                name, _, desc = lines[i].strip()[2:].partition(":")
                desc = desc.strip()
                items.append(f"- {code(name)}" + (f": {inline(desc, current)}" if desc else ""))
                i += 1
            out.append("Values:")
            out.append("")
            out.extend(items)
            out.append("")
            continue
        m = re.fullmatch(r"\[(default|possible values): (.*)\]", stripped)
        if m:
            flush_para()
            flush_code()
            if m.group(1) == "default":
                meta.append(f"Default: {code(m.group(2))}")
            else:
                values = " ".join(code(v.strip()) for v in m.group(2).split(","))
                meta.append(f"Values: {values}")
            i += 1
            continue
        if ind > TEXT_INDENT:
            flush_para()
            block.append(line)
        else:
            flush_code()
            para.append(stripped)
        i += 1
    flush_para()
    flush_code()
    if meta:
        out.append(" · ".join(meta))
    while out and out[-1] == "":
        out.pop()
    return out


def parse(lines):
    """Yield ("section", name, body) and ("option", signature, desc) items."""
    i = 0
    while i < len(lines):
        line = lines[i]
        sec = SECTION.match(line)
        if sec:
            name = sec.group(1)
            body = []
            i += 1
            if name == "Exit Codes":
                while i < len(lines) and not SECTION.match(lines[i]):
                    body.append(lines[i][2:] if lines[i].startswith("  ") else lines[i])
                    i += 1
                while body and not body[-1].strip():
                    body.pop()
            yield "section", name, body
            continue
        if line.strip() and indent_of(line) in OPTION_INDENTS:
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
            yield "option", line.strip(), desc
            continue
        i += 1


def render(text):
    lines = text.splitlines()
    start = next(i for i, line in enumerate(lines) if line == "Arguments:")
    items = list(parse(lines[start:]))

    FLAGS.clear()
    for kind, signature, _ in items:
        if kind == "option":
            for flag in re.findall(r"(?<![\w-])(--[a-z0-9-]+|-[A-Za-z])\b", signature):
                FLAGS.setdefault(flag, anchor(signature))

    out = []
    for kind, name, body in items:
        if kind == "section":
            out.append(f"## {'General' if name == 'Options' else name}")
            out.append("")
            if body:
                out.append("```text")
                out.extend(body)
                out.append("```")
                out.append("")
            continue
        target = anchor(name)
        out.append(f"{code(name)} {{ #{target} }}")
        desc = render_description(body, target)
        first = True
        for line in desc:
            if first:
                out.append(f":   {line}")
                first = False
            else:
                out.append(f"{DD}{line}" if line else "")
        if first:
            out.append(":   &nbsp;")
        out.append("")
    return "\n".join(out)


MARKER = "<!-- kelora --help -->"


def on_page_markdown(markdown, page, **kwargs):  # noqa: ARG001
    if page.file.src_uri == "reference/cli-reference.md":
        if MARKER not in markdown:
            raise ValueError(f"{page.file.src_uri}: missing {MARKER} marker")
        markdown = markdown.replace(MARKER, render(help_text()))
    return markdown


if __name__ == "__main__":
    print(render(help_text()))
