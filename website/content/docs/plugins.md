+++
title = "Plugins"
template = "docs.html"
[extra]
weight = 5
+++

Plugins can be written in any language, they just need to be executable and support JSON input and output over STDIO. A plugin can be defined with TOML in the standard `tattoy.toml` file. Here is an example:
```toml
[[plugins]]
name = "my-cool-plugin"
path = "/path/to/plugin/executable"
enabled = true
# Layer `0` has special meaning: that this plugin will completely replace the user's TTY.
layer = -5
```

See the [tattoy-protocol](https://github.com/tombh/tattoy/tree/main/crates/tattoy-protocol) crate for more docs and details about the plugin architecture.

There are [example Rust plugins](https://github.com/tombh/tattoy/tree/main/crates/tattoy-plugins) in the main Tattoy repo.

### Output (via STDOUT)

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

### Input (via STDIN)

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
