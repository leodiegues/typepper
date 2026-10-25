---
weight: 1
extra:
  covers:
    - crates/typepper-server/src/http.rs
    - crates/typepper-server/src/ssh.rs
    - crates/typepper-server/src/runs.rs
    - crates/typepper-server/src/board.rs
    - crates/typepper/src/online.rs
---

# Protocol

What the game and the server say to each other. The terminal game talks over
**SSH** (it runs the system `ssh`); the web game over **HTTPS**. Both carry
the same requests and the same one-line answers, so the game parses them the
same way whichever way they came.

## Versioning

| Layer | Version | Changes when |
|---|---|---|
| commands | `v1` (SSH), `/v1` (HTTP) | a request or an answer changes shape |
| replay bytes | `format` | the byte layout changes ([replays](replays.md)) |
| rules | `engine` | results would change ([engine](../built/engine.md)) |

Released games keep calling what they were built with, so a `v1` command
never changes meaning: a different shape gets `v2`, next to it. `/health`,
share pages and `ping` are unversioned: they're for Railway, for people, and
for checking the door is open.

## Commands

| SSH (`ssh -p 36080 -l typepper ssh.typepper.com …`) | HTTP (`https://api.typepper.com`) | Does |
|---|---|---|
| `v1 submit`, submission on stdin | `POST /v1/runs`, submission as the body | plays and stores each replay; ranks it if the caller is a linked player |
| | `POST /v1/me/runs`, with the session cookie | the same, as the logged-in player; no session: `err signed-out` (401) |
| | `GET /v1/me/bests`, with the session cookie | the logged-in player's best in each mode, `<mode> <score>` lines; no session: `err signed-out` (401) |
| `v1 board <mode> <period>` | `GET /v1/boards/{mode}?period=<period>` | a leaderboard |
| `v1 whoami` | | `ok <login>`, or `ok anonymous` |
| `ssh -l <github-login> … v1 link` | | links the key that logged in to that GitHub account |
| `v1 unlink` | | lets go of the key that logged in |
| | `GET /auth/github` | web login (see [browsers](#browsers)) |
| | `POST /auth/logout` | ends the browser's session |
| `v1 replay <id>` | `GET /v1/runs/{id}` | a stored replay (hex over SSH, bytes over HTTP) |
| | `GET https://r.typepper.com/{id}` | a run's share page |
| `ping` | `GET /health` | `pong`; readiness |

The SSH user matters: **`typepper`** is the game, and any other user is a
GitHub login asking to link (see [server](../built/server.md#accounts-and-linking)).
Anything else over SSH answers `err unknown-command <it>` with exit status 1;
a plain `ssh` without a command prints that there's no shell and exits.

## Submitting

### The submission

```text
submission = item × 1..20                       back to back: the outbox in one go
item       = claimed: f64 LE (8 bytes) | replay
```

- No length prefix: a replay ends where its `count` inputs end.
- `claimed` is the client's own score (`NaN` for none). It's stored next to
  the server's and logged when they differ; it's never believed.
- At most 20 items, each replay at most 1 MiB.

```sh
# one replay, by hand (bash): NaN as the claim, then the bytes
{ printf '\x00\x00\x00\x00\x00\x00\xf8\x7f'; cat run.replay; } \
  | ssh -p 36080 -l typepper ssh.typepper.com v1 submit
curl --data-binary @submission.bin https://api.typepper.com/v1/runs
```

### The answer

One line per item, in order:

```text
ok <id> <score> <rank> <link>
err <code> <why>
```

```text
ok 7Xf9qyuC 108 - https://r.typepper.com/7Xf9qyuC         anonymous: no rank
ok nEGsHePP 0 3 https://r.typepper.com/nEGsHePP           a linked player: #3 all time on that mode
err old-engine update typepper to submit
err bad-replay truncated
```

- **`<score>`** is the server's (`-` in zen). **`<rank>`** is the player's
  place on the mode's all-time board after this run (`-` when anonymous or
  zen).
- A broken item ends the answer: with its bytes unreadable there's no telling
  where the next one starts.

| Code | Means | The game's outbox |
|---|---|---|
| `old-engine` | recorded by other rules than the server's | drops it |
| `bad-replay` | unreadable, a mode the game doesn't offer, or a round that never ends | drops it |
| `too-big` | a replay over 1 MiB, or more than a submission over SSH | drops it |
| `too-many` | past the 20th item | keeps it |
| `busy` | the server couldn't store it right now | keeps it |
| `signed-out` | `/v1/me/runs` or `/v1/me/bests` without a live session (logged out, expired, cleared); the whole submission | keeps it, and forgets the login |
| `rate-limited` | over the caller's hourly allowance; the whole submission | keeps it |
| no answer | the network, a timeout, the server down | keeps everything |

Over HTTP, `rate-limited` comes with status 429 and `signed-out` with 401;
everything else is 200, with the outcome in the lines.

## Boards

```text
v1 board time:30 week                      GET /v1/boards/time:30?period=week
```

- **`<mode>`**: a `Mode::key` (`time:30`, `pursuit:7`, …).
- **`<period>`**: `day`, `week`, `month`, `year` or `all`. Periods are UTC
  calendar spans; weeks start on Monday.
- **The answer**: the top 20, one line each, then, for a linked caller (SSH)
  or a logged-in browser (HTTP) who's below the top, their own row:

```text
1 131 leodiegues 2026-10-23 7Xf9qyuC
2 118 y0sif 2026-10-21 nEGsHePP
3 108 OctoTest 2026-10-24 Qm3kZ0aB
me 31 61 someone 2026-10-20 a1B2c3D4
```

`<rank> <score> <login> <day> <run>`; ties share a rank and go to whoever got
there first. `<run>` is the id of the row's run, to watch it; it came last,
so games that read four words still read the rest. Every row's run is kept
(see [the sweep](../built/server.md#the-sweep)). An empty board is no lines.
A bad mode or period: `err bad-board …` (404 over HTTP). HTTP boards may be
30 s old (cached `private` when they carry a `me` row).

## Linking

```text
ssh -p 36080 -l <github-login> ssh.typepper.com v1 link
ok linked <Login>                  (as GitHub spells it)
```

The SSH login only gets in with a key listed on
`github.com/<github-login>.keys`; otherwise ssh reports `Permission denied`.
`v1 link` as `typepper` answers `err not-linking …`.
`typepper link <login>` runs this for you. After linking, `v1 whoami` as
`typepper` with that key answers `ok <login>`, and its submissions are that
player's. `v1 unlink` answers `ok unlinked` (or `ok not-linked`).

## Replays and share pages

- **`v1 replay <id>`**: `ok <hex>`, the replay as submitted, as hex (the
  game reads answers as text). `err gone …` for unknown or expired ids.
- **`GET /v1/runs/{id}`**: `application/octet-stream`, the replay as
  submitted; cacheable for a week (`immutable`: a run never changes). 404 for
  unknown or expired ids.
- **`GET r.typepper.com/{id}`**: HTML with link-preview tags (`og:title`
  like `108 wpm · BLITZ 15s · typepper`), then a refresh to
  `https://typepper.com/?replay={id}`. Gone runs get a short page that says so.

Ids are 8 base62 characters (`[0-9A-Za-z]`).

## Health

`GET /health` is readiness: **200** once Postgres answers, **503** when it
doesn't (within 3 s). Railway only switches traffic to a new deploy once it's
200. The body follows the IETF health check draft (`application/health+json`),
so its field names are the draft's camelCase:

```json
{
  "status": "pass",
  "version": "1",
  "releaseId": "0.4.0",
  "serviceId": "typepper-server",
  "description": "deployment c5856d37",
  "checks": {
    "postgres:responseTime": [
      { "componentType": "datastore", "status": "pass", "observedValue": 5, "observedUnit": "ms" }
    ]
  }
}
```

`version` is the API's, `releaseId` the server's (the release). Failures
never say why; that goes to the logs. JSON we design ourselves will be
snake_case.

## Browsers

The web game calls `api.typepper.com` from `typepper.com`. CORS allows
`https://typepper.com` and local `http://localhost:*` / `http://127.0.0.1:*`
origins (for `trunk serve`), for `GET` and `POST` with a `content-type`
header, with credentials.

Web login goes through typepper's GitHub App:

1. `GET /auth/github` sets a `typepper_state` cookie (10 minutes) and sends
   the browser to GitHub.
2. GitHub sends it back to `GET /auth/callback?code=…&state=…`. A `state`
   that isn't the cookie's is refused (400). The server trades the code for
   a token, asks `GET /user` who it is, and drops the token.
3. The browser gets a `typepper_session` cookie (HttpOnly, Secure,
   SameSite=Lax, host-only, 400 days) and goes back to
   `https://typepper.com/?login=<login>`.

With that cookie, `/v1/me/runs` submits as the player, `/v1/me/bests` gives
their bests, and boards add their `me` row. `POST /auth/logout` ends it. Turned down at GitHub, the callback
just goes back to the game.

## Limits

- 20 replays a submission, 1 MiB each; a mode key of at most 32 bytes.
- **60 submissions an hour per caller**: an ssh key's fingerprint, a
  logged-in browser's player, else the connection's address (HTTP: the first
  `x-forwarded-for` address).
- An SSH session idle for 30 s is closed; the game gives up on a call after
  20 s.
