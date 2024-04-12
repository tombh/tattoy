//! Docs

use termwiz::color::SrgbaTuple;
use termwiz::escape::parser::Parser;
use termwiz::escape::Action;
use termwiz::surface::{Change, Position, Surface};
use termwiz::terminal::buffered::BufferedTerminal;
use termwiz::terminal::Terminal as TermwizTerminal;
use wezterm_term::color::ColorPalette;
use wezterm_term::{Terminal, TerminalConfiguration, TerminalSize};

use std::io::Result;
use std::sync::Arc;

use rand::Rng;

use tokio::sync::mpsc;

use crate::pty::PTY;

enum SurfaceType {
    BGSurface,
    PTYSurface,
}

struct TattoySurface {
    kind: SurfaceType,
    surface: Surface,
}

#[derive(Debug)]
struct TermConfig {
    scrollback: usize,
}
impl TerminalConfiguration for TermConfig {
    fn scrollback_size(&self) -> usize {
        self.scrollback
    }

    fn color_palette(&self) -> ColorPalette {
        ColorPalette::default()
    }
}

/// Docs
/// # Panics
/// Because it's async?
#[allow(
    clippy::print_stdout,
    clippy::wildcard_enum_match_arm,
    clippy::use_debug
)]
pub async fn run() -> Result<()> {
    let log_file = "tattoy.log";
    let file = std::fs::File::create(log_file).unwrap();
    tracing_subscriber::fmt()
        .with_writer(file)
        .with_env_filter(tracing_subscriber::filter::EnvFilter::from_default_env())
        .init();
    tracing::info!("Starting Schlam");

    // Make sure the buffer size isn't too big, because `terminal.advance_bytes()` even reads
    // the empty bytes.
    let (pty_output_tx, mut pty_output_rx) = mpsc::unbounded_channel::<[u8; 128]>();
    let (bg_screen_tx, mut screen_rx) = mpsc::unbounded_channel();
    let pty_screen_tx = bg_screen_tx.clone();

    let caps = termwiz::caps::Capabilities::new_from_env().unwrap();
    let mut terminal = termwiz::terminal::new_terminal(caps).unwrap();
    terminal.set_raw_mode().unwrap();
    let size = terminal.get_screen_size().unwrap();
    let mut buf = BufferedTerminal::new(terminal).unwrap();

    let width = size.cols;
    let height = size.rows;

    let shell = "zsh";

    let mut term = Terminal::new(
        TerminalSize {
            rows: height,
            cols: width,
            pixel_width: 10 * 8,
            pixel_height: 10 * 16,
            dpi: 0,
        },
        Arc::new(TermConfig { scrollback: 100 }),
        "Tattoy",
        "O_o",
        Box::new(Vec::new()),
    );

    let pty_output_worker = std::thread::spawn(move || {
        let mut parser = Parser::new();

        loop {
            let output = pty_output_rx.blocking_recv();
            match output {
                Some(o) => {
                    term.advance_bytes(o);

                    parser.parse(&o, |action| match action {
                        Action::Print(c) => (),
                        // Action::Control(c) => match c {
                        //     ControlCode::HorizontalTab
                        //     | ControlCode::LineFeed
                        //     | ControlCode::CarriageReturn => print!("{}", c as u8 as char),
                        //     _ => {}
                        // },
                        // Action::CSI(csi) => {}
                        _o => {
                            // print!("{o}");
                        }
                    });
                }
                None => (),
                // Err(_e) => (),
            }

            let mut block = Surface::new(width, height);

            let s = term.screen_mut();
            for y2 in 0..=height {
                for x2 in 0..=width {
                    let cell = s.get_cell(x2, y2 as i64);
                    match cell {
                        Some(c) => {
                            let attrs = c.attrs();
                            block.add_change(Change::CursorPosition {
                                x: Position::Absolute(x2),
                                y: Position::Absolute(y2),
                            });
                            block.add_changes(vec![
                                Change::Attribute(termwiz::cell::AttributeChange::Foreground(
                                    attrs.foreground(),
                                )),
                                Change::Attribute(termwiz::cell::AttributeChange::Background(
                                    attrs.background(),
                                )),
                            ]);
                            block.add_change(c.str());
                        }
                        None => (),
                    }
                }
            }

            let cursor = term.cursor_pos();
            block.add_change(Change::CursorPosition {
                x: Position::Absolute(cursor.x),
                y: Position::Absolute(cursor.y as usize),
            });

            pty_screen_tx
                .send(TattoySurface {
                    kind: SurfaceType::PTYSurface,
                    surface: block,
                })
                .unwrap();
        }
    });

    let bg_output_worker = std::thread::spawn(move || {
        let target_frame_rate = 30;
        let target_frame_rate_micro = std::time::Duration::from_micros(1000000 / target_frame_rate);

        let mut x1 = rand::thread_rng().gen_range(1..width);
        let mut y1 = rand::thread_rng().gen_range(1..height);
        let mut thing = (x1, y1);
        loop {
            let frame_time = std::time::Instant::now();
            let mut block = Surface::new(width, height);

            x1 = x1 + rand::thread_rng().gen_range(0..=2) - 1;
            x1 = x1.clamp(1, width);

            y1 = y1 + rand::thread_rng().gen_range(0..=2) - 1;
            y1 = y1.clamp(1, height);

            block.add_change(Change::CursorPosition {
                x: Position::Absolute(x1),
                y: Position::Absolute(y1),
            });
            block.add_changes(vec![Change::Attribute(
                termwiz::cell::AttributeChange::Foreground(
                    termwiz::color::ColorAttribute::TrueColorWithDefaultFallback(SrgbaTuple(
                        rand::thread_rng().gen_range(0.0..1.0),
                        rand::thread_rng().gen_range(0.0..1.0),
                        rand::thread_rng().gen_range(0.0..1.0),
                        1.0,
                    )),
                ),
            )]);
            block.add_change("▄");

            bg_screen_tx
                .send(TattoySurface {
                    kind: SurfaceType::BGSurface,
                    surface: block,
                })
                .unwrap();

            if let Some(i) = target_frame_rate_micro.checked_sub(frame_time.elapsed()) {
                std::thread::sleep(i);
            }
        }
    });

    let render_worker = std::thread::spawn(move || {
        // let mut parser = Parser::new();

        let mut background = Surface::new(width, height);
        let mut pty = Surface::new(width, height);
        let mut frame = Surface::new(width, height);
        loop {
            let update = screen_rx.blocking_recv().unwrap();
            match update.kind {
                SurfaceType::BGSurface => background = update.surface,
                SurfaceType::PTYSurface => pty = update.surface,
            }

            frame.draw_from_screen(&background, 0, 0);
            let cells = pty.screen_cells();
            for (y, line) in cells.iter().enumerate() {
                for (x, cell) in line.iter().enumerate() {
                    let attrs = cell.attrs();
                    frame.add_change(Change::CursorPosition {
                        x: Position::Absolute(x),
                        y: Position::Absolute(y),
                    });
                    frame.add_changes(vec![
                        Change::Attribute(termwiz::cell::AttributeChange::Foreground(
                            attrs.foreground(),
                        )),
                        Change::Attribute(termwiz::cell::AttributeChange::Background(
                            attrs.background(),
                        )),
                    ]);

                    let character = cell.str();
                    if character != " " {
                        frame.add_change(character);
                    }
                }
            }

            let minimum_changes = buf.diff_screens(&frame);
            buf.add_changes(minimum_changes);

            let (x, y) = pty.cursor_position();
            buf.add_change(Change::CursorPosition {
                x: Position::Absolute(x),
                y: Position::Absolute(y),
            });

            buf.flush().unwrap();
        }
    });

    let pty = PTY::new(height as u16, width as u16, shell.to_owned(), pty_output_tx);
    if let Err(err) = pty.run() {
        tracing::error!("PTY error: {err}");
        std::process::exit(1);
    };
    Ok(())
}
