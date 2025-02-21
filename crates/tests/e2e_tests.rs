//! End to end tests

#[expect(
    clippy::large_futures,
    clippy::unreadable_literal,
    reason = "
        These are just tests, and the downsides should mainfest as a showstopping stack
        overflow, so we'll know about it soon enough.
    "
)]
#[cfg(test)]
mod e2e {
    use shadow_terminal::{shadow_terminal::Config, steppable_terminal::SteppableTerminal};

    fn workspace_dir() -> std::path::PathBuf {
        let output = std::process::Command::new(env!("CARGO"))
            .arg("locate-project")
            .arg("--workspace")
            .arg("--message-format=plain")
            .output()
            .unwrap()
            .stdout;
        let cargo_path = std::path::Path::new(std::str::from_utf8(&output).unwrap().trim());
        let workspace_dir = cargo_path.parent().unwrap().to_path_buf();
        tracing::debug!("Using workspace directory: {workspace_dir:?}");
        workspace_dir
    }

    fn tattoy_binary_path() -> String {
        workspace_dir()
            .join("target/debug/tattoy")
            .display()
            .to_string()
    }

    async fn start_tattoy(maybe_config_path: Option<String>) -> SteppableTerminal {
        let shell = "bash";
        let prompt = "tattoy $ ";

        let config = Config {
            width: 50,
            height: 10,
            command: vec![shell.into()],
            ..Config::default()
        };
        let mut stepper = SteppableTerminal::start(config).await.unwrap();

        let command = generate_tattoy_command(shell, prompt, maybe_config_path);
        stepper.send_command(&command).unwrap();
        stepper.wait_for_string(prompt, None).await.unwrap();
        assert_random_walker_moves(&mut stepper).await;
        stepper
    }

