---
weight: 2
extra:
  covers:
    - crates/typepper/src/online.rs
    - crates/typepper/src/main.rs
---

# Link your key

Linking puts your rounds on the [boards](boards.md), under your GitHub login.
There's no sign-up and no password: you prove the login with an ssh key you
already have on GitHub.

```sh
typepper link <your-github-login>
```

```text
linked to github.com/<login>: your rounds go up from now on
```

It works if one of your ssh keys is listed at
`https://github.com/<your-github-login>.keys`, which is every key you've
added to GitHub. Your ssh agent may ask to use it; that's the one time it
asks.

## In the browser

Press **l** on the title screen, the boards, or after a round to log in
with GitHub. GitHub asks once to
authorize typepper, which reads only your public profile. You come back
logged in, and your rounds go up as you, just like a linked terminal game.
Your bests from the terminal show on the mode cards too, a moment after the
game opens (only rounds that went up count). **l** again logs out.

If the login runs out (or you clear the site's data), your next rounds wait
in the browser until you log in again. Rounds played while logged out stay
anonymous.

## After linking

- Your login shows under the logo on the title screen, next to the version.
- Every round you finish goes up by itself, as you, and the results line
  says where it landed: `saved: r.typepper.com/… · #3 all time`.
  Today's board opens beside the chart, with your row in it.
- Offline? Rounds wait on your machine and go up with the next one played
  online.
- At a table ([rooms](rooms.md)) you're seated under your login, and the
  rounds you play there go up like the rest.
- Settings → online: off stops it all. Your rounds stay on your machine.

## Unlinking

```sh
typepper unlink
```

That lets go of the key on typepper.com and forgets the account on this
machine. Your rounds already up stay on the boards.

## When it doesn't link

```text
typepper: couldn't link: ssh didn't get in (see above): either typepper.com
is unreachable, or none of your ssh keys is on github.com/<login>.keys
```

ssh's own message, just above, says which:

- **`Permission denied`**: none of the keys ssh offered is on that GitHub
  account. Add one at https://github.com/settings/keys, or check which keys
  your agent has with `ssh-add -l`.
- **Anything about connecting** (timed out, refused): typepper.com didn't
  answer; try again later.

The same thing by hand, for the curious, is in the
[protocol](../reference/protocol.md#linking).
