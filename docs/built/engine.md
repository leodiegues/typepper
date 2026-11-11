---
weight: 2
extra:
  covers:
    - crates/typepper-engine/src/**
---

# The engine

`crates/typepper-engine` is typepper's rules: typing, heat and ranks,
combos, levels, fuel, the cop, falling lava words. It has no IO and no UI, so
the game (terminal and wasm) and the server run exactly the same code. Its
one hard promise is **determinism**: the same `Setup` and the same inputs
give the same round, on any platform, at any frame rate.

## Layout

| File | Holds |
|---|---|
| `lib.rs` | `VERSION`, `Mode`, `Aim`, `Setup`, `Input`, `Round`, `Outcome`, `simulate`, `Replay::of` / `setup` |
| `test.rs` | `Test`: one typing test (heat, ranks, combo, score, levels, fuel, the cop), `RANKS`, `random_words` |
| `lava.rs` | FLOOR IS LAVA's field: drops, lock-on and free aim, landings |
| `rng.rs` | `Rng`, SplitMix64 written out |
| `replay.rs` | the replay format (see [replays](../reference/replays.md)) |
| `words.txt` | the word list |

The game keeps everything that draws: `ui.rs`, `fx.rs`, `lava.rs` (the field
on screen), `menu.rs`.

## Modes

```rust
pub enum Mode {
    Time(u64),                              // BLITZ 15s / 30s / 60s
    Words(usize),                           // SPRINT 10w / 25w / 50w
    Zen,                                    // no clock; esc ends it
    Survival { start_level: u32 },          // fuel that drains faster every level
    Lava { aim: Aim, start_level: u32 },    // FLOOR IS LAVA, lock-on or free
    Pursuit { start_level: u32 },           // HOT PURSUIT: a cop on your caret
    HotOne,                                 // THE HOT ONE: plates of peppers (see below)
}
```

A mode carries its own options, so "zen from level 7" can't be written.
Leveled modes start from 1, 4, 7 or 10 (`LEVELS`).

| Method | Gives | Used for |
|---|---|---|
| `key()` | `time:30`, `words:25`, `zen`, `survival:4`, `lava:lock:4`, `lava:free:1`, `pursuit:7`, `hot-one` | replays and the server's boards: stable forever |
| `parse(key)` | the mode, only if the game offers it (one spelling per mode) | refusing made-up modes, like `pursuit:99`, so they can't start boards of their own |
| `name()` | `BLITZ 30s`, `FLOOR IS LAVA lock-on`, `HOT PURSUIT` | the screens, share pages, and local bests (as `… from lv4`) |
| `start_level()`, `leveled()`, `from_level(n)` | the level, if any | the level picker |
| `words_scored()` | the leveled modes and the hot one | scores in words, not wpm |

Never change a key, or old replays and the boards' history stop meaning
anything. `name()` is safe to restyle only alongside a migration of saved
bests, since the local bests file is keyed by it.

## A round

```rust
pub struct Setup { pub mode: Mode, pub seed: u64 }
pub enum Input { Char(char), Backspace, End, Pick(u8) }  // End: zen's esc (and a room's win); Pick: a pepper off the plate

impl Round {
    pub fn new(setup: Setup) -> Round;             // t0 = now
    pub fn input(&mut self, at_ms: u32, input: Input);
    pub fn advance(&mut self, to_ms: u32);
    pub fn result(&self) -> Option<Outcome>;       // once it's over
    pub fn test(&self) -> &Test;                   // what the screens draw
    pub fn ms(&self, now: Instant) -> u32;         // live time as round time
    pub log: Vec<(u32, Input)>,                    // the replay
}
pub fn simulate(setup: Setup, inputs: &[(u32, Input)]) -> Option<Outcome>;
```

Everything that plays goes through the same two calls:

| Who | `input` | `advance` |
|---|---|---|
| the live game | on each key, at `round.ms(now)` | every frame, to `round.ms(now)` |
| the bot (`--demo`, `--autopilot`) | at the times it schedules | every frame |
| a watched replay | as each logged input's time comes | every frame |
| the server (`simulate`) | every logged input, straight through | until the round ends |

`input(at_ms, …)` first runs the clock up to `at_ms`, then applies the input,
and logs it if it did anything. Time never runs backwards: an input earlier
than the last one lands at the last one's time. Restarting and leaving aren't
inputs: they abandon the round.

`Outcome { score, wpm, accuracy, words, ms }`: `score` is what's put up
against a best, words in leveled modes, rounded wpm in the rest, none in zen.

`simulate` plays the inputs, then lets the clock run until the round ends by
itself (time up, fuel gone, cooked, busted, last word typed). If it doesn't
end within 10 minutes of the last input, which happens to zen without its esc
or a sprint left unfinished, there's no outcome.

## THE HOT ONE

A `Test` like the other endless modes, with a dinner on top (`test.rs`,
the `hot_one` paths; nothing else in `Test` changed, so the golden numbers
didn't move and `VERSION` stayed 1):

- The clock runs from the round's start (`started` is set at `t0`), so
  everyone at a table shares it.
- A wrong key never lands (like pursuit) and is a **strike**; so is
  `STALL_SECS` (3 s) without a key, once you've typed. A right key clears
  them. `STRIKES` (3) in a row **serves a plate**; so does the clock every
  `COURSE_SECS` (30 s), a course.
- A plate (`Test::plate`) has `PLATE` (5) peppers minus the plates survived
  (never fewer than one), plus one at rank `HOT` and above. Which one is the
  hot one comes from the round's rng when it's served, so a replay serves
  the same plate. Keys don't type while it's on the table, and don't go in
  the log.
- `Input::Pick(n)` bites pepper `n` (only `n < peppers`, only while the plate
  is up; anything else is ignored and not logged). A plate left `PICK_SECS`
  (8 s) picks for itself, from the rng.
- A bite is **chewed** for `BITE_SECS` (1.6 s), nothing typing; then
  `swallow`: the hot one sets `bitten` and finishes the round, any other
  counts a plate survived.
- `Input::End` finishes the round, as in zen: a room's last one standing
  ends theirs with it.

So a round with no inputs at all ends by itself (courses, self-picking
plates, the one-pepper plate at the end): `simulate` never gives up on one.

## Determinism

Four things make a round reproducible:

1. **A fixed timestep.** The clock ticks 120 times a second, at
   `t0 + n/120 s`, whatever the frame rate. Frames only say "run the clock up
   to now". Inputs land between ticks at their exact millisecond, so a run's
   sequence of events (ticks and inputs, in time order) depends only on its
   inputs. Ticks are never recorded.
2. **Whole milliseconds.** Inputs are timed in `u32` ms from the round's
   start; inside, `t0 + Duration::from_millis(ms)` keeps every difference
   exact.
3. **One seeded rng.** Every random pick (the words, the text streamed in
   mid-run, lava's drops and their positions) comes from the round's `Rng`:
   SplitMix64, written out in `rng.rs`, so no crate update can change the
   sequence. `below(n)` is multiply-shift and `unit()` takes the top 53 bits;
   both are fully specified.
4. **The same maths everywhere.** `exp`, `pow` and `log` come from `libm`
   (pure Rust) instead of the platform's math library, which differs between
   macOS, glibc and wasm in the last bit. Plain float arithmetic is IEEE and
   identical; Rust doesn't fuse operations behind your back.

### The tests that hold it

| Test | Checks |
|---|---|
| `a_round_plays_back_the_same_at_any_frame_rate` | 7 modes × 3 frame patterns: a scripted player (typos, a slow patch) plays live; replaying the log gives the same result, score, typed text, **heat bit for bit**, and log |
| `same_numbers_on_every_platform` | three rounds recorded on macOS (score, words, end time, heat bits, score points, log length); CI re-checks them on Linux |
| `splitmix64_reference_values` | the published SplitMix64 sequence |
| `a_replay_plays_its_round_back` | round → bytes → `simulate` → the same outcome |
| `hot_one_*` (four tests), `rounds_without_an_end_give_up` | strikes serve a plate, a mild bite is chewed then typed on from, the hot one ends it; courses on the clock and plates that pick themselves; stalls; plates shrink to one and heat adds a pepper; a round with no inputs ends on its own |

The frame-rate test fails if the fixed timestep is removed (checked by
hand), so it really guards it.

## `VERSION`

```rust
pub const VERSION: u32 = 1;
```

A replay only plays back right on the rules that recorded it. **Bump
`VERSION` when a change would alter any result:** balance numbers, the word
list, how or when the rng is used, the timestep, anything in `Test` or `Lava`
that changes what a key does. Don't bump it for anything else (drawing,
effects, menus, the server).

The golden test trips on any such change and asserts `VERSION == 1` next to
its numbers. A rules change therefore goes:

1. make the change, see `same_numbers_on_every_platform` fail;
2. bump `VERSION`;
3. re-record the golden numbers (print them from the test, paste them in);
4. note it in the changelog if players will feel it.

What a bump means elsewhere: the server answers `err old-engine` to replays
from other versions, and the game's outbox drops them; `typepper --replay`
refuses them. Boards keep old runs, scored when they came in. Playing back
replays recorded by other versions would need the old engine kept around
(see [roadmap](roadmap.md)).
