---
weight: 6
---

# Decisions

Why typepper's online side is built the way it is. Each entry: what was
decided, what it beat, and what it costs. Newest last.

## 1. Online is opt-in; offline is the product

The game makes no network call until the player opts in: sharing, the
boards, `typepper link`, or logging in on the web. After that, a call the
player didn't ask for (a logged-in browser fetching its bests) never stands
between them and a round: the game starts without waiting on it, and its
failure changes nothing. Calls the player makes fail as one dim line, and
`online: off` removes them all. **Instead of** accounts at startup,
telemetry, or update checks. **Cost:** features have to be designed around
an action the player took.

## 2. Replays are inputs, not video

A run is its seed plus each input at its millisecond, about 2.5 bytes a key;
playing it means running the engine again, live, in the real game. **Instead
of** recording keys and rendering video on a queue into a bucket: no workers,
no ffmpeg, no storage bill, and the replay looks exactly like the game,
because it is the game. **Cost:** the engine must be deterministic forever
(decision 3), and a replay needs the engine version that recorded it.

## 3. A deterministic engine, in its own crate

The rules moved out of the game into `typepper-engine`, with a fixed 120 Hz
timestep, millisecond inputs, SplitMix64 written out, and `libm` for
transcendental maths. The same code runs in the game, the browser and the
server. **Cost:** every rules change is a `VERSION` bump with golden numbers
to re-record; frame-time effects can't touch the rules.

## 4. The server scores every run itself

Uploads carry inputs; the server simulates them and keeps its own score.
The client's number is only logged when it differs. **Instead of** trusting a
posted score, which would put any number on the boards with one `curl`.
**Cost:** CPU per submission (on a blocking thread), and replays from other
engine versions can't be scored (`old-engine`).

## 5. A custom binary format, stored as `bytea`

Format 1 is a small fixed header plus LEB128 varints, versioned by its
`format` byte and readable without the engine. Each replay is one `bytea` in
its run's row. **Instead of** a row per keystroke: that would be about 30
times the storage, millions of rows to sweep, and the format defined twice
(bytes on the wire, rows in the database). **Cost:** no SQL over individual
keystrokes; statistics would be computed from the bytes when needed.

## 6. SSH for the terminal, with GitHub keys as identity

The terminal game runs the system `ssh`; a player is a GitHub account, proven
by a key on `github.com/<login>.keys`. **Instead of** an HTTP client in the
game plus OAuth device flow: no tokens on disk, no new dependency, and keys
players already have. The SSH user carries intent (`typepper` plays, a login
links), and keys are judged before signing, so a wrong key never prompts.
**Cost:** SSH needs a TCP proxy and a fixed port (decision 9), and agents
like 1Password may ask to approve each linked upload.

## 7. Anonymous sharing without a key

The server lets the game in through keyboard-interactive with nothing asked,
and the game offers no key while anonymous. **Instead of** requiring a key or
an account to share. **Cost:** without `BatchMode`, the game has to rule out
every possible prompt itself (host key checked, no passwords, askpass
nowhere). Anonymous callers share a quota by address.

## 8. Railway, declared in the repo, deployed by CI on tags

One service and Postgres on Railway; `railway.ts` describes them, CI applies
it and uploads the tagged commit, and Railway builds the image. **Instead of**
building and pushing images to a registry, or deploying on every push. The
server always runs the release the game shipped with. **Cost:** IaC can't
register custom domains or carry dashboard changes, so those must be mirrored
in `railway.ts` by hand.

## 9. The TCP proxy's port is part of the game

Railway assigns the SSH port (36080), and the game ships it. **Instead of**
port 22, which Railway can't give. **Cost:** the proxy must never be
recreated; `TYPEPPER_SSH` is the escape hatch.

## 10. Random `bigint` ids, written in base62

Every id is random below 62⁸, shown as 8 characters. **Instead of** UUIDs (36
characters, or 22 encoded; twice the index size) or sequential ids (links
could be counted through). **Cost:** a retry on the rare collision, handled
by `with_new_id`.

## 11. Boards from a sweep-friendly schema

Boards are queries over `runs`: each player's best per mode, all time or
since a UTC calendar period began. An hourly sweep keeps the last 8 days and
each month's best per player and mode. **Instead of** separate board tables
kept in sync. **Cost:** only current periods are browsable (past months'
bests are kept, so they can come later).

## 12. Health in the IETF format

`/health` gates Railway's traffic switch with its status code and describes
itself in `application/health+json`, with that draft's camelCase names.
**Instead of** an always-200 status page (a broken deploy would take
traffic) or our own JSON. **Cost:** camelCase in one place; our own JSON will
be snake_case.

## 13. One version for everything

Every crate inherits the workspace version; a release tag ships the game, the
site and the server. **Instead of** versioning the server on its own.
**Cost:** a release redeploys the server even when it didn't change.

## 14. No database crate, no repository layer

The server is the only program that touches Postgres, and each core module
owns its SQL. **Instead of** a `typepper-database` crate or traits over
storage. When a second program needs the database, the server becomes a
library with a second binary first.
