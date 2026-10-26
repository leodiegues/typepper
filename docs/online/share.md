---
weight: 1
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

Linked, or logged in in the browser? Your rounds go up on their own, under
your login (see [link your key](link.md)), and u only retries one that
couldn't. A round played at a table ([rooms](rooms.md)) is one of yours
like any other: it goes up the same way, and can be shared the same way.

## How long a link lasts

8 days. A linked player's best round of the month stays as long as the
boards need it.

## When there's no u

- Settings → online is off.
- A touch screen: no u to press.
- You're watching someone else's round.

If sharing fails, the results line says why (`couldn't send: …`); press u
again.

## Finding a link again

Press **h** on the menu: your last 50 rounds, kept on your machine (in the
browser, by the browser), each with its link once it went up. Enter watches
one, offline too; u shares one that hasn't gone up yet. In the browser, u on
a shared round copies its link again.

The boards play their rounds as well: ↑/↓ picks a row, enter watches it.

## Your last round, from the shell

```sh
typepper --replay
```
