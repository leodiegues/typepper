---
extra:
  covers:
    - crates/typepper/src/online.rs
---

# Share a round

Press **u** on the results screen. The round goes up, and you get a link:

```text
shared: r.typepper.com/7Xf9qyuC
```

In the browser the link is copied for you (`copied: …`). Anyone who opens it
gets a preview card with the score and mode, then watches your round play
back in their browser, key by key, and can play the mode themselves.

## What goes up

The round's replay: the mode, and every key you pressed with its time. The
server plays it back to score it, so a shared score is the real one. No
account, no name: an unlinked share is anonymous.

Linked? Your rounds go up on their own, under your login (see
[link your key](link.md)), and u only retries one that couldn't.

## How long a link lasts

8 days. A linked player's best round of the month stays as long as the
boards need it.

## When there's no u

- Settings → online is off.
- A touch screen: no u to press.
- You're watching someone else's round.

If sharing fails, the results line says why (`couldn't send: …`); press u
again.

## Your last round, offline

The game keeps your last round on your machine, online or not:

```sh
typepper --replay
```
