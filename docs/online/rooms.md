---
weight: 4
extra:
  covers:
    - crates/typepper/src/room.rs
---

# Rooms: THE HOT ONE with friends

THE HOT ONE is the one mode you can play against people. Press enter on its
card and it asks: **solo**, **host a room**, or **join a room**.

- **Host a room** puts you at a table with a four-letter code, like `KXQP`.
  Say it to your friends. The game starts when you press enter; up to eight
  can sit down.
- **Join a room** takes the code (type it, then enter). Rooms take joiners
  between games, not mid-game.

Everyone types the same words from the same moment: the others' carets ride
along your text in their colors, with their initial, and the board under the
panel shows how far each one is, their strikes, and when a plate comes round
for them ("picking a pepper", "chewing…"). Bite the hot one and you're out;
you keep watching the others until the game's over. **The last one standing
wins.** Out at the same moment, the one with more words places higher.

After the standings, enter takes everyone back to the table for another
game; esc leaves the table.

## Who you are

Linked in the terminal, or logged in in the browser, you're at the table
under your GitHub login; otherwise you're `guest-1`, `guest-2`, … by seat.

## What counts

Every round you play at a table is a round of yours: it counts for your best,
goes into your history, and, linked or logged in, goes up for the boards like
any other. The server plays each finished round's replay before it believes
when you went out or with how many words (see [the protocol](../reference/protocol.md#rooms)).

## Being offline

With **settings → online: off** the card plays solo without asking. A lost
connection says so (`lost typepper.com`); esc goes back to the menu.
