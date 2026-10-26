# typepper's docs

Playing typepper, and how it's built.

## For players

| Doc | Read it for |
|---|---|
| [play/install.md](play/install.md) | the browser, the installers, `typepper upgrade` |
| [play/modes.md](play/modes.md) | the seven modes, levels, heat and ranks, what counts as a best |
| [play/keys.md](play/keys.md) | every key, the settings, playing on a phone, flags |
| [online/share.md](online/share.md) | sharing a round as a link |
| [online/link.md](online/link.md) | linking your ssh key to your GitHub login |
| [online/boards.md](online/boards.md) | the leaderboards, in the game and over HTTP |
| [online/rooms.md](online/rooms.md) | THE HOT ONE with friends: hosting and joining a room |

## How it's built

Beyond what the code says. Start with [architecture](built/architecture.md); the
rest goes deeper on one part each.

| Doc | Read it for |
|---|---|
| [architecture.md](built/architecture.md) | the whole system: principles, components, hosts, crates, how a round travels, what's trusted |
| [engine.md](built/engine.md) | the rules crate and why a round plays back the same everywhere: `Round`, `Mode`, the fixed timestep, the rng, `VERSION` |
| [replays.md](reference/replays.md) | the replay format byte by byte, decoding rules, watching and sharing replays |
| [protocol.md](reference/protocol.md) | what the game and the server say to each other, over SSH and HTTP |
| [server.md](built/server.md) | `typepper-server`: modules, database, ids, accounts and linking, boards, the sweep, configuration |
| [game-client.md](built/game-client.md) | the game's online side: transports, the outbox, sharing, the boards screen, local files, flags |
| [operations.md](built/operations.md) | Railway, deploys, secrets, CI, pinned tools, local development, the runbook |
| [decisions.md](built/decisions.md) | why things are the way they are |
| [roadmap.md](built/roadmap.md) | what's open |

To work on typepper, see [CONTRIBUTING.md](../CONTRIBUTING.md).

## Where `online.md` went

These docs replace `docs/online.md`, the plan the online features were built
from. The migrations, which can't change once applied, still name its
sections:

| `online.md` section | Now in |
|---|---|
| Premise, Architecture, Repository layout | [architecture.md](built/architecture.md) |
| Engine contract | [engine.md](built/engine.md) |
| Protocol: Replay bytes | [replays.md](reference/replays.md) |
| Protocol: Submission, Commands, Responses, Server limits | [protocol.md](reference/protocol.md) |
| Identity, Database, Boards, Sweep | [server.md](built/server.md) |
| Game client | [game-client.md](built/game-client.md) |
| Deploys | [operations.md](built/operations.md) |
| Phases, Open, Risks | [roadmap.md](built/roadmap.md), [decisions.md](built/decisions.md) |
