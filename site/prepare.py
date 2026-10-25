"""docs/ → site/content/, for Zola; the files in docs/ stay as GitHub reads them.

- Links between docs (`../reference/protocol.md#boards`) become Zola's
  `@/reference/protocol.md#boards`, so `zola build` fails on a broken one.
- Links out of docs/ (`../../CONTRIBUTING.md`, code) go to GitHub.
- docs/README.md is the home page (`_index.md`).
- A page without a `title` gets its first `# heading`.
- The font, wordmarks and favicon come from assets/ (site/static/assets/).

Run from anywhere: `python3 site/prepare.py`. Then `zola --root site build`.
"""

import filecmp
import json
import re
import shutil
import tomllib
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parent.parent
SITE = ROOT / "site"
REPO = tomllib.loads((SITE / "config.toml").read_text())["extra"]["repo"]
ASSETS = ["fonts/DepartureMono-Regular.woff2", "logo/wordmark-dark.svg", "logo/wordmark-light.svg", "logo/favicon.ico"]
LINK = re.compile(r"\]\(([^)\s]+)\)")


def target(link: str, page: PurePosixPath) -> str:
    """Where a link in `page` (a path under the repo root) should point on the site."""
    if re.match(r"[a-z]+:|#|@/", link):
        return link
    path, _, anchor = link.partition("#")
    anchor = "#" + anchor if anchor else ""
    to = PurePosixPath(*_resolve(page.parent / path))
    if to.parts[0] != "docs":
        return f"{REPO}/blob/main/{to}{anchor}"
    inside = PurePosixPath(*to.parts[1:])
    if inside.suffix != ".md":
        return f"{REPO}/blob/main/{to}{anchor}"
    if inside.name == "README.md":
        inside = inside.with_name("_index.md")
    return f"@/{inside}{anchor}"


def _resolve(p: PurePosixPath) -> list[str]:
    out = []
    for part in p.parts:
        if part == "..":
            out.pop()
        elif part != ".":
            out.append(part)
    return out


def convert(text: str, page: PurePosixPath) -> str:
    if not text.startswith("---\n"):
        text = "---\n---\n" + text
    _, front, body = text.split("---\n", 2)
    if not re.search(r"^title:", front, re.M):
        heading = re.search(r"^# (.+)$", body, re.M)
        front = f"title: {json.dumps(heading.group(1) if heading else page.stem)}\n" + front
    lines, fenced = [], False
    for line in body.splitlines(keepends=True):
        if line.lstrip().startswith("```"):
            fenced = not fenced
        lines.append(line if fenced else LINK.sub(lambda m: f"]({target(m.group(1), page)})", line))
    return f"---\n{front}---\n{''.join(lines)}"


def main() -> None:
    # only what changed is written, so `zola serve` reloads just that page
    content = SITE / "content"
    want = {}
    for src in sorted((ROOT / "docs").rglob("*.md")):
        rel = src.relative_to(ROOT / "docs")
        out = content / (rel.with_name("_index.md") if rel.name == "README.md" else rel)
        want[out] = convert(src.read_text(), PurePosixPath(src.relative_to(ROOT).as_posix()))
    for old in content.rglob("*.md"):
        if old not in want:
            old.unlink()
    for out, text in want.items():
        if not out.exists() or out.read_text() != text:
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_text(text)
    static = SITE / "static" / "assets"
    static.mkdir(parents=True, exist_ok=True)
    for a in ASSETS:
        src, out = ROOT / "assets" / a, static / Path(a).name
        if not out.exists() or not filecmp.cmp(src, out, shallow=False):
            shutil.copy(src, out)


if __name__ == "__main__":
    main()
