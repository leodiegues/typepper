---
name: release
description: Cut a typepper release end to end — check main is releasable, pick the version from what changed, write player-facing changelog notes, sync the docs, tag and push with cargo-release, then watch the release and site deploy and verify typepper.com and the installer. Use whenever the user wants to release, ship, publish, tag, bump the version, or "put out" a new typepper version, even if they don't say "release".
---

# Releasing typepper

A `vX.Y.Z` tag drives everything. Pushing it starts `release.yml` (cargo-dist:
binaries for 5 platforms plus the shell/PowerShell installers, published as a
GitHub Release whose notes come from that version's `CHANGELOG.md` section)
`pages.yml` (deploys the web game to https://typepper.com) and
`deploy-production.yml` (deploys typepper-server to Railway, behind
api.typepper.com and ssh.typepper.com). `cargo release`
makes the tag (config in `crates/typepper/release.toml`). Your part is the judgment around it,
the version and the notes, and confirming it all landed.

Once a tag is pushed, installers and the website update for everyone, so get
the user's OK on the version and notes before step 4.

## 1. Preflight

Releasing from a broken or unpushed `main` ships that state to every player,
so stop and explain if any of this fails:

```sh
git switch main && git pull --ff-only
git status --short                                   # must be empty
git describe --tags --abbrev=0                       # the last release
gh run list --workflow ci.yml --branch main --limit 1 --json headSha,conclusion
```

CI's latest run must be `success` for the current `HEAD`. If CI hasn't run
on `HEAD`, run its checks locally instead: `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`,
`cargo clippy --target wasm32-unknown-unknown -- -D warnings`, `cargo test`.

## 2. Pick the version

Read `git log --oneline <last-tag>..HEAD`, and the diffs where a message is
unclear. typepper is pre-1.0:

- **patch**: fixes, polish, tuning, site-only changes
- **minor**: anything a player would call new (a mode, setting, effect,
  platform), or a change to how bests/settings are saved
- **major**: only when the user decides it's 1.0

If nothing player-visible changed, say so and suggest not releasing. A
site-only fix can go out with a manual run of the pages workflow instead
(`gh workflow run pages.yml --ref main`). The server only deploys from tags too: to
redeploy one (say, after an infra change), run
`gh workflow run deploy-production.yml --ref vX.Y.Z`.

## 3. Write the notes

Edit `CHANGELOG.md` under `## Unreleased`. Keep that heading: cargo-release
renames it to `## [X.Y.Z] - <date>` and opens a fresh one. Fold in any
bullets already there.

Write for players, not developers: what they'll notice and why it's nice, in
short plain bullets. Leave out refactors, CI and internals unless they change
something a player sees (a fix for a crash counts; "moved code into store.rs"
doesn't). Look at the `0.1.0` section for the tone.

Then run the docs-sync skill from the last tag, so the docs describe what
ships. It only reads pages no pull request since the tag touched (those that
said `docs: none`, or went in without one): usually a few, usually with
nothing to change.

Show the user the proposed version, notes and doc edits, and wait for their
OK. Then commit them (cargo-release refuses a dirty tree; `git add` picks up
a new doc page, which `-a` wouldn't):

```sh
git add CHANGELOG.md docs/
git commit -m "Changelog for vX.Y.Z"
```

## 4. Release

Dry-run first and read it: the version bump, the changelog heading becoming
`## [X.Y.Z] - <date>`, the tag `vX.Y.Z`.

```sh
cargo release <patch|minor|major>
cargo release <patch|minor|major> --execute --no-confirm
```

This bumps the shared version in the root `Cargo.toml` (every crate inherits it) and `Cargo.lock`, commits `Release vX.Y.Z`, tags it,
and pushes `main` and the tag.

If it fails before pushing, everything is still local: fix it and retry (ask
before deleting a local tag or resetting). If the tag was already pushed,
don't move or reuse it. Installers may have picked it up. Fix forward with
the next patch.

## 5. Watch

```sh
gh run list --limit 5 --json databaseId,name,headBranch,status
gh run watch <id> --exit-status
```

Expect `release` (about 10 minutes, mostly the macOS and Windows builds),
`pages` and `deploy-production` for the tag, and `ci` for `main`. The deploy
is detached: green means Railway took the commit, so also check that
`curl -s https://api.typepper.com/health` reports the new `releaseId`. On a failure, read
`gh run view <id> --log-failed` and report what broke. Don't retag.

## 6. Verify

```sh
gh release view vX.Y.Z --json assets --jq '.assets[].name'
curl -sSfL https://typepper.com/ | grep -o '<title>[^<]*'
curl -sSfL https://typepper.com/install.sh | tail -2
curl -sSIL -o /dev/null -w '%{http_code}\n' \
  https://github.com/leodiegues/typepper/releases/latest/download/typepper-installer.sh
```

Expect 5 platform archives (macOS ×2, Linux ×2, Windows), a
`typepper-<target>-update` updater for each (what `typepper upgrade` runs),
both installers and checksums, the site's title, the installer hand-off script, and a `200`.
Don't run the installer itself without asking: it installs into the user's
`~/.cargo/bin`.

## 7. Report

Keep it short: the version and why that level, links to the GitHub release
and https://typepper.com, the run results, and anything that needs the
user's attention.
