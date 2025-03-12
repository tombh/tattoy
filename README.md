# Tattoy: Eye-candy for your terminal

_Cross-Platform Terminal Compositor_


> [!CAUTION]
> This is _beta_ software for early testers only! Use at your own risk!
> It has many bugs and crashes often. Please report new bugs in the issues ❤️

Roadmap milestones for public release: https://github.com/tombh/tattoy/milestone/1


```
                                _.       _..
           _.._      _.._      :$$L      $$$  _.    _..._   ..    ._
     _,;i$$$$$$$  .d$$$$$$L   _.$$$$$$:  $$$d$$: .d$$$$$$$b.`$$b. $$l
    $$$$$$$$P"`` j$$P""4$$$: :$$$$$P" .;i$$$P`` J$$P"```"4$$L T$$b$$:
    "``  :$$    :$$$L..j$$$l  "``$$$  $$P$$$   :$$:      :$$$  :$$$$
         i$$:   T$$$$$P"$$$$     T$$: `  l$$:  `$$b,.___.d$$F   $$$:
         $$$i    `"""`   `""     `""`    ``""    `4$$$$$$$P`   .$$F
          `""                                       `"""`    ;i$$F

```
— _logo by [Sam Foster](https://cmang.org)_

## Live Streamed Development on Twitch
Come join us at: https://www.twitch.tv/tom__bh

## Known Major Issues
* Currently not working on Windows, see this issue for updates: https://github.com/tombh/tattoy/issues/22

## Installation
* Have a Rust installed already: https://www.rust-lang.org/tools/install
* Clone the repo: `git clone https://github.com/tombh/tattoy`
* On Linux you may need these dependencies, eg (for Debian/Ubuntu): `sudo apt install libxcb1-dev libdbus-1-dev pkg-config`.
* Soon upi'll be able to skip instalation once pre-built binaries are available.

## Usage
* Parse your palette: `cargo run --release -- --capture-palette` or `cargo run --release -- --parse-palette path_to_screenshot.png`
* Once you've parsed your pale, start with: `cargo run --release`
* Configurable through the automatically generated config file at `$XDG_CONFIG_DIR/tattoy/tattoy.toml` (not in the repo's `crates/tattoy/default_config.toml`).

> [!WARNING]
> Don't place `tattoy` in your `.bashrc` or `.zshrc`. It's not ready to be a default terminal yet.

## Providing Beta Feedback
It would be really useful if you could try the following:
* Installing
* Parsing your palette
* Checking that the scrollbar appears when you scroll with your mouse (and there is actually text in your scrollback).
* Enabling the minimap, hovering over the right hand column and seeing that the minimap appears.
* Enabling the shaders, and seeing that the point light moves with your cursor.
* Enabling the smokey_cursor and seeing that the smoke particles interact, or "collect", under the text of your terminal.
* Change numbers in the config, like `saturation`, `opacity`, etc and see if they live update in the terminal.
* Find new shaders on https://www.shadertoy.com to try. Currently you can only use single file shaders that _don't_ use the `iChannel0` box (see underneath the code on the shader's webpage). The most interesting shaders will be the ones that are interactive because the terminal cursor is currently providing the `iMouse` value in the shaders.

Logs go to: `$XDG_STATE_DIR/tattoy/tattoy.log` (path configurable in the config file).
