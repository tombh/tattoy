# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/tattoy-org/tattoy/releases/tag/shadow-terminal-v0.1.0) - 2025-06-13

### Added

- notifications
- auto adjust text contrast
- background commands or the "Second Terminal"
- update wezterm dep for undercurl/colour support
- plugins
- keybindings
- aggregate PTY outputs for performance
- prioritise PTY frame rendering
- logging improvements
- ANSI cursor position response
- shaders 😏
- minimap slide animation
- output diffing
- terminal palette parsing
- blending tattoy frames with alpha support
- implemented scrolling
- refactored shadow terminal into its own crate

### Fixed

- support wide UTF8 characters
- do not accumlate zero bytes from the PTY
- pass cursor shape from PTY to Tattoy
- support terminal 'application mode'
- don't use Unix FD for PTY io
- prevent PTY output sample log from erroring

### Other

- e2e tests for Tattoy using SteppableTerminal
