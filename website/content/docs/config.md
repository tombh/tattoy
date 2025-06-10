+++
title = "Configuration"
template = "docs.html"
[extra]
weight = 2
+++

## Config File Location
When starting Tattoy for the first time, the default config file is copied to your filesystem.
  * Linux: `$HOME/.config/tattoy/tattoy.toml`.
  * MacOS: `$HOME/Library/Application Support/tattoy/tattoy.toml`
  * Windows: `%LOCALAPPDATA%\tattoy\tattoy.toml`.

You can start Tattoy with a custom config file using: `tattoy --main-config <path/to/file>`.

Because Tattoy's configuration requires a file containing the terminal palette's true colour values (`palette.toml`), you can also start Tattoy with an entire custom config directory using: `tattoy --config-dir <path/to/directory>`.



## Default Config

```toml
{{ include(path="build-vars/default_config.toml") }}
```
