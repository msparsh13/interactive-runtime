use crate::error::Result;
use crossterm::terminal;

//TOD CHECK cursor position handle at output
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub character: char,
}

impl Default for Cell {
    fn default() -> Self {
        Self { character: ' ' }
    }
}

#[derive(Clone, Debug)]
pub struct Row {
    pub cells: Vec<Cell>,
    pub wrap: bool,
}

impl Row {
    pub fn new(cols: usize) -> Self {
        Self {
            cells: vec![Cell::default(); cols],
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

    pub fn cell(&self, row: usize, col: usize) -> Option<&Cell> {
        self.buffer.get(row).and_then(|r| r.cells.get(col))
    }

    pub fn cell_mut(&mut self, row: usize, col: usize) -> Option<&mut Cell> {
        self.buffer.get_mut(row).and_then(|r| r.cells.get_mut(col))
    }

    pub fn move_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row.min(self.buffer.len().saturating_sub(1));
        self.cursor_col = col.min((self.cols as usize).saturating_sub(1));
    }

    pub fn move_cursor_relative(&mut self, row_delta: isize, col_delta: isize) {
        let new_row = (self.cursor_row as isize + row_delta)
            .clamp(0, self.buffer.len().saturating_sub(1) as isize) as usize;

        let new_col = (self.cursor_col as isize + col_delta)
            .clamp(0, (self.cols as usize).saturating_sub(1) as isize)
            as usize;

        self.cursor_row = new_row;
        self.cursor_col = new_col;
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

        if c == '\t' {
            let spaces = 8 - (self.cursor_col % 8);
            for _ in 0..spaces {
                self.write_char(' ');
            }
            return;
        }

        // Wrap before writing the next character if the previous
        // character filled the final column.
        if self.cursor_col >= self.cols as usize {
            self.buffer[self.cursor_row].wrap = true;
            self.cursor_col = 0;
            self.line_feed();
        }

        if let Some(cell) = self.cell_mut(self.cursor_row, self.cursor_col) {
            cell.character = c;
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
        // Preserve existing rows for scrollback.
        self.buffer.insert(0, Row::new(self.cols as usize));
        self.cursor_row += 1;
    }

    pub fn clear_screen(&mut self, mode: usize) {
        match mode {
            0 => {
                for col in self.cursor_col..self.cols as usize {
                    if let Some(cell) = self.cell_mut(self.cursor_row, col) {
                        cell.character = ' ';
                    }
                }
                for row in self.cursor_row + 1..self.rows as usize {
                    self.buffer[row] = Row::new(self.cols as usize);
                }
            }
            1 => {
                for row in 0..self.cursor_row {
                    self.buffer[row] = Row::new(self.cols as usize);
                }
                for col in 0..=self.cursor_col {
                    if let Some(cell) = self.cell_mut(self.cursor_row, col) {
                        cell.character = ' ';
                    }
                }
            }
            2 | 3 => {
                for row in &mut self.buffer {
                    *row = Row::new(self.cols as usize);
                }
                self.cursor_row = 0;
                self.cursor_col = 0;
            }
            _ => {}
        }
    }

    pub fn clear_line(&mut self, mode: usize) {
        let cols = self.cols as usize;
        match mode {
            0 => {
                for col in self.cursor_col..cols {
                    if let Some(cell) = self.cell_mut(self.cursor_row, col) {
                        cell.character = ' ';
                    }
                }
            }
            1 => {
                for col in 0..=self.cursor_col.min(cols.saturating_sub(1)) {
                    if let Some(cell) = self.cell_mut(self.cursor_row, col) {
                        cell.character = ' ';
                    }
                }
            }
            2 => {
                self.buffer[self.cursor_row] = Row::new(cols);
            }
            _ => {}
        }
    }

    pub fn delete_char(&mut self) {
        let cols = self.cols as usize;
        let row = self.cursor_row;
        let col = self.cursor_col;
        if col < cols {
            let r = &mut self.buffer[row];
            r.cells.remove(col);
            r.cells.push(Cell::default());
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let cols = self.cols as usize;
        let row = self.cursor_row;
        let col = self.cursor_col;
        if col < cols {
            let r = &mut self.buffer[row];
            r.cells.insert(col, Cell { character: c });
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
                // Keep meaningful trailing spaces; only trim padding that
                // is indistinguishable from unused screen cells.
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

        // Rebuild visual rows at the new width.
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

        // Keep the top of the buffer anchored. If the content exceeds the
        // available height, retain the top rows rather than draining them.
        while new_buffer.len() < new_rows as usize {
            new_buffer.push(Row::new(new_cols as usize));
        }

        self.rows = new_rows;
        self.cols = new_cols;
        self.buffer = new_buffer;

        self.cursor_row = new_cursor_row.min(self.buffer.len().saturating_sub(1));
        self.cursor_col = new_cursor_col.min(new_cols as usize - 1);
    }

    pub fn buffer_rows(&self) -> usize {
        self.buffer.len()
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
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
}
