use crate::terminal::screen::Screen;

enum ParserState {
    Ground,
    Escape,
    Csi(Vec<u8>),
}

pub struct AnsiParser {
    state: ParserState,
}

impl AnsiParser {
    pub fn new() -> Self {
        Self {
            state: ParserState::Ground,
        }
    }

    pub fn feed(&mut self, screen: &mut Screen, bytes: &[u8]) {
        for &byte in bytes {
            let state = std::mem::replace(&mut self.state, ParserState::Ground);

            self.state = match state {
                ParserState::Ground => self.process_ground(screen, byte),
                ParserState::Escape => self.process_escape(byte),
                ParserState::Csi(mut params) => self.process_csi(screen, byte, &mut params),
            };
        }
    }

    fn process_ground(&self, screen: &mut Screen, byte: u8) -> ParserState {
        match byte {
            0x1B => ParserState::Escape,
            b'\n' => {
                screen.write_char('\n');
                ParserState::Ground
            }
            b'\r' => {
                screen.write_char('\r');
                ParserState::Ground
            }
            b'\t' => {
                screen.write_char('\t');
                ParserState::Ground
            }
            0x08 | 0x7F => {
                screen.move_cursor_relative(0, -1);
                ParserState::Ground
            }
            0x20..=0x7E => {
                screen.write_char(byte as char);
                ParserState::Ground
            }
            _ => ParserState::Ground,
        }
    }

    fn process_escape(&self, byte: u8) -> ParserState {
        match byte {
            b'[' => ParserState::Csi(Vec::new()),
            _ => ParserState::Ground,
        }
    }

    fn process_csi(&self, screen: &mut Screen, byte: u8, params: &mut Vec<u8>) -> ParserState {
        match byte {
            0x20..=0x3F => {
                params.push(byte);
                if params.len() > 128 {
                    return ParserState::Ground;
                }
                ParserState::Csi(std::mem::take(params))
            }
            0x40..=0x7E => {
                self.execute_csi(screen, params, byte);
                ParserState::Ground
            }
            _ => ParserState::Ground,
        }
    }

    fn execute_csi(&self, screen: &mut Screen, params: &[u8], command: u8) {
        let params_str = String::from_utf8_lossy(params);
        let is_private = params_str.starts_with('?');
        let clean_params = if is_private {
            &params_str[1..]
        } else {
            &params_str
        };

        let values: Vec<usize> = clean_params
            .split(';')
            .map(|value| value.parse::<usize>().unwrap_or(0))
            .collect();

        let first = values.first().copied().unwrap_or(0);

        if is_private {
            return; // Safely ignore unsupported private mode sequences
        }

        match command {
            b'A' => screen.move_cursor_relative(-(first.max(1) as isize), 0),
            b'B' => screen.move_cursor_relative(first.max(1) as isize, 0),
            b'C' => screen.move_cursor_relative(0, first.max(1) as isize),
            b'D' => screen.move_cursor_relative(0, -(first.max(1) as isize)),
            b'H' | b'f' => {
                let row = values.first().copied().unwrap_or(1).max(1);
                let col = values.get(1).copied().unwrap_or(1).max(1);
                screen.move_cursor(row - 1, col - 1);
            }
            b'G' => {
                let col = first.max(1);
                screen.move_cursor(screen.cursor_row(), col - 1);
            }
            b'J' => screen.clear_screen(first),
            b'K' => screen.clear_line(first),
            b'm' => {}
            b'P' => {
                let count = first.max(1);
                for _ in 0..count {
                    screen.delete_char();
                }
            }
            b'@' => {
                let count = first.max(1);
                for _ in 0..count {
                    screen.insert_char(' ');
                }
            }
            _ => {}
        }
    }
}

impl Default for AnsiParser {
    fn default() -> Self {
        Self::new()
    }
}
