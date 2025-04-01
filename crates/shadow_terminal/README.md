# Shadow Terminal
A fully-functional, fully-rendered terminal purely in memory.

Useful for terminal multiplexers (a la `tmux`, `zellij`) and end to end testing TUI applications.

Making a live terminal that automatically updates as you send it input and as any programs running in it send output.
```rust
let config = ShadowTerminalConfig::default();
let active_terminal = shadow_terminal::active_terminal::ActiveTerminal::start(config);
active_terminal.send_input(forwarded_stdin_bytes);
let surface = shadow_terminal.surface_output_rx.recv().await;
dbg!(surface);
```

An example of a basic end to end test using the `SteppableTerminal`.
```rust
let config = ShadowTerminalConfig::default();
let mut stepper = SteppableTerminal::start(config).await.unwrap();
stepper.send_string("echo $((1+1))\n").unwrap();
stepper.wait_for_change().await.unwrap();
let output = stepper.screen_as_string().unwrap();
assert_eq!(
    output,
    indoc::formatdoc! {"
        {prompt} echo $((1+1))
        2
        {prompt} 
    "}
);
```

## Testing
* End to end tests depend on `nano` (to help text resizing the terminal).

## TODO
* [ ] Every test has to be marked with `#[tokio::test(flavor = "multi_thread")]` otherwise tests can hang. I'm not sure why, I'd really like to know.
* [ ] If `#[tokio::test(flavor = "multi_thread")]` is really necessary it'd be nice if there was a way to globally set it for tests.


## Notes
* Useful ANSI code resources:
  * https://gist.github.com/ConnerWill/d4b6c776b509add763e17f9f113fd25b
  * https://www.qnx.com/developers/docs/qnx_4.25_docs/qnx4/utils/d/devansi.html
