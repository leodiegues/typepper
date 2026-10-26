---
weight: 7
---

# Roadmap

What's built, and what's open.

## Built

| Release | What |
|---|---|
| 0.1.0 – 0.3.0 | the game: six modes, heat and ranks, bests, the web build |
| 0.4.0 | the workspace and the deterministic engine; replays (`--replay`, `?replay=`); sharing (`u`); accounts (`typepper link`); the outbox; leaderboards (`b`) for day, week, month, year and all time; the server on Railway; the `online` setting; the version on the title screen |
| next | THE HOT ONE: a seventh mode (strikes, plates, courses, chewing; replay format 2 for the picks), and rooms: the first thing played together, over a kept-open SSH channel or a WebSocket |

## Open

| | Why | Notes |
|---|---|---|
| **Smooth deploys** | about 2 in 45 SSH connections fail at a deploy's switch | finish open sessions on SIGTERM; `overlapSeconds` / `drainingSeconds` in `railway.ts` |
| **Rooms, next** | guests are `guest-N`; a dropped connection is out, with no way back in; a deploy ends every room | a name for guests; rejoining a game by seat; rooms kept across a deploy would need a store |
| **Rooms on phones** | no ←/→ to turn the plate | the plate takes numbers already; a tap target per pepper |
| **Sharing on touch screens** | phones have no `u` | a tap target on the results screen |
| **Anonymous quota behind the TCP proxy** | keyless SSH callers may all arrive from Railway's address and share one quota | check the logged address in production; key the quota differently, or raise it |
| **A GitHub token for linking** | GitHub's unauthenticated API allows 60 requests an hour per address, shared on Railway | an optional `TYPEPPER_GITHUB_TOKEN`, sent as a bearer header, `preserve()`d in `railway.ts` |
| **Old replays after a `VERSION` bump** | replays recorded by older rules can't play back | keep old engines as renamed dependencies (`engine_v1 = { package = "typepper-engine", version = "…" }`), dispatching on the header; split `replay.rs` into its own crate then; or keep old web builds at `/e/<n>/` |
| **Past boards** | only current periods are browsable | `?period=month&at=2026-09`; the sweep already keeps each month's bests |
| **Delete my data** | players may want their runs gone | `v1 forget`: deleting the player cascades to keys and runs |
| **Staging** | worth it once a migration could hurt real data | a `ctx.environment` branch in `railway.ts`, a `staging` GitHub environment, a second deploy job |
| **Release commit message** | cargo-release titles it `chore: Release` since the workspace move | move `pre-release-commit-message` to a workspace-level `release.toml` |
