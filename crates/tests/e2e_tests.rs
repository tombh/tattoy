//! End to end tests
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

    async fn start_tattoy() -> SteppableTerminal {
        let prompt = SteppableTerminal::get_prompt_string("sh".into())
            .await
            .unwrap();
        // Add an extra space to differentiate it from the prompt that is used to start Tattoy.
        let ready_prompt = format!("{prompt} ");

        let config = Config {
            width: 50,
            height: 10,
            command: vec!["sh".into()],
            ..Config::default()
        };
        let mut stepper = SteppableTerminal::start(config).await.unwrap();

        let command = generate_tattoy_command();
        stepper.send_command(&command).unwrap();
        stepper.wait_for_string(&ready_prompt, None).await.unwrap();
        stepper
    }

    // We use the minimum possible ENV to support reproducibility of tests.
    fn generate_tattoy_command() -> String {
        let pwd = std::env::current_dir().unwrap();
        #[expect(
            clippy::option_if_let_else,
            reason = "In this case `match` reads better that `map_or`"
        )]
        let rust_log = match std::env::var_os("RUST_LOG") {
            Some(value) => format!("RUST_LOG=\"{value:?}\""),
            None => String::new(),
        };

        let minimum_env = format!(
            "\
            TERM=xterm-256color \
            SHELL=sh \
            PATH=/usr/bin \
            PWD={pwd:?} \
            {rust_log}\
            "
        );
        format!(
            "exec env --ignore-environment {} {} --use random_walker",
            minimum_env,
            tattoy_binary_path()
        )
    }

    async fn assert_random_walker_moves(tattoy: &mut SteppableTerminal) {
        let iterations = 500;
        tattoy.wait_for_string("▄", Some(500)).await.unwrap();
        let coords = tattoy.get_coords_of_cell_by_content("▄").unwrap();
        for i in 0u16..=iterations {
            tattoy.render_all_output();
            assert!(
                i != iterations,
                "Random walker didn't move in a {iterations} iterations."
            );

            tattoy.wait_for_string("▄", Some(500)).await.unwrap();
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
        let mut tattoy = start_tattoy().await;

        assert_random_walker_moves(&mut tattoy).await;

        tattoy.send_command("echo $((1+1))").unwrap();
        tattoy.wait_for_string("2", None).await.unwrap();

        assert_random_walker_moves(&mut tattoy).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn resizing() {
        setup_logging();
        let mut tattoy = start_tattoy().await;
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
            .wait_for_string_at("^X Exit", 0, resized_bottom, None)
            .await
            .unwrap();
        let resized_menu_item_paste = tattoy
            .get_string_at(resized_right - 10, resized_bottom, 5)
            .unwrap();
        assert_eq!(resized_menu_item_paste, "Paste");

        assert_random_walker_moves(&mut tattoy).await;
    }
}
