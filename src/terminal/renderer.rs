use crate::{error::Result, terminal::screen::Screen};

use crossterm::{
    cursor, execute, queue,
    style::Print,
    terminal::{self, Clear, ClearType},
};

use std::io::{Write, stdout};

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

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
    execute!(
        stdout(),
        cursor::Show,
        Clear(ClearType::All),
        cursor::MoveTo(0, 0),
        terminal::LeaveAlternateScreen,
    )?;

    terminal::disable_raw_mode()?;
    stdout().flush()?;

    Ok(())
}

// Draw one screen across the full terminal.
pub fn render(screen: &Screen) -> Result<()> {
    let (width, height) = terminal::size()?;

    queue!(stdout(), cursor::Hide, Clear(ClearType::All),)?;

    render_region(
        screen,
        Rect {
            x: 0,
            y: 0,
            width,
            height,
        },
        false,
    )?;

    // Draw separator AFTER the screen content.
    render_vertical_separator(screen, width / 2, 0, height)?;

    // Draw cursor last.
    render_cursor(
        screen,
        Rect {
            x: 0,
            y: 0,
            width,
            height,
        },
    )?;

    Ok(())
}

// Draw only inside the given pane rectangle.
// This function deliberately does not show or move the cursor.
pub fn render_region(screen: &Screen, rect: Rect, clear: bool) -> Result<()> {
    let mut out = stdout();
    let (terminal_width, terminal_height) = terminal::size()?;

    // Ignore rectangles outside the terminal.
    if rect.x >= terminal_width || rect.y >= terminal_height {
        return Ok(());
    }

    // Clip the pane to both its own dimensions and terminal bounds.
    let width = rect.width.min(screen.cols()).min(terminal_width - rect.x);

    let height = rect.height.min(screen.rows()).min(terminal_height - rect.y);

    if width == 0 || height == 0 {
        return Ok(());
    }

    // Clear only this pane's area, never the rest of the terminal.
    if clear {
        let spaces = " ".repeat(width as usize);

        for row in 0..height {
            queue!(out, cursor::MoveTo(rect.x, rect.y + row), Print(&spaces),)?;
        }
    }

    let viewport_start = screen.viewport_start();

    for visible_row in 0..height as usize {
        let buffer_row = viewport_start + visible_row;

        queue!(out, cursor::MoveTo(rect.x, rect.y + visible_row as u16),)?;

        for col in 0..width as usize {
            let character = screen
                .cell(buffer_row, col)
                .map(|cell| cell.character)
                .unwrap_or(' ');

            queue!(out, Print(character))?;
        }
    }

    out.flush()?;
    Ok(())
}

// Call this ONCE after drawing all panes, for the focused pane only.
pub fn render_cursor(screen: &Screen, rect: Rect) -> Result<()> {
    let mut out = stdout();
    let (terminal_width, terminal_height) = terminal::size()?;

    if screen.scroll_offset() != 0
        || rect.width == 0
        || rect.height == 0
        || rect.x >= terminal_width
        || rect.y >= terminal_height
    {
        queue!(out, cursor::Hide)?;
        out.flush()?;
        return Ok(());
    }

    let width = rect.width.min(screen.cols()).min(terminal_width - rect.x);
    let height = rect.height.min(screen.rows()).min(terminal_height - rect.y);

    if width == 0 || height == 0 {
        queue!(out, cursor::Hide)?;
        out.flush()?;
        return Ok(());
    }

    let row = screen.cursor_row().saturating_sub(screen.viewport_start());
    let col = screen.cursor_col();

    if row >= height as usize || col >= width as usize {
        queue!(out, cursor::Hide)?;
        out.flush()?;
        return Ok(());
    }

    queue!(
        out,
        cursor::MoveTo(rect.x + col as u16, rect.y + row as u16,),
        cursor::Show,
    )?;

    out.flush()?;
    Ok(())
}

pub fn hide_cursor() -> Result<()> {
    execute!(stdout(), cursor::Hide)?;
    Ok(())
}

pub fn render_vertical_separator(screen: &Screen, x: u16, y: u16, height: u16) -> Result<()> {
    let mut out = stdout();
    let (terminal_width, terminal_height) = terminal::size()?;

    if x >= terminal_width || y >= terminal_height {
        return Ok(());
    }

    let height = height
        .min(screen.rows().saturating_sub(y))
        .min(terminal_height - y);

    for row in 0..height {
        queue!(out, cursor::MoveTo(x - 2, y + row), Print("│"),)?;
    }

    out.flush()?;
    Ok(())
}
// Draw a horizontal separator line.
pub fn render_horizontal_separator(screen: &Screen, x: u16, y: u16, width: u16) -> Result<()> {
    let mut out = stdout();
    let (terminal_width, terminal_height) = terminal::size()?;

    if x >= screen.cols() || x >= terminal_width || y >= screen.rows() || y >= terminal_height {
        return Ok(());
    }

    let width = width.min(screen.cols() - x).min(terminal_width - x);

    for col in 0..width {
        queue!(out, cursor::MoveTo(x + col, y), Print("─"),)?;
    }

    out.flush()?;
    Ok(())
}
