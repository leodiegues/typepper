---
name: docs-sync
description: Bring typepper's docs/ up to date with the code that changed — find the pages whose covered code moved, read only those diffs, and propose small edits to the claims that are now wrong. Use before opening a pull request, when docs.yml fails on one, from the release skill before tagging, or whenever the user asks to update, sync, or check the docs.
---

# Syncing the docs

Each page in `docs/` names the code it covers in its front matter
(`extra.covers`, git globs). `.github/docs-check.sh` lists the pages whose
covered code changed but which weren't touched; CI runs it on every pull
request (`docs.yml`). This skill does the part a script can't: deciding what
the change made false, and fixing just that.

Keep it cheap. The docs are long and the user pays for every page read, so
read only what the check points at, and edit, never rewrite.

## 1. What changed

The base is the last tag when called from the release skill
(`git describe --tags --abbrev=0`), else `origin/main`.

```sh
.github/docs-check.sh <base>
```

Nothing listed: say the docs are current and stop. Don't read any pages.

## 2. What it made wrong

For each listed page, read the page and only its covered diff:

```sh
git diff $(git merge-base <base> HEAD) -- <the page's covers globs>
```

Find the claims the diff made false: names, numbers, formats, flags, error
codes, limits, steps, diagrams. A change that leaves every claim true needs no
edit (a refactor, a fix that restores documented behavior).

Read a page the check didn't list only when the diff adds something with no
home yet (a command, endpoint, mode, file, workflow). Then propose where it
goes: a few lines on the closest page, and a `covers` entry if new code
should be watched.

## 3. Propose, then edit

Show each proposed edit as a short diff, with the line of code that makes it
true. Match the page's voice: short plain sentences, tables where the page
already has them. Don't restyle, reorder, or fix wording nobody asked about.

Wait for the user's OK, then edit. Run `cargo test -p typepper-server --test
docs` and `.github/docs-check.sh <base>` again.

If nothing needed changing on a pull request, say so: its description gets a
`docs: none` line so `docs.yml` passes.
