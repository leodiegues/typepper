# Contributing to typepper

Thanks for wanting to make typepper better. This covers setting up, running
every part, testing, and what a good change looks like. How things are built
is in [docs/](docs/README.md); start with
[docs/built/architecture.md](docs/built/architecture.md).

## What's here

```text
crates/typepper/          the game (terminal and web)
crates/typepper-engine/   the rules, shared by the game and the server
crates/typepper-server/   the server (HTTP + SSH over Postgres)
infra/                    Railway, Docker, the server's host key
docs/                     playing it, the protocol, how it's built
site/                     docs.typepper.com, made from docs/ (Zola)
demo/                     recording the README's GIFs
```

## Setup

You need [rustup](https://rustup.rs) and [mise](https://mise.jdx.dev):

```sh
git clone https://github.com/leodiegues/typepper && cd typepper
mise trust && mise install    # node, the railway CLI, Trunk, dist, cargo-release, pinned
cargo build                   # rustup installs the pinned Rust (rust-toolchain.toml) first
```

For the server you'll also want Docker (for a local Postgres) and an `ssh`
client.

## Running

### The game

```sh
cargo run                        # the terminal game
cargo run -- --demo              # the self-playing tour
cargo run -- --replay            # watch your last round
```

`cargo run`, `cargo build` and `cargo test` mean the game; use
`-p typepper-server`, `-p typepper-engine` or `--workspace` for the rest.

### The web build

```sh
trunk serve --port 8090          # http://127.0.0.1:8090
```

Append `?demo` for the tour, `?replay=<id>` to watch a shared round, and
`?api=http://localhost:8080` to talk to a local server.

### The server

```sh
mise run server
```

This starts Postgres 18 in Docker (`typepper-postgres`, port 55432), makes a
throwaway host key, and runs the server on `localhost:8080` (HTTP) and
`localhost:2222` (SSH). It prints how to point a game at it:

```sh
curl -i localhost:8080/health
ssh -p 2222 -l typepper localhost ping
TYPEPPER_SSH=localhost:2222 TYPEPPER_SSH_KNOWN_HOST="$(cat ${TMPDIR:-/tmp}/typepper-host-key.pub)" cargo run
```

Linking against a local server needs a stand-in for GitHub: serve a folder
with `<login>.keys` (your public key) and `users/<login>` (`{"id": 1,
"login": "<login>"}`) with `python3 -m http.server 8765`, and start the server
with `TYPEPPER_GITHUB=http://127.0.0.1:8765`.

Web login against a local server goes through the real GitHub App:

1. Once, on the app (github.com → Settings → Developer settings → GitHub
   Apps → typepper → General): **Add callback URL**
   `http://localhost:8080/auth/callback`, under the production one (the
   first stays the default).
2. Put the app's id and secret in `mise.local.toml` (gitignored):

   ```toml
   [env]
   TYPEPPER_GITHUB_CLIENT_ID = "Iv23li…"
   TYPEPPER_GITHUB_CLIENT_SECRET = "…"
   ```

3. `mise run server`, `trunk serve --port 8090`, and open
   `http://localhost:8090/?api=http://localhost:8080`: **localhost** both,
   not 127.0.0.1, or the session cookie counts as another site's and isn't
   sent. Use Chrome or Firefox: Safari may drop a `Secure` cookie over http.

`TYPEPPER_WEB` and `TYPEPPER_GITHUB_CALLBACK` (set by the task) send GitHub
and the player back to these local addresses.

### The docs site

```sh
mise run docs
```

This serves docs.typepper.com from `docs/` at `http://127.0.0.1:1111`, and
reloads as you edit. `site/prepare.py` copies `docs/` into Zola's
`site/content/`, turning links between docs into ones Zola checks: a broken
link or `#anchor` fails the build. Write links the way GitHub reads them
(`../reference/protocol.md#boards`); a page needs front matter (its
`weight` orders it in its section), and its first `# heading` is its title.

## Tests

```sh
cargo test --workspace
```

The server's database tests run only when a Postgres is given (they skip with
a note otherwise). CI always runs them:

