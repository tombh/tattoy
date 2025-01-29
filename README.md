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
* [ ] Background colour of " " (space) isn't passed through.
* [ ] Greys or bold don't get passed through properly, run `htop` to see.
* [ ] Mouse events don't seem to be faithfully parsed see `htop`.
* [ ] Resizing isn't detected.
* [ ] Double width characters aren't passed through, eg "🦀".
* [ ] Up and down aren't detected in `less`.
* [ ] `CTRL-D` doesn't fully return to terminal, needs extra `CTRL-C`.
* [ ] Doesn't work on Nushell. Just freezes.
