---
extra:
  covers:
    - crates/typepper/src/boards.rs
    - crates/typepper-server/src/board.rs
---

# Boards

Press **b** on the title screen: the board for the card you're on, from the
starting level you last picked. ←/→ switches between **today, this week, this
month, this year and all time**. Esc goes back.

```text
1  131  leodiegues  2026-10-23
2  118  y0sif       2026-10-21
3  108  OctoTest    2026-10-24
```

Rank, score, login, and the day it was set.

- **The score** is wpm in sprint and blitz, and words survived in survival,
  floor is lava and hot pursuit. Zen has no board.
- **Every length and starting level has its own board**: blitz 15s and blitz
  60s don't compete, and neither do lava from level 1 and from level 10.
- **The top 20** are shown. Linked and lower down? Your own row shows under
  them.
- **Your best counts once**: one row per player. Ties go to whoever got there
  first.
- **Periods are UTC**, weeks start on Monday, and the screen says when the
  current one resets.

Anyone can look, in the terminal or the browser. To be on them,
[link your key](link.md).

## Without the game

The boards are plain text over HTTP:

```sh
curl 'https://api.typepper.com/v1/boards/time:30?period=week'
```

Mode keys and the rest are in the [protocol](../protocol.md#boards).
