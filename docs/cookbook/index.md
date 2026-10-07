# Cookbook

Short answers to common questions, each a command you can adapt. Every
command runs against a file in
[`examples/`](https://github.com/dloss/kelora/tree/main/examples); replace it
with your own log and adjust the field names (`kelora yourfile -d` lists
them). Each recipe links to the guide page that explains the options.

```python exec="on" idprefix=""
import pathlib, re

pages = [
    ("errors.md", "Errors and incidents"),
    ("web.md", "Web and API traffic"),
    ("security-privacy.md", "Security and privacy"),
    ("parsing.md", "Parsing"),
    ("exports.md", "Exports and samples"),
    ("monitoring.md", "Monitoring and time"),
]
for name, title in pages:
    url = name.removesuffix(".md") + "/"  # rendered output bypasses MkDocs link rewriting
    print(f"\n### [{title}]({url})\n")
    for line in pathlib.Path("docs/cookbook", name).read_text().splitlines():
        if line.startswith("## "):
            heading = line[3:]
            slug = re.sub(r"\s+", "-", re.sub(r"[^\w\s-]", "", heading.lower()).strip())
            print(f"- [{heading}]({url}#{slug})")
```
