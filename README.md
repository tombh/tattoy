# Tattoy: Eye-candy for your terminal

Currently running with:

```
SHELL=zsh RUST_BACKTRACE=1 RUST_LOG="none,tattoy=trace" cargo run -- --use smokey_cursor
```

Testing with:

```
RUST_LOG="none,tattoy=trace" cargo test -- --nocapture
```

Generate docs with:
`cargo doc --no-deps --document-private-items --open`

Logs go to: `./tattoy.log`

## TODO
* [x] Background colour of " " (space) isn't passed through.
* [x] Bold doesn't get passed through properly, run `htop` to see.
* [x] Resizing isn't detected.
* [x] Cursor isn't transparent.
* [x] Send surface updates to state only, then protocol sends small signal not big update.
* [x] Look into performance, especially scrolling in nvim.
* [x] `CTRL-D` doesn't fully return to terminal, needs extra `CTRL-C`.
* [x] Resizing is broken.
* [ ] Look at projects like Ratatui to see how to do integration tests.
* [ ] Implement scrollback/history.
* [ ] How should smokey_cursor particles respond to resizing?
* [ ] Detect alternate screen so to hide cursor
* [ ] Up and down aren't detected in `less` or `htop`.
* [ ] User-friendly error output for known errors
* [ ] Double width characters aren't passed through, eg "🦀".
* [ ] Bug: `atuin` can't get the cursor position. Maybe I need to intercept '\e[6n'?
* [ ] Don't log to file by default
* [ ] Explore rendering a frame even if any of the surfaces aren't the right size, in order to not prevent updates from other surfaces.
* [ ] Use `tokio::select!` in Loader
* [ ] Tattoy-specific keybinding to toggle all tattoys on and off.
* [ ] `tmux` mouse events cause runaway behaviour in `htop`.
* [ ] Explore returning errors in tasks/threads, joining them and acting on them. Instead of sending the error to shared state.
* [ ] Centralise place where app exits and outputs backtrace and messages etc.
* [ ] Doesn't work on Nushell. Just freezes.
* [ ] More profiling. I tried https://github.com/mstange/samply and https://github.com/flamegraph-rs/flamegrap but they had some obscure errors which I assumed were from my CPU architecture, Asahi M1 etc.

## Design

### Terminals/Surfaces
There are quite a few terminals, PTYs, shadow PTYs, surfaces, etc, that are all terminal-like in some way, but do different things.

* __The user's actual real terminal__ We don't really have control of this. Or rather, Tattoy as an application merely is a kind of magic trick that reflects the real terminal whilst sprinkling eye-candy onto it. The goal of Tattoy is that you should _always_ be able to recover your original untouched terminal.
* __The PTY (pseudo TTY) of the "original" terminal process__ To achieve the magic trick of Tattoy we manage a "shadow" subprocess of the user's real terminal. It is managed completely in memory and is rendered headlessly by yet another "terminal" (see shadow TTY). The PTY code itself is provided by the [portable_pty](https://docs.rs/portable-pty/latest/portable_pty/) crate from the [Wezterm project](https://github.com/wez/wezterm) ❤️.
* __The shadow PTY of the "original" terminal screen__ This is just a headless rendering of the underlying shadow PTY. It is a virtual terminal. It is a purely in-memory representation of the PTY and hence of the user's original terminal. This is done with a [wezterm_term::Terminal](https://github.com/wez/wezterm/blob/main/term/README.md).
* __The Tattoy magic surface__ A surface here refers to a [termwiz::surface::Surface](https://github.com/wez/wezterm/tree/main/termwiz). It represents a terminal screen, but is not an actual real terminal, it's merely a convenient visual representation. This is where we can create all the magical Tattoy eye-candy. Although it does not intefere with the shadow TTY, it can be informed by it. Hence why you can create Tattoys that seem to interact with the real terminal. In the end, this Tattoy surface is composited with the contents of the shadow PTY.
* __The shadow PTY surface__ This is merely a copy of the current visual status of the shadow TTY. We don't use the actual shadow TTY as the source because it's possible that this data is queried frequently by various Tattoys. Querying the a static visual representation is more efficient than querying a TTY, even if it exists only in memory.
* __The final composite surface__ This is the final composited surface of the both the underlying shadow PTY and all the active Tattoys. A diff of this with the user's current real terminal is then used to do the final update.
