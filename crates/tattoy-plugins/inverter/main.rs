//! Just a simple plugin for end to end testing and as a basic example for plugin authors.
//!
//! All it does is invert the user's terminal left-to-right, top-to-bottom.

#![allow(clippy::restriction)]

/// Entrypoint
fn main() {
    let lines = std::io::stdin().lines();

    for line in lines {
        let message: tattoy_protocol::PluginInputMessages =
            serde_json::from_str(line.unwrap().as_str()).unwrap();

        match message {
            tattoy_protocol::PluginInputMessages::PTYUpdate {
                size,
                cells,
                cursor: _,
            } => {
                if size.0 == 0 || size.1 == 0 {
                    continue;
                }

                let tty_width = size.0;
                let tty_height = size.1;

                let mut outgoing_cells = Vec::<tattoy_protocol::Cell>::new();
                for incoming_cell in cells {
                    let outgoing_cell = tattoy_protocol::Cell::builder()
                        .character(incoming_cell.character)
                        .coordinates((
                            u32::from(tty_width) - incoming_cell.coordinates.0 - 1,
                            u32::from(tty_height) - incoming_cell.coordinates.1 - 1,
                        ))
                        .maybe_bg(incoming_cell.bg)
                        .maybe_fg(incoming_cell.fg)
                        .build();
                    outgoing_cells.push(outgoing_cell);
                }

                let output = tattoy_protocol::PluginOutputMessages::OutputCells(outgoing_cells);
                print!("{}", serde_json::to_string(&output).unwrap());
            }
            _ => todo!(),
        }
    }
}
