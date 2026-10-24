use crate::terminal::screen::Screen;

enum ParserState {
    Ground,
    Escape,
    Csi(Vec<u8>),
    Osc { esc: bool },
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
                ParserState::Osc { esc } => self.process_osc(byte, esc),
            };
        }
    }

    fn process_ground(&self, screen: &mut Screen, byte: u8) -> ParserState {
        match byte {
            0x1B => return ParserState::Escape,
            b'\n' => screen.write_char('\n'),
            b'\r' => screen.write_char('\r'),
            b'\t' => screen.write_char('\t'),
            0x08 => screen.backspace(),
            0x20..=0x7E => screen.write_char(byte as char),
            _ => {} // includes 0x7F (DEL), BEL, non-ASCII
        }
        ParserState::Ground
    }

    fn process_escape(&self, byte: u8) -> ParserState {
        match byte {
            b'[' => ParserState::Csi(Vec::new()),
            b']' => ParserState::Osc { esc: false },
            _ => ParserState::Ground,
        }
    }

    // Swallow OSC (window title etc.) until BEL or ESC \
    fn process_osc(&self, byte: u8, prev_esc: bool) -> ParserState {
        if byte == 0x07 || (prev_esc && byte == b'\\') {
            ParserState::Ground
        } else {
            ParserState::Osc { esc: byte == 0x1B }
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

        // ? < = >  prefixes are private/extended modes we don't support.
        if params_str.starts_with(['?', '<', '=', '>']) {
            return;
        }

        // Treat ':' like ';' (e.g. 38:2::r:g:b).
        let values: Vec<usize> = params_str
            .split([';', ':'])
            .map(|v| v.parse::<usize>().unwrap_or(0))
            .collect();

        let first = values.first().copied().unwrap_or(0);

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
            b'G' => screen.set_cursor_col(first.max(1) - 1),
            b'J' => screen.clear_screen(first),
            b'K' => screen.clear_line(first),
            b'm' => screen.apply_sgr(&values),
            b'P' => {
                for _ in 0..first.max(1) {
                    screen.delete_char();
                }
            }
            b'@' => {
                for _ in 0..first.max(1) {
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
