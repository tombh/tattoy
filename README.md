![Tattoy logo](https://tattoy.sh/assets/screenshots/logo_full.png)

— _logo by [Sam Foster](https://cmang.org)_

# A text-based compositor for modern terminals
![Tattoy with a shader and minmap](https://tattoy.sh/assets/screenshots/hero.webp)

## Usage
See the [documentation](https://tattoy.sh/docs/getting-started/) for full details.

## Debugging
* Set `log_level = "trace"` in `$XDG_CONFIG_DIR/tattoy/tattoy.toml`
* Default log path is `$XDG_STATE_DIR/tattoy/tattoy.log`.
* Log path can be changed with `log_path = "/tmp/tattoy.log"` in `$XDG_CONFIG_DIR/tattoy/tattoy.toml`
* Or log path can be changed per-instance with the `--log-path` CLI argument.
