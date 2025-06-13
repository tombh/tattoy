# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/tattoy-org/tattoy/releases/tag/tattoy-v0.1.0) - 2025-06-13

### Added

- update default shader
- prevent Tattoy running in Tattoy
- startup logo
- a quick solution to not rendering blank frames
- `scrollback_size` config
- notifications
- default to screenshotting current window
- improve palatte parssing UX
- shader opacity and layer improvements
- `iChannel` support for shaders
- auto adjust text contrast
- set opacity for plugins in config
- background commands or the "Second Terminal"
- a little blue pixel indicator
- support sending PTY contents to plugins
- plugins
- keybinding for toggling the minimap
- keybindings for cycling through shaders
- keybindings for scrolling
- keybindings
- add support for lossy palette screenshots
- prioritise PTY frame rendering
- logging improvements
- `--main-config` argument
- basic changes for first beta release
- shaders 😏
- minimap slide animation
- refactored tattoys to be more plugin-like
- output diffing
- minimap
- palette config to true colour
- terminal palette parsing
- blending tattoy frames with alpha support
- implemented scrolling
- refactored shadow terminal into its own crate

### Fixed

- *(shaders)* upload blank TTY pixels
- ensure cell under cursor is not a pixel
- list initialised systems in shared state
- startup race condition
- make all Background Command options optional
- support wide UTF8 characters
- support resizing in smokey cursor plugin
- inherit TERM from parent
- pixels in bottom of empty cells
- prevent banding with shader and BGCommand
- always be checking for resize
- require that all tattoys have the TTY size before starting
- *(plugins)* break listener loop on exit
- pass cursor shape from PTY to Tattoy
- pasting large text
- don't render frames in the backlog
- don't use Unix FD for PTY io
- empty cells and default bg cells consistently blend
- avoid debug output per clippy's advice
- direct users to the correct logfile location
- comment out `command` stanza in default_config.toml

### Other

- add .deb, .rpm, AUR and Homebrew
- *(website)* add page about shaders
- setup release-plz
- move blending tests into blender.rs
- rename `OpaqueCell` to `Blender`
- moved smokey cursor to its plugin
- move TerminalProxy files into their own folder
- move input event handling into its own file
- state machine into its own file
- e2e tests for Tattoy using SteppableTerminal
- moved to workspace