    // We use the minimum possible ENV to support reproducibility of tests.
    fn generate_tattoy_command(
        shell: &str,
        prompt: &str,
        maybe_temp_dir: Option<String>,
    ) -> String {
        let pwd = std::env::current_dir().unwrap();
        #[expect(
            clippy::option_if_let_else,
            reason = "In this case `match` reads better that `map_or`"
        )]
        let rust_log = match std::env::var_os("RUST_LOG") {
            Some(value) => format!("RUST_LOG=\"{value:?}\""),
            None => String::new(),
        };

        let config_path = match maybe_temp_dir {
            None => {
                let temp_dir = tempfile::tempdir().unwrap();
                temp_dir.path().display().to_string()
            }
            Some(path) => path,
        };

        let minimum_env = format!(
            "\
            TERM=xterm-256color \
            SHELL='{shell}' \
            PATH=/usr/bin:/bin:/sbin:/usr/local/bin:/usr/sbin \
            PWD={pwd:?} \
            PS1='{prompt}' \
            {rust_log} \
            "
        );
        format!(
            "\
            unset $(env | cut -d= -f1) && \
            {} {} \
            --use random_walker \
            --command 'bash --norc --noprofile' \
            --config-dir {} \
            ",
            minimum_env,
            tattoy_binary_path(),
            config_path
        )
    }

    async fn assert_random_walker_moves(tattoy: &mut SteppableTerminal) {
        let iterations = 1000;
        tattoy.wait_for_string("▄", Some(iterations)).await.unwrap();
        let coords = tattoy.get_coords_of_cell_by_content("▄").unwrap();
        for i in 0..=iterations {
            tattoy.render_all_output();
            assert!(
                i != iterations,
                "Random walker didn't move in a {iterations} iterations."
            );

            tattoy.wait_for_string("▄", Some(iterations)).await.unwrap();
            let next_coords = tattoy.get_coords_of_cell_by_content("▄").unwrap();
            if coords != next_coords {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }
    }

    fn setup_logging() {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .init();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn basic_interactivity() {
        let mut tattoy = start_tattoy(None).await;

        assert_random_walker_moves(&mut tattoy).await;

        tattoy.send_command("echo $((1+1))").unwrap();
        tattoy.wait_for_string("2", None).await.unwrap();

        assert_random_walker_moves(&mut tattoy).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn resizing() {
        let mut tattoy = start_tattoy(None).await;
        tattoy.send_command("nano --restricted").unwrap();
        tattoy.wait_for_string("GNU nano", None).await.unwrap();

        assert_random_walker_moves(&mut tattoy).await;

        let size = tattoy.shadow_terminal.terminal.get_size();
        let bottom = size.rows - 1;
        let right = size.cols - 1;
        tattoy
            .wait_for_string_at("Paste", right - 10, bottom, None)
            .await
            .unwrap();

        tattoy
            .shadow_terminal
            .resize(
                u16::try_from(size.cols + 3).unwrap(),
                u16::try_from(size.rows + 3).unwrap(),
            )
            .unwrap();
        let resized_size = tattoy.shadow_terminal.terminal.get_size();
        let resized_bottom = resized_size.rows - 1;
        let resized_right = resized_size.cols - 1;
        tattoy
            .wait_for_string_at("^X Exit", 0, resized_bottom, Some(1000))
            .await
            .unwrap();
        let resized_menu_item_paste = tattoy
            .get_string_at(resized_right - 10, resized_bottom, 5)
            .unwrap();
        assert_eq!(resized_menu_item_paste, "Paste");

        assert_random_walker_moves(&mut tattoy).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn scrolling() {
        async fn assert_scrolling_off(tattoy: &mut SteppableTerminal) {
            let size = tattoy.shadow_terminal.terminal.get_size();
            let bottom = size.rows - 1;
            let right = size.cols - 1;
            tattoy
                .wait_for_string_at("nulla pariatur?", 0, bottom - 1, None)
                .await
                .unwrap();

            // Check for absence of scrollbar
            tattoy
                .wait_for_bg_color_at(None, right, bottom - 3, None)
                .await
                .unwrap();
        }

        async fn assert_scrolled_up(tattoy: &mut SteppableTerminal) {
            let size = tattoy.shadow_terminal.terminal.get_size();
            let bottom = size.rows - 1;
            let right = size.cols - 1;
            tattoy
                .wait_for_string_at("riosam, nisi", 0, bottom, None)
                .await
                .unwrap();

            // Check for scrollbar
            tattoy
                .wait_for_bg_color_at(
                    Some((0.33333334, 0.33333334, 0.33333334, 1.0)),
                    right,
                    bottom - 3,
                    None,
                )
                .await
                .unwrap();
        }

        setup_logging();
        let escape = "\x1b";
        let mouse_up = "\x1b[<64;14;2M";
        let mouse_down = "\x1b[<65;15;5M";

        let mut tattoy = start_tattoy(None).await;

        tattoy
            .send_command("cat resources/LOREM_IPSUM.txt")
            .unwrap();
        assert_scrolling_off(&mut tattoy).await;

        tattoy.send_input(mouse_up).unwrap();
        assert_scrolled_up(&mut tattoy).await;

        tattoy.send_input(mouse_down).unwrap();
        tattoy.send_input(mouse_down).unwrap();
        assert_scrolling_off(&mut tattoy).await;

        tattoy.send_input(mouse_up).unwrap();
        assert_scrolled_up(&mut tattoy).await;

        tattoy.send_input(escape).unwrap();
        assert_scrolling_off(&mut tattoy).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn palette_to_true_colour() {
        let config_dir = tempfile::tempdir().unwrap();
        let config_path = config_dir.path();
        std::fs::copy("resources/palette.toml", config_path.join("palette.toml")).unwrap();

        let mut tattoy = start_tattoy(Some(config_path.display().to_string())).await;

        tattoy
            .send_command("echo -e \"\\033[0;31m$((1000-1))\\033[m\"")
            .unwrap();
        tattoy.wait_for_string("999", None).await.unwrap();

        let cell = tattoy.get_cell_at(0, 1).unwrap().unwrap();

        assert_eq!(cell.str(), "9");
        assert_eq!(
            cell.attrs().foreground(),
            termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(
                termwiz::color::SrgbaTuple(0.96862745, 0.4627451, 0.5568628, 1.0)
            ),
        );
    }
}
