//! Supporting user-defined keybindings.

/// The user config for defining keybindings.
#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Debug, Clone)]
pub(crate) struct KeybindingConfigRaw {
    /// The modifier keys, like `CTRL`, `SHIFT`, etc.
    pub mods: Option<String>,
    /// The actual key, like a 'x' or `PageUp`.
    pub key: String,
}

/// All the possible actions a user can trigger in Tattoy
#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Debug, Clone, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum KeybindingAction {
    /// Toggle Tattoy's rendering. Doesn't effect the TTY.
    ToggleTattoy,
    /// Toggle scrollback scrolling mode.
    ToggleScrolling,
    /// Scroll up. Also triggers scroll mode if it's not currently enabled.
    ScrollUp,
    /// Scroll down.
    ScrollDown,
    /// Exit scrolling mode.
    ScrollExit,
}

/// All the active user-configured keybindings.
pub(crate) type KeybindingsRaw = std::collections::HashMap<KeybindingAction, KeybindingConfigRaw>;

/// The user keybindings converted to native `termwiz::input::KeyEvent`s.
pub(crate) type KeybindingsAsEvents =
    std::collections::HashMap<KeybindingAction, termwiz::input::KeyEvent>;

impl TryFrom<KeybindingConfigRaw> for termwiz::input::KeyEvent {
    type Error = std::io::Error;

    /// This is a bit of hack to get between our config syntax and Wezterm's `KeyEvent` syntax.
    /// `termwiz::input::KeyEvent` doesn't have a `impl From<String>` but it does derive
    /// `serde::Deserialize` so we can use our `toml` crate as a stepping stone. It's a bit of a
    /// hack but there's no performance concerns and it avoids having to manually map all the
    /// keycodes and modifiers.
    fn try_from(binding: KeybindingConfigRaw) -> std::result::Result<Self, Self::Error> {
        let key = if binding.key.len() == 1 {
            format!("{{ Char = \"{}\" }}", binding.key)
        } else {
            format!("\"{}\"", binding.key)
        };

        let config = format!(
            "
                modifiers = {{ bits = 0 }}
                key = {key}
            ",
        );

        let result: core::result::Result<Self, toml::de::Error> = toml::from_str(&config);
        match result {
            Ok(mut key_event) => {
                if let Some(modifiers) = binding.mods {
                    match modifiers.try_into() {
                        Ok(parsed_modifier) => {
                            key_event.modifiers = parsed_modifier;
                        }
                        Err(err) => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidInput,
                                format!("Couldn't parse keybinding modifier: {err:?}"),
                            ))
                        }
                    }
                }
                Ok(key_event)
            }
            Err(error) => {
                let message = format!("Invalid key: {}", error.message());
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Couldn't parse keybinding modifier ({binding:?}): {message}"),
                ))
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn run(config: &str) -> termwiz::input::KeyEvent {
        let parsed: KeybindingConfigRaw = toml::from_str(config).unwrap();
        parsed.try_into().unwrap()
    }

    #[test]
    fn keybinding_x() {
        let config = r#"
            key = "x"
        "#;
        let expected = termwiz::input::KeyEvent {
            modifiers: termwiz::input::Modifiers::NONE,
            key: termwiz::input::KeyCode::Char('x'),
        };

        let actual = run(config);
        assert_eq!(actual, expected);
    }

    #[test]
    fn keybinding_home() {
        let config = r#"
            key = "Home"
        "#;
        let expected = termwiz::input::KeyEvent {
            modifiers: termwiz::input::Modifiers::NONE,
            key: termwiz::input::KeyCode::Home,
        };
        let actual = run(config);
        assert_eq!(actual, expected);
    }

    #[test]
    fn keybinding_shift_x() {
        let config = r#"
            mods = "SHIFT"
            key = "x"
        "#;
        let expected = termwiz::input::KeyEvent {
            modifiers: termwiz::input::Modifiers::SHIFT,
            key: termwiz::input::KeyCode::Char('x'),
        };
        let actual = run(config);
        assert_eq!(actual, expected);
    }

    #[test]
    fn keybinding_capital_x() {
        let config = r#"
            key = "X"
        "#;
        let expected = termwiz::input::KeyEvent {
            modifiers: termwiz::input::Modifiers::NONE,
            key: termwiz::input::KeyCode::Char('X'),
        };
        let actual = run(config);
        assert_eq!(actual, expected);
    }

    #[test]
    fn keybinding_ctrl_home() {
        let config = r#"
            mods = "ALT"
            key = "Home"
        "#;
        let expected = termwiz::input::KeyEvent {
            modifiers: termwiz::input::Modifiers::ALT,
            key: termwiz::input::KeyCode::Home,
        };
        let actual = run(config);
        assert_eq!(actual, expected);
    }

    #[test]
    fn keybinding_ctrl_shift_x() {
        let config = r#"
            mods = "CTRL|SHIFT"
            key = "x"
        "#;
        let expected = termwiz::input::KeyEvent {
            modifiers: termwiz::input::Modifiers::CTRL | termwiz::input::Modifiers::SHIFT,
            key: termwiz::input::KeyCode::Char('x'),
        };
        let actual = run(config);
        assert_eq!(actual, expected);
    }
}
