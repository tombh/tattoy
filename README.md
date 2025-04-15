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

## Windows
Tattoy largely works on Windows, but I don't have a local Windows machine to easily test on, so would appreciate feeedback. See this issue for more details: https://github.com/tombh/tattoy/issues/42

## Installation
* Have a Rust installed already: https://www.rust-lang.org/tools/install
* Clone the repo: `git clone https://github.com/tombh/tattoy`
* On Linux you may need these dependencies, eg (for Debian/Ubuntu): `sudo apt install libxcb1-dev libdbus-1-dev pkg-config`.
* Soon you'll be able to skip installation once pre-built binaries are available.

## Usage
* Parse your palette: `cargo run --release -- --capture-palette` or `cargo run --release -- --parse-palette path_to_screenshot.png`
* Once you've parsed your palette, start with: `cargo run --release`
* Configurable through the automatically generated config file at `$XDG_CONFIG_DIR/tattoy/tattoy.toml` (not in the repo's `crates/tattoy/default_config.toml`).
* Note that Tattoy replaces your terminal, it may even look exactly the same as your existing terminal at first. So it can't be exited with `CTRL+C`. You exit as you would exit a normal shell, therefore with `CTRL+D` or running the `exit` command.

> [!WARNING]
> Don't place `tattoy` in your `.bashrc` or `.zshrc`. It's not ready to be a default terminal yet.

> [!TIP]
> If you use `is_vim` in `tmux`, it is better to use a `tmux set-option -p @is_vim yes` approach to detect when a `tmux` pane is running (n)vim. See [this comment](https://github.com/christoomey/vim-tmux-navigator/issues/295#issuecomment-1123455337) for inspiration.

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

## Debugging
* Set `log_level = "trace"` in `$XDG_CONFIG_DIR/tattoy/tattoy.toml`
* Default log path is `$XDG_STATE_DIR/tattoy/tattoy.log`.
* Log path can be changed with `log_path = "/tmp/tattoy.log"` in `$XDG_CONFIG_DIR/tattoy/tattoy.toml`
* Or log path can be changed per-instance with the `--log-path` CLI argument.

## Writing Plugins
Plugins can be written in any language, they just need to be executable and support JSON input and output over STDIO. A plugin can be defined with TOML in the standard `tattoy.toml` file. Here is an example:
```toml
[[plugins]]
name = "my-cool-plugin"
path = "/path/to/plugin/executable"
enabled = true
# Layer `0` has special meaning: that this plugin will completely replace the user's TTY.
layer = -5
```

See the [crates/tattoy-protocol](crates/tattoy-protocol) crate for more docs and details about the plugin architecture.

There is an example Rust plugin at [crates/tattoy-plugins/inverter](crates/tattoy-plugins/inverter).

### Plugin Output (sent on STDOUT)

#### Render text of arbitrary length in the terminal
```json
{
    "output_text": {
        "text": "foo",
        "coordinates": [1, 2],
        "bg": null,
        "fg": [0.1, 0.2, 0.3, 0.4],
    }
}
```

#### Render an arbitrary amount of cells in the terminal
Note that it does not need to include blank cells.
```json
{
    "output_cells": [{
        "character": "f",
        "coordinates": [1, 2],
        "bg": null,
        "fg": [0.1, 0.2, 0.3, 0.4],
    }]
}
```

#### Renders pixels in the terminal
Note that the y-coordinate is twice the height of the terminal.
```json
{
    "output_pixels": [{
        "coordinates": [1, 2],
        "color": [0.1, 0.2, 0.3, 0.4],
    }]
}
```

### Plugin Input (read from STDIN)

#### The current contents of the PTY screen
Note that it does not contain any of the scrollback.
```json
{
    "pty_update": {
        "size": [1, 2],
        "cells": [{
            "character": "f",
            "coordinates": [1, 2],
            "bg": null,
            "fg": [0.1, 0.2, 0.3, 0.4],
        }]
    }
}

```

#### A terminal resize event
```json
{
    "tty_resize": {
        "width": 1,
        "height": 2,
    }
}
```
