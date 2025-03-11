# Tattoy: Eye-candy for your terminal

_Cross-Platform Terminal Compositor_


> [!CAUTION]
> This is _beta_ software for early testers only! Use at your own risk!
> It has many bugs and crashes often. Please report new bugs in the issues ❤️

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

## Usage
* Parse your palette: `cargo run --release -- --capture-palette` or `cargo run -- --parse-palette path_to_screenshot.png`
* Once you've parsed your pale, start with: `cargo run --release`
* Configurable through the automatically generated config file at `$XDG_CONFIG_DIR/tattoy/tattoy.toml`

Logs go to: `$XDG_STATE_DIR/tattoy/tattoy.log` (path configurable in the config file).


