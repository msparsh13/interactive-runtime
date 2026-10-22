use crate::{
    backend::pty::Pty,
    error::Result,
    terminal::{ansi::AnsiParser, screen::Screen},
};

pub struct Pane {
    pty: Pty,
    screen: Screen,
    parser: AnsiParser,
}

impl Pane {
    pub fn new(rows: u16, cols: u16) -> Result<Self> {
        let mut pty = Pty::spawn_shell(rows, cols)?;
        pty.resize(rows, cols)?;

        Ok(Self {
            pty,
            screen: Screen::with_size(rows, cols),
            parser: AnsiParser::new(),
        })
    }

    pub fn pty(&self) -> &Pty {
        &self.pty
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    pub fn write_input(&mut self, input: &[u8]) -> Result<usize> {
        self.pty.write(input)
    }

    pub fn process_output(&mut self, output: &[u8]) {
        self.parser.feed(&mut self.screen, output);
    }

    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        self.screen.resize_to(rows, cols);
        self.pty.resize(rows, cols)?;
        Ok(())
    }

    pub fn scroll_view_up(&mut self, lines: usize) {
        self.screen.scroll_viewport(lines as isize);
    }

    pub fn scroll_view_down(&mut self, lines: usize) {
        self.screen.scroll_viewport(-(lines as isize));
    }

    pub fn scroll_page_up(&mut self) {
        let half = (self.screen.rows() as usize / 2).max(1);
        self.scroll_view_up(half);
    }

    pub fn scroll_page_down(&mut self) {
        let half = (self.screen.rows() as usize / 2).max(1);
        self.scroll_view_down(half);
    }
}
