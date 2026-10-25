# Roadmap

What's built, and what's open.

## Built

| Release | What |
|---|---|
| 0.1.0 – 0.3.0 | the game: six modes, heat and ranks, bests, the web build |
| 0.4.0 | the workspace and the deterministic engine; replays (`--replay`, `?replay=`); sharing (`u`); accounts (`typepper link`); the outbox; leaderboards (`b`) for day, week, month, year and all time; the server on Railway; the `online` setting; the version on the title screen |

## Open

| | Why | Notes |
|---|---|---|
| **Web login** | web players can watch boards but not be on them | a GitHub OAuth app; `/auth/github` → `/auth/callback`; a `sessions` table (a 256-bit cookie secret, only its hash stored, `Domain=typepper.com`, `SameSite=Lax`); CORS with credentials; the outbox on the web |
| **Smooth deploys** | about 2 in 45 SSH connections fail at a deploy's switch | finish open sessions on SIGTERM; `overlapSeconds` / `drainingSeconds` in `railway.ts` |
| **Sharing on touch screens** | phones have no `u` | a tap target on the results screen |
| **Anonymous quota behind the TCP proxy** | keyless SSH callers may all arrive from Railway's address and share one quota | check the logged address in production; key the quota differently, or raise it |
| **A GitHub token for linking** | GitHub's unauthenticated API allows 60 requests an hour per address, shared on Railway | an optional `TYPEPPER_GITHUB_TOKEN`, sent as a bearer header, `preserve()`d in `railway.ts` |
| **Old replays after a `VERSION` bump** | replays recorded by older rules can't play back | keep old engines as renamed dependencies (`engine_v1 = { package = "typepper-engine", version = "…" }`), dispatching on the header; split `replay.rs` into its own crate then; or keep old web builds at `/e/<n>/` |
| **Past boards** | only current periods are browsable | `?period=month&at=2026-09`; the sweep already keeps each month's bests |
| **Delete my data** | players may want their runs gone | `v1 forget`: deleting the player cascades to keys and runs |
| **Staging** | worth it once a migration could hurt real data | a `ctx.environment` branch in `railway.ts`, a `staging` GitHub environment, a second deploy job |
| **Release commit message** | cargo-release titles it `chore: Release` since the workspace move | move `pre-release-commit-message` to a workspace-level `release.toml` |
