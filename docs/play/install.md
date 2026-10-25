---
extra:
  covers:
    - install.sh
    - dist-workspace.toml
---

# Install

## In your browser

https://typepper.com/. Nothing to install. On a phone it plays by touch.

## In your terminal

macOS and Linux:

```sh
curl -LsSf https://typepper.com/install.sh | sh
```

Windows (PowerShell):

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/leodiegues/typepper/releases/latest/download/typepper-installer.ps1 | iex"
```

Arch Linux, from the [AUR](https://aur.archlinux.org/packages/typepper)
(community maintained, so it may be a release behind):

```sh
yay -S typepper-bin   # prebuilt
yay -S typepper       # builds from source
```

From source, with Rust:

```sh
cargo install --git https://github.com/leodiegues/typepper typepper
```

Then run `typepper`.

## Staying current

```sh
typepper upgrade
```

That works when you installed with one of the installers above. From the AUR
or `cargo`, upgrade it the same way you installed it.

`typepper --version` (or `-V`) prints the version; the title screen shows it
too.

## Online needs ssh

Sharing, boards and linking go over `ssh` in the terminal, the one already on
your machine. Without it the game still plays; it just says "ssh isn't
installed" when you try to share.
