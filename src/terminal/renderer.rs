use crate::{error::Result, terminal::screen::Screen};

use crossterm::{
    cursor, execute, queue,
    style::Print,
    terminal::{self, Clear, ClearType},
};

use std::io::{Write, stdout};

pub fn enter() -> Result<()> {
    terminal::enable_raw_mode()?;

    execute!(
        stdout(),
        terminal::EnterAlternateScreen,
        cursor::Hide,
        Clear(ClearType::All),
        cursor::MoveTo(0, 0),
    )?;

    stdout().flush()?;

    Ok(())
}

pub fn exit() -> Result<()> {
    terminal::disable_raw_mode()?;
    stdout().flush()?;

    Ok(())
}

pub fn render(screen: &Screen) -> Result<()> {
    let mut out = stdout();

    let visible_rows = screen.rows() as usize;
    let visible_cols = screen.cols() as usize;
    let viewport_start = screen.viewport_start();

    queue!(out, cursor::Hide, Clear(ClearType::All),)?;

    for visible_row in 0..visible_rows {
        let buffer_row = viewport_start + visible_row;

        queue!(out, cursor::MoveTo(0, visible_row as u16),)?;

        for col in 0..visible_cols {
            if let Some(cell) = screen.cell(buffer_row, col) {
                queue!(out, Print(cell.character))?;
            }
        }
    }

    // Show the cursor only when following live output.
    if screen.scroll_offset() == 0 {
        let cursor_screen_row = screen
            .cursor_row()
            .saturating_sub(viewport_start)
            .min(visible_rows.saturating_sub(1));

        queue!(
            out,
            cursor::MoveTo(
                screen.cursor_col().min(visible_cols.saturating_sub(1)) as u16,
                cursor_screen_row as u16,
            ),
            cursor::Show,
        )?;
    }

    out.flush()?;

    Ok(())
}
