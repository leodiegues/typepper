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
| enter or space | play |
| b | the boards for this mode |
| s or F2 | settings |
| esc | quit (not in the browser: close the tab) |

Survival, floor is lava and hot pursuit then ask for a starting level: ←/→
(or ↑/↓) picks, enter goes, esc goes back.

## Playing

| Key | Does |
|---|---|
| tab | start over, at once |
| backspace | take back a letter |
| esc | the menu; in zen, finish the round |

## The results

| Key | Does |
|---|---|
| tab or enter | again, from the same level |
| u | share this round (see [share](../online/share.md)) |
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
