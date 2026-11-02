---
weight: 3
extra:
  covers:
    - crates/typepper/src/app.rs
    - crates/typepper/src/settings.rs
---

# Keys and settings

The bottom line of every screen says what its keys do. All of them:

## The title screen

| Key | Does |
|---|---|
| ←/→ | pick the mode |
| ↑/↓ | its length (floor is lava: lock-on or free aim) |
| enter or space | play (the hot one asks first: solo, host a room, or join one by its code) |
| b | the boards for this mode |
| l | browser only: log in with GitHub, or log out |
| h | your rounds (the history) |
| s or F2 | settings |
| esc | quit (not in the browser: close the tab) |

Survival, floor is lava and hot pursuit then ask for a starting level: ←/→
(or ↑/↓) picks, enter goes, esc goes back.

## Playing

| Key | Does |
|---|---|
| tab | start over, at once |
| backspace | take back a letter |
| ←/→, enter, 1-6 | the hot one, a plate on the table: turn it, bite the pepper (or bite by its number) |
| esc | the menu; in zen, finish the round |

## Rooms

THE HOT ONE's table (see [rooms](../online/rooms.md)).

| Key | Does |
|---|---|
| enter | the host starts a game; after one, back to the table |
| letters, backspace | the code, when joining |
| esc | leave the table |

## The results

| Key | Does |
|---|---|
| tab or enter | again, from the same level |
| u | share this round (see [share](../online/share.md)) |
| ←/→ | the boards beside the chart: open them, then switch period |
| b | the full board for this mode |
| l | browser only: log in with GitHub, or log out |
| esc | the menu |

## The boards

| Key | Does |
|---|---|
| ←/→ | the period: today, this week, month, year, all time |
| ↑/↓ | pick a row |
| enter | watch that row's round |
| l | browser only: log in with GitHub to be on them, or log out |
| esc | the menu |

## The history

Your last 50 rounds, newest first, kept on this machine.

| Key | Does |
|---|---|
| ↑/↓ | pick a round |
| enter | watch it (offline too) |
| u | share it; in the browser, copy the link of one already shared |
| esc | the menu |

## Settings

↑/↓ picks a row, ←/→ changes it, esc closes. Changes save at once.

| Setting | Choices |
|---|---|
| theme | auto (follows your terminal or browser), dark, light |
| effects | full, or calm: nothing screen-wide moves or flickers, nothing shakes, no fast color pulses |
| font | browser only: Departure Mono, JetBrains Mono, Fira Code, IBM Plex Mono, system |
| online | on, or off: no sharing, no boards, no hints about them |

## On a phone

In the browser on a touch screen: swipe to pick, tap to go, ✕ to go back,
and the ⚙ button for settings. Sharing needs a keyboard (there's no u to
press), but shared links play anywhere.

## Flags

```sh
typepper --replay [FILE]   # watch your last round, or a replay file
typepper --demo            # the bot tours the modes (add --flawless: it never misses)
typepper --light           # or --dark: if auto guesses your terminal wrong
typepper --version         # or -V
```

Watching a replay is hands off: esc leaves, tab or enter plays that mode
yourself.
