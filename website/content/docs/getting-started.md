+++
title = "Getting Started"
template = "docs.html"
[extra]
weight = 1
+++

First you will need to install the `tattoy` binary. Instructions for your operating system and distro
should be on the [downloads](/download) page.

## Requirements
* The only hard requirement is a terminal that supports true color (and has it enabled), which most modern terminal emulators do. For an in-depth overview of the technical aspects of terminal true color and for a list of terminals that support it, see: [https://github.com/termstandard/colors](https://github.com/termstandard/colors).
* For shader support you will also need a GPU, which almost all modern machines have, even if it's just an integrated one. Most Tattoy features still work without a GPU.

## Palette Parsing
In order for Tattoy to be able to composite the colours of your terminal's palette theme you will need
to provide a screenshot of your palette so that Tattoy can extract the true colour values.

Simply running `tattoy` for the first time will prompt to take a screenshot of the terminal from where
the command was run. If you would rather provide your own screenshot then you can take the screenshot
at this point and provide the file with the argument `tattoy --parse-palette <path/to/file>`. You may also
want to provide your own screenshot if your OS's default screenshotter doesn't work for whatever reason.

You can always re-capture your terminal's palette at any time with `tattoy --capture-palette`.

## Starting Tattoy
Simply run `tattoy` from the CLI.

Tattoy uses your current theme, shell and prompt, so it's not always visually obvious that Tattoy has successfully started. The default configuration has the following notable features:
* **A Shader**: This should be the most obvious indication that Tattoy has started. However, this depends
on successfully connecting to a GPU, which isn't always possible.
* **The Blue Indicator**: This is a small, blue, UTF8 half-block "pixel" located in the very top-right of your
terminal. Tattoy goes to great lengths to ensure that it always cleans up the screen whether it exits successfully or not. Therefore the presence of the blue indicator should be a reliable cue to show that Tattoy is running. Note that it is possible to disable the indicator in the config file.
* **Scrollbar**: Tattoy has a transparent scrollbar on the right hand side that appears when you scroll your terminal (`ALT+s` or mouse scrollwheel) whilst it has scrollback contents (therefore it doesn't appear when scrolling a fresh terminal instance).

## Common Keybindings
* `ALT+t`: Toggle Tattoy's renderer. This returns your terminal back to its normal state without exiting Tattoy itself.
* `ALT+s`: Start scrolling.
* `ALT+9`/`ALT+0`: Cycle back and forth through shaders in the same directory as the current shader.

## Tips
* If you use `is_vim` in `tmux`, it is better to use a `tmux set-option -p @is_vim yes` approach to detect when a `tmux` pane is running (n)vim. See [this comment](https://github.com/christoomey/vim-tmux-navigator/issues/295#issuecomment-1123455337) for inspiration.
