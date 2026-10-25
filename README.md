<p align="center">
  <a href="https://typepper.com"><img alt="typepper — a spicy little typing game for your terminal and browser: sprint, blitz, zen, survival, floor is lava and hot pursuit" src="assets/logo/og.png" width="720"></a>
</p>

<p align="center">
  <a href="https://ratatui.rs/"><img alt="Built With Ratatui" src="https://img.shields.io/badge/Built_With_Ratatui-000?logo=ratatui&logoColor=fff"></a>
</p>

<p align="center">
  <img alt="typepper in action: swiping the mode carousel, then a BLITZ run climbing the heat ranks to CAROLINA REAPER with fever, flames and a cheering crowd" src="assets/gifs/hero.gif" width="714">
</p>

## Play

**In your browser:** https://typepper.com/

**In your terminal** (macOS, Linux):

```sh
curl -LsSf https://typepper.com/install.sh | sh
```

Windows (PowerShell):

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/leodiegues/typepper/releases/latest/download/typepper-installer.ps1 | iex"
```

Arch Linux ([AUR](https://aur.archlinux.org/packages/typepper)):

```sh
# Using yay
yay -S typepper-bin   # prebuilt
yay -S typepper       # builds from source

# Using paru
paru -S typepper-bin   # prebuilt
paru -S typepper       # builds from source
```

> [!NOTE]
> The AUR version is community maintained by [@Vaishnav-Sabari-Girish](https://github.com/Vaishnav-Sabari-Girish/), so it may be a bit behind on the latest release.

Or build it yourself: `cargo install --git https://github.com/leodiegues/typepper typepper`

Installed with one of the installers above? `typepper upgrade` gets you the latest version.

Modes, keys, sharing and the boards: [docs/](docs/README.md#for-players).

## A look

Six modes on a carousel: sprint, blitz, zen, survival, floor is lava and hot pursuit.

<p align="center"><img alt="the mode carousel: cards blur toward the edges while each mode's scenery cross-fades in" src="assets/gifs/menu.gif" width="714"></p>

**FLOOR IS LAVA:** type the falling words before they land; every one that does raises the lava.

<p align="center"><img alt="FLOOR IS LAVA from level 10: words fall, one is locked onto and typed, missed words splash into rising lava until the run is cooked" src="assets/gifs/lava.gif" width="714"></p>

**HOT PURSUIT:** a cop chases your caret down the text you've typed, leaving tire tracks; a wrong key stops you cold, and if the cop catches up you're busted.

**Streaks:** the caret catches fire, fever turns the trail to embers, and a broken combo shatters.

<p align="center"><img alt="a hot BLITZ run up close: caret flame, fever trail, then a wrong key shatters a 137 combo" src="assets/gifs/streak.gif" width="644"></p>

Recorded with `demo/record.sh` (the autopilot types; see the script).

## Built with

typepper is a [Ratatui](https://ratatui.rs/) app from top to bottom: every card, flame, and
falling word is drawn with it, and it's what made a game like this fun to build in a terminal.
Thank you to the Ratatui team and community.

- [Ratatui](https://ratatui.rs/): the terminal UI library everything is drawn with
- [Ratzilla](https://github.com/ratatui/ratzilla), also from the Ratatui project: runs the same app in the browser (WebGL2, via [beamterm](https://github.com/junkdog/beamterm))
- [crossterm](https://github.com/crossterm-rs/crossterm): the terminal backend
- [cargo-dist](https://github.com/axodotdev/cargo-dist) builds the releases and installers; [Trunk](https://github.com/trunk-rs/trunk) builds the web version
- [Departure Mono](https://departuremono.com) by Helena Zhang: the pixel font of the web game and the recordings

## Contributing

Ideas, bugs and pull requests are welcome: see [CONTRIBUTING.md](CONTRIBUTING.md)
for setting up and running everything, and [docs/](docs/README.md) for how
typepper is built.

## License

typepper is free software: you can redistribute it and/or modify it under
the terms of the [GNU General Public License](LICENSE) as published by the
Free Software Foundation, either version 3 of the License, or (at your
option) any later version.

The bundled [Departure Mono](https://departuremono.com) font is © Helena
Zhang, under the [SIL Open Font License 1.1](assets/fonts/DepartureMono-LICENSE.txt).
