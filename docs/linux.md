# Linux

mddog runs on Linux with a native CPU renderer: cosmic-text shapes text, Swash rasterizes glyphs, and tiny-skia draws the page. macOS keeps its CoreText and CoreGraphics renderer. Both produce the same RGBA tiles for the terminal, so keys, search, selection, links, and the file list behave the same.

## Requirements

- glibc 2.39 or newer on x86_64 or aarch64. The release binaries are built on Ubuntu 24.04.
- A terminal that speaks the [kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/): Ghostty, kitty, WezTerm, or Rex. GNOME Terminal, Konsole, and foot draw no images, so mddog refuses to start there rather than showing the page as text.
- Fonts, read through fontconfig. DejaVu, Liberation, or Noto cover the body and code text, and STIX covers math. Install one of each:

  ```sh
  # Debian, Ubuntu
  sudo apt install fonts-dejavu-core fonts-liberation fonts-noto-core fonts-stix
  # Fedora
  sudo dnf install dejavu-sans-fonts liberation-sans-fonts google-noto-sans-fonts stix-fonts
  ```

  A theme that names a font family mddog cannot find falls back to the installed sans-serif, monospace, or serif face.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/liyutongzuishuai/mddogforlinux/main/install.sh | sh
```

`MDDOG_INSTALL_DIR` picks the folder, `~/.local/bin` by default, and `MDDOG_RELEASE_URL` picks another release download directory. The script verifies the release's SHA-256 with `sha256sum` or `shasum`.

From a checkout:

```sh
cargo build --release --locked
install -Dm755 target/release/mdd ~/.local/bin/mdd
```

## Appearance

mddog reads the system appearance from the environment it starts in, not from a live desktop setting:

1. `GTK_THEME`, when it names a dark variant, such as `Adwaita:dark`.
2. Otherwise the last field of `COLORFGBG`, when it is an ANSI background color below 7 or equal to 8.
3. Otherwise light.

A theme pair in `config.toml` follows that answer, and the theme list calls the pair *Follow system*. Changing the desktop's appearance takes a restart. To pin one theme, set a single `theme` value.

## Scale

The scale is output pixels per CSS pixel. Linux terminals do not report the scale of the monitor they draw on, so mddog uses the first of:

1. `MDDOG_SCALE`, a positive number such as `1.25`.
2. `GDK_SCALE`, a positive integer.
3. `WINIT_X11_SCALE_FACTOR`, a positive number.
4. `1`.

Set `MDDOG_SCALE` on a HiDPI display whose terminal does not scale its window itself.

## Clipboard and links

A copy goes to the terminal with OSC 52, which also works over SSH, and to the desktop clipboard through the first helper that succeeds:

- `wl-copy --type text/plain;charset=utf-8` when `WAYLAND_DISPLAY` is set.
- `xclip -selection clipboard -in`, then `xsel --clipboard --input`, when `DISPLAY` is set.

Install `wl-clipboard`, `xclip`, or `xsel`; without one, copies still reach the terminal. Web and mail links open through `xdg-open`, and only links whose scheme is `http`, `https`, or `mailto`, as on macOS.

## Files

The remote image cache lives in `$XDG_CACHE_HOME/mddog/remote`, else `~/.cache/mddog/remote`. `MDDOG_CACHE_DIR` moves it. `config.toml` stays in `$XDG_CONFIG_HOME/mddog/config.toml`, else `~/.config/mddog/config.toml`.

## Nix

`flake.nix` exports `x86_64-linux` and `aarch64-linux`. The package wraps `mdd` with a `FONTCONFIG_FILE` that points at DejaVu, Liberation, Noto, and STIX, so the binary finds fonts under a Nix profile.

## Limits

- The viewer allocates its tiles in `/dev/shm`, as the kitty protocol's shared-memory transport requires. A container without `/dev/shm` may show no tiles; run it with a shared memory mount of at least a few hundred MiB.
- A terminal usually takes Ctrl-Shift-C for itself, so select text and use the copy button, Ctrl-c in the page's prompt, or `copy_on_select`.
- The theme hints above are read once at startup. mddog does not watch the desktop portal for appearance changes.
