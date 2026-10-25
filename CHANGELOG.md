# Changelog

What changed in each typepper release, newest first.

## Unreleased

- The leaderboard is now on the results screen, beside the chart: press ←/→ to see where your round would land today, this week, this month, this year or all time, and b for the whole board. Linked players see their rank show up by itself. The boards now open on today
- Upgrade with `typepper upgrade`. It comes with this version, so re-run the installer once to get it (`curl -LsSf https://typepper.com/install.sh | sh`, or the PowerShell line on Windows). After that, one command does it

## [0.4.0] - 2026-10-24

- Share a round: press u on the results screen for a link anyone can open to watch it, key by key, in their browser. No account needed
- Leaderboards: press b on the menu to see the best for today, this week, this month, this year and all time. Link your GitHub account once with `typepper link <your-github-login>` (it uses your ssh key) and your rounds go up by themselves, with your rank on the results screen. Rounds played offline go up the next time you're online
- Watch your last round again with `typepper --replay`
- New setting, online: off, turns all of that off. typepper never needs the internet to play
- Reaper disco: hit Carolina reaper and a mirror ball drops while a break-dancing pepper tackles the crowd and moonwalks
- The version now shows on the title screen (and `typepper --version` prints it)

## [0.3.0] - 2026-10-22

- New mode, HOT PURSUIT: a cop chases your caret down the text you've typed and speeds up with every word and every second, so stay ahead or you're busted. A wrong key stops you until you type the right one. Start from level 1, 4, 7 or 10; bests are kept per starting level (thanks @y0sif!)

## [0.2.0] - 2026-10-16

- New mode, FLOOR IS LAVA: words fall toward rising lava, so type them before they land. Pick lock-on (your first letter locks onto a word) or free (any word you spell clears)
- Play on your phone: tap to play, swipe to pick a mode, and type with your phone's keyboard
- Survival and FLOOR IS LAVA let you pick a starting level (1, 4, 7 or 10), so experienced typers can skip the warm-up; bests are kept per starting level
- typepper.com gets link previews and a GitHub link in the footer

## [0.1.0] - 2026-10-16

The first public release.

- Pick a mode from the carousel: sprint (10, 25 or 50 words), blitz (15, 30 or 60 seconds), zen (no clock) or survival (a fuel clock that drains faster every level)
- Your speed is heat: climb from bell pepper to Carolina reaper, with combos, a multiplier and fever
- Personal bests for every mode and length
- Settings for theme, calmer effects and (in the browser) fonts
- Play at https://typepper.com or install it in your terminal
