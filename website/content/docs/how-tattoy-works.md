+++
title = "How Tattoy Works"
template = "docs.html"
[extra]
weight = 10
+++

## The Shadow Terminal

The main engine of Tattoy is a fully-featured headless terminal that runs purely in memory. I've called it the "Shadow Terminal" and it's available as a [dedicated crate](https://github.com/tombh/tattoy/tree/main/crates/shadow_terminal).

If you're familiar with headless browser projects like [Selenium](https://www.selenium.dev/) and [Playwright](https://playwright.dev/) then you'll have an idea of what the Shadow Terminal does. Namely it offers a convenient interface to a real, modern terminal running outside of a desktop environment, that can be interacted with just as a normal user would. Whilst it does offer an API for conveniently testing CLI apps (see the [`SteppableTerminal`](https://github.com/tombh/tattoy/blob/main/crates/shadow_terminal/src/steppable_terminal.rs)), its main focus is providing a realtime, machine-readable interface to an in-memory modern terminal emulator. To some this may seem similar to a PTY process, and it is, but with one crucial difference: the Shadow Terminal parses and "renders" ANSI codes. "Render" here doesn't mean rendering fonts, it just means keeping track of the terminal contents such as styling, text flow, cursor position, scrollback, etc.

The state of the headless terminal can be easily queried on a cell-by-cell basis. Tattoy can they get both the contents and styling for every cell and composite that with other layers (shaders or plugins) for example, to the end user's actual terminal. The contents of the Shadow Terminal can also be used as base data for other layers, say for building a minimap view of the scrollback.

The Shadow Terminal is built upon the [`wezterm`](https://github.com/wezterm/wezterm/tree/main/wezterm) crate, a modern, mature and popular terminal emulator.

## User Input

Key presses and mouse input are first parsed by Tattoy itself to check for any Tattoy-specific input, like scrolling, toggling the renderer, etc. All other input data is sent without modification to the Shadow Terminal. Copies of the raw input data are also sent to each layer in case they also want to respond to user input.

## "Graphics"

Any graphics you see in Tattoy is really just a visual trick created by the well-known UTF8 half-block technique. A terminal cell can fit 2 pixel-like squares in it with these characters: ▀, ▄. Then with the help of true colour a crude but powerful form of graphics can be created.

For every frame (which is only ever triggered by underlying changes) Tattoy composites all its layers (eg PTY, shader, plugin) together, depending on both individual cell opacity values and layer-configured values. Text always takes precedence over pixels, but it still tries to honour pixel colours by letting them inform the above text's background colour.


