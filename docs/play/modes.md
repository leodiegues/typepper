---
extra:
  covers:
    - crates/typepper-engine/src/lib.rs
    - crates/typepper-engine/src/test.rs
    - crates/typepper-engine/src/lava.rs
    - crates/typepper/src/menu.rs
---

# Modes

Six cards on a carousel. ←/→ picks the mode, ↑/↓ its length or variant.

| Mode | The idea | Lengths | Your best is |
|---|---|---|---|
| **SPRINT** | type the words, fast as you can | 10, 25 or 50 words | wpm |
| **BLITZ** | beat the clock | 15, 30 or 60 seconds | wpm |
| **ZEN** | no clock, no pressure; esc when you're done | | none: it's zen |
| **SURVIVAL** | feed the fuel; it drains faster every level | | words survived |
| **FLOOR IS LAVA** | type the falling words before they hit the lava | lock-on or free aim | words survived |
| **HOT PURSUIT** | outrun the cops; a wrong key stops you cold | | words survived |

Bests are kept for every mode, length and starting level, on your machine
(in the browser, in its storage).

## Levels

Survival, floor is lava and hot pursuit ask where to start: level **1, 4, 7
or 10**. Each level is faster than the last, and every 10 words takes you up
one.

- **Survival:** you start with 10 seconds of fuel; every word adds 1.5. The
  fuel drains faster each level. Run dry and it's over.
- **Floor is lava:** words fall toward a pool of lava. Each one that lands
  raises the lava, so the next land sooner; five and you're cooked.
  - **Lock-on:** your first letter locks onto the lowest word starting with
    it.
  - **Free:** a word clears as soon as what you've typed spells it.
- **Hot pursuit:** a cop chases your caret down the text, from about 25 wpm
  at level 1, faster every 10 words and every 20 seconds. A wrong key stays
  put until you type the right one. If the cop catches up, you're busted.

## Heat

Your speed is heat. Type fast and it climbs; stop and it cools; a mistake
costs more than half of it. Heat is a pepper:

| Rank | Scoville | About the speed that holds it |
|---|---|---|
| BELL PEPPER | 0 | |
| POBLANO | 1,000 | 40 wpm |
| JALAPEÑO | 2,500 | 60 wpm |
| SERRANO | 10,000 | 90 wpm |
| HABANERO | 100,000 | 110 wpm |
| GHOST PEPPER | 855,000 | |
| CAROLINA REAPER | 1,641,000 | 135 wpm and up |

## Combo, multiplier, fever

Every correct letter adds to your combo; a mistake breaks it. Every 20 combo
raises the multiplier, ×1 up to ×4, and at ×4 you're in **fever**. The points
on screen are 10 × multiplier × (your rank + 1) per letter.

Points are for the show. A best, and a place on the boards, is wpm in sprint
and blitz, and words survived in the leveled modes.
