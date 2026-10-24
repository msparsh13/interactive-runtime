use crate::error::Result;
use crossterm::terminal;

use crossterm::style::Color;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub character: char,
    pub style: Style,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            character: ' ',
            style: Style::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Row {
    pub cells: Vec<Cell>,
    pub wrap: bool,
}

impl Row {
    pub fn new(cols: usize) -> Self {
        Self::filled(cols, Cell::default())
    }

    pub fn filled(cols: usize, cell: Cell) -> Self {
        Self {
            cells: vec![cell; cols],
            wrap: false,
        }
    }
}

pub struct Screen {
    rows: u16,
    cols: u16,
    cursor_row: usize,
    cursor_col: usize,
    buffer: Vec<Row>,
    scroll_offset: usize,
    style: Style,
}

impl Screen {
    pub fn new() -> Result<Self> {
        let (rows, cols) = Self::terminal_size()?;
        Ok(Self::with_size(rows, cols))
    }

    pub fn with_size(rows: u16, cols: u16) -> Self {
        let rows = rows.max(1);
        let cols = cols.max(1);
        let mut buffer = Vec::with_capacity(rows as usize);

        for _ in 0..rows {
            buffer.push(Row::new(cols as usize));
        }
        Self {
            rows,
            cols,
            cursor_row: 0,
            cursor_col: 0,
            buffer,
            scroll_offset: 0,
            style: Style::default(),
        }
    }

    pub fn terminal_size() -> Result<(u16, u16)> {
        let (cols, rows) = terminal::size()?;
        Ok((rows.max(1), cols.max(1)))
    }

    pub fn rows(&self) -> u16 {
        self.rows
    }

    pub fn cols(&self) -> u16 {
        self.cols
    }

    pub fn cursor_row(&self) -> usize {
        self.cursor_row
    }

    pub fn cursor_col(&self) -> usize {
        self.cursor_col
    }

    /// First buffer row of the live (bottom) screen.
    fn origin(&self) -> usize {
        self.buffer.len().saturating_sub(self.rows as usize)
    }

    pub fn cell(&self, row: usize, col: usize) -> Option<&Cell> {
        self.buffer.get(row).and_then(|r| r.cells.get(col))
    }

    pub fn cell_mut(&mut self, row: usize, col: usize) -> Option<&mut Cell> {
        self.buffer.get_mut(row).and_then(|r| r.cells.get_mut(col))
    }

    /// CUP: row/col are screen-relative.
    pub fn move_cursor(&mut self, row: usize, col: usize) {
        let last = self.buffer.len().saturating_sub(1);
        self.cursor_row = (self.origin() + row).min(last);
        self.cursor_col = col.min((self.cols as usize).saturating_sub(1));
    }

