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