```sh
docker run -d --rm --name tp-test -e POSTGRES_PASSWORD=pw -p 55433:5432 postgres:18-alpine
TYPEPPER_TEST_DATABASE_URL=postgres://postgres:pw@localhost:55433/postgres cargo test --workspace
```

What's covered is in each part's doc ([engine](docs/built/engine.md#the-tests-that-hold-it),
[replays](docs/reference/replays.md#decoding), [server](docs/built/server.md#tests)). Leave
one runnable check behind for logic that can break: a test that fails if the
logic does. Small tests, close to the code, no frameworks.

## Before you open a pull request

The same checks CI runs:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p typepper --target wasm32-unknown-unknown --locked -- -D warnings
cargo test --workspace --locked
.github/docs-check.sh origin/main   # docs whose code you changed
```

If you touched the web build, run `trunk build --release` too.

## Rules that keep things working

- **Changing the rules?** Anything that would change a round's result (balance,
  the word list, how the rng is used, the timestep) needs `engine::VERSION`
  bumped and the golden numbers re-recorded. The `same_numbers_on_every_platform`
  test tells you when. See [engine.md](docs/built/engine.md#version).
- **Migrations only add**: tables, nullable or defaulted columns, indexes.
  Never edit an applied migration; drops and renames wait for a later
  release. See [server.md](docs/built/server.md#database).
- **Protocol commands don't change meaning.** A different shape is a new
  version (`v2`) next to the old one, since released games keep calling what
  they were built with. See [protocol.md](docs/reference/protocol.md#versioning).
- **Never break offline.** No network calls at startup or during play, nothing
  sent without the player asking, no prompt over the game.
  See [architecture.md](docs/built/architecture.md#principles).
- **Mode keys are forever** (`time:30`, `pursuit:7`): replays and boards are
  stored under them.
- **Pins stay pins.** Actions by SHA, images by digest, tools in
  `mise.lock`. Dependabot bumps most; see
  [operations.md](docs/built/operations.md#pinned-by-hash).
- **Don't edit `.github/workflows/release.yml`**: `dist` generates it.

## Style

- `cargo fmt`, and clippy with warnings as errors.
- Comments say **why**, in plain lowercase sentences; the code says what.
  Doc comments on public items say what they're for.
- The simplest thing that works. A deliberate shortcut with a known ceiling
  gets a `ponytail:` comment naming it and the way up
  (`// ponytail: a global lock; per-account locks if throughput matters`).
- No new dependency for what a few lines can do.

## Commits and pull requests

- **Commit subjects** say what changed for the code's reader, as a short
  sentence, often with the area first: `Sharing: a finished round becomes a
  link anyone can watch`, `--flawless: the bot never misses`. The body
  explains why, wrapped at about 72 columns.
- **One concern per pull request**, with a description of what changed, why,
  and how you checked it (what you ran, what you saw).
- **Docs move with the code.** Each page in `docs/` lists the code it covers
  (`covers` in its front matter); change that code and CI asks for the page
  too. If nothing it says became wrong, put a `docs: none` line in the
  description. The docs-sync skill finds and proposes the edits.
- **Player-visible changes** get a bullet under `## Unreleased` in
  `CHANGELOG.md`, written for players: what they'll notice and why it's nice.
  Refactors, CI and internals stay out.

## Releases

Maintainers release with `cargo release` (configured in
`crates/typepper/release.toml`). A `vX.Y.Z` tag publishes the binaries and
installers, deploys typepper.com, and deploys the server, together. The
steps, checks and verification are in
[.claude/skills/release/SKILL.md](.claude/skills/release/SKILL.md);
infrastructure is in [docs/built/operations.md](docs/built/operations.md).

## License

typepper is GPL-3.0-or-later. By contributing you agree your contribution is
licensed the same way.