    pub fn move_cursor_relative(&mut self, row_delta: isize, col_delta: isize) {
        let min_row = self.origin() as isize;
        let max_row = self.buffer.len().saturating_sub(1) as isize;
        self.cursor_row = (self.cursor_row as isize + row_delta).clamp(min_row, max_row) as usize;
        self.cursor_col = (self.cursor_col as isize + col_delta)
            .clamp(0, (self.cols as usize).saturating_sub(1) as isize)
            as usize;
    }

    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            // also covers the "pending wrap" state (cursor_col == cols)
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 && self.buffer[self.cursor_row - 1].wrap {
            // reverse-wrap onto the previous wrapped row
            self.cursor_row -= 1;
            self.cursor_col = self.cols as usize - 1;
        }
    }

    pub fn write_char(&mut self, c: char) {
        if c == '\r' {
            self.cursor_col = 0;
            return;
        }

        if c == '\n' {
            self.line_feed();
            return;
        }

        if c == '\x08' {
            self.backspace();
            return;
        }

        if c == '\t' {
            let spaces = 8 - (self.cursor_col % 8);
            for _ in 0..spaces {
                self.write_char(' ');
            }
            return;
        }

        if self.cursor_col >= self.cols as usize {
            self.buffer[self.cursor_row].wrap = true;
            self.cursor_col = 0;
            self.line_feed();
        }
        let style = self.style;
        if let Some(cell) = self.cell_mut(self.cursor_row, self.cursor_col) {
            *cell = Cell {
                character: c,
                style,
            };
        }

        self.cursor_col += 1;
    }

    pub fn line_feed(&mut self) {
        if self.cursor_row + 1 >= self.buffer.len() {
            self.buffer.push(Row::new(self.cols as usize));
        }

        self.cursor_row += 1;
    }

    pub fn scroll_up(&mut self) {
        self.buffer.insert(0, Row::new(self.cols as usize));
        self.cursor_row += 1;
    }

    pub fn clear_screen(&mut self, mode: usize) {
        let cols = self.cols as usize;
        let origin = self.origin();
        let end = self.buffer.len();
        let blank = self.blank();
        match mode {
            0 => {
                self.clear_line(0);
                for r in self.cursor_row + 1..end {
                    self.buffer[r] = Row::filled(cols, blank);
                }
            }
            1 => {
                for r in origin..self.cursor_row {
                    self.buffer[r] = Row::filled(cols, blank);
                }
                self.clear_line(1);
            }
            2 => {
                for r in origin..end {
                    self.buffer[r] = Row::filled(cols, blank);
                }
            }
            3 => {
                self.buffer.drain(..origin);
                self.cursor_row = self.cursor_row.saturating_sub(origin);
                self.scroll_offset = 0;
            }
            _ => {}
        }
    }

    pub fn clear_line(&mut self, mode: usize) {
        let cols = self.cols as usize;
        let blank = self.blank();
        match mode {
            0 => {
                for col in self.cursor_col..cols {
                    if let Some(cell) = self.cell_mut(self.cursor_row, col) {
                        *cell = blank;
                    }
                }
            }
            1 => {
                for col in 0..=self.cursor_col.min(cols.saturating_sub(1)) {
                    if let Some(cell) = self.cell_mut(self.cursor_row, col) {
                        *cell = blank;
                    }
                }
            }
            2 => {
                self.buffer[self.cursor_row] = Row::filled(cols, blank);
            }
            _ => {}
        }
    }

    pub fn delete_char(&mut self) {
        let cols = self.cols as usize;
        let (row, col) = (self.cursor_row, self.cursor_col);
        let blank = self.blank();
        if col < cols {
            let r = &mut self.buffer[row];
            r.cells.remove(col);
            r.cells.push(blank);
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let cols = self.cols as usize;
        let (row, col) = (self.cursor_row, self.cursor_col);
        let style = self.style;
        if col < cols {
            let r = &mut self.buffer[row];
            r.cells.insert(
                col,
                Cell {
                    character: c,
                    style,
                },
            );
            r.cells.pop();
        }
    }

    pub fn resize_to(&mut self, new_rows: u16, new_cols: u16) {
        let new_rows = new_rows.max(1);
        let new_cols = new_cols.max(1);

        if self.rows == new_rows && self.cols == new_cols {
            return;
        }

        // Save the cursor's logical line and character offset.
        let mut logical_lines: Vec<Vec<Cell>> = Vec::new();
        let mut current_line = Vec::new();
        let mut cursor_logical_line = 0usize;
        let mut cursor_offset = 0usize;
        let mut logical_line_index = 0usize;

        for (row_index, row) in self.buffer.iter().enumerate() {
            if row_index == self.cursor_row {
                cursor_logical_line = logical_line_index;
                cursor_offset = current_line.len() + self.cursor_col.min(row.cells.len());
            }

            current_line.extend(row.cells.iter().cloned());

            if !row.wrap {
                while current_line.last().is_some_and(|c| c.character == ' ') {
                    current_line.pop();
                }

                logical_lines.push(std::mem::take(&mut current_line));
                logical_line_index += 1;
            }
        }

        if !current_line.is_empty() {
            logical_lines.push(current_line);
        }

        if logical_lines.is_empty() {
            logical_lines.push(Vec::new());
        }

        let mut new_buffer = Vec::new();
        let mut new_cursor_row = 0usize;
        let mut new_cursor_col = 0usize;

        for (line_index, line) in logical_lines.iter().enumerate() {
            if line.is_empty() {
                if line_index == cursor_logical_line {
                    new_cursor_row = new_buffer.len();
                    new_cursor_col = 0;
                }

                new_buffer.push(Row::new(new_cols as usize));
                continue;
            }

            let width = new_cols as usize;
            let chunks: Vec<&[Cell]> = line.chunks(width).collect();

            for (chunk_index, chunk) in chunks.iter().enumerate() {
                let visual_row = new_buffer.len();
                let mut row = Row::new(width);

                for (col, cell) in chunk.iter().enumerate() {
                    row.cells[col] = cell.clone();
                }

                row.wrap = chunk_index + 1 < chunks.len();
                new_buffer.push(row);

                if line_index == cursor_logical_line {
                    let start = chunk_index * width;
                    let end = start + chunk.len();

                    if cursor_offset >= start
                        && (cursor_offset < end || chunk_index + 1 == chunks.len())
                    {
                        new_cursor_row = visual_row;
                        new_cursor_col = cursor_offset.saturating_sub(start).min(width - 1);
                    }
                }
            }
        }
        // Drop blank rows below the cursor so the live screen stays aligned with the top.
        while new_buffer.len() > new_rows as usize
            && new_buffer.len() - 1 > new_cursor_row
            && new_buffer
                .last()
                .is_some_and(|r| !r.wrap && r.cells.iter().all(|c| *c == Cell::default()))
        {
            new_buffer.pop();
        }

        while new_buffer.len() < new_rows as usize {
            new_buffer.push(Row::new(new_cols as usize));
        }

        self.rows = new_rows;
        self.cols = new_cols;
        self.buffer = new_buffer;

        self.cursor_row = new_cursor_row.min(self.buffer.len().saturating_sub(1));
        self.cursor_col = new_cursor_col.min(new_cols as usize - 1);

        // clamp here (this is where it belongs, not in Pane)
        self.scroll_offset = self
            .scroll_offset
            .min(self.buffer.len().saturating_sub(new_rows as usize));
    }

    pub fn buffer_rows(&self) -> usize {
        self.buffer.len()
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub fn reset_scroll(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn scroll_viewport(&mut self, delta: isize) {
        let max_offset = self.buffer.len().saturating_sub(self.rows as usize);

        self.scroll_offset =
            (self.scroll_offset as isize + delta).clamp(0, max_offset as isize) as usize;
    }

    pub fn viewport_start(&self) -> usize {
        self.buffer
            .len()
            .saturating_sub(self.rows as usize)
            .saturating_sub(self.scroll_offset)
    }

    fn blank(&self) -> Cell {
        Cell {
            character: ' ',
            style: Style {
                bg: self.style.bg,
                ..Style::default()
            },
        }
    }

    pub fn apply_sgr(&mut self, params: &[usize]) {
        if params.is_empty() {
            self.style = Style::default();
            return;
        }
        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => self.style = Style::default(),
                1 => self.style.bold = true,
                2 => self.style.dim = true,
                3 => self.style.italic = true,
                4 => self.style.underline = true,
                7 => self.style.reverse = true,
                22 => {
                    self.style.bold = false;
                    self.style.dim = false;
                }
                23 => self.style.italic = false,
                24 => self.style.underline = false,
                27 => self.style.reverse = false,
                n @ 30..=37 => self.style.fg = Some(Color::AnsiValue((n - 30) as u8)),
                39 => self.style.fg = None,
                n @ 40..=47 => self.style.bg = Some(Color::AnsiValue((n - 40) as u8)),
                49 => self.style.bg = None,
                n @ 90..=97 => self.style.fg = Some(Color::AnsiValue((n - 90 + 8) as u8)),
                n @ 100..=107 => self.style.bg = Some(Color::AnsiValue((n - 100 + 8) as u8)),
                n @ (38 | 48) => {
                    let color = match params.get(i + 1).copied() {
                        Some(5) => {
                            let c = params.get(i + 2).map(|&v| Color::AnsiValue(v as u8));
                            i += 2;
                            c
                        }
                        Some(2) => {
                            let c = match (params.get(i + 2), params.get(i + 3), params.get(i + 4))
                            {
                                (Some(&r), Some(&g), Some(&b)) => Some(Color::Rgb {
                                    r: r as u8,
                                    g: g as u8,
                                    b: b as u8,
                                }),
                                _ => None,
                            };
                            i += 4;
                            c
                        }
                        _ => None,
                    };
                    if let Some(c) = color {
                        if n == 38 {
                            self.style.fg = Some(c)
                        } else {
                            self.style.bg = Some(c)
                        }
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    pub fn set_cursor_col(&mut self, col: usize) {
        self.cursor_col = col.min((self.cols as usize).saturating_sub(1));
    }
}
