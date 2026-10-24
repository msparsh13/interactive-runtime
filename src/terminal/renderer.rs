use crate::{
    error::Result,
    terminal::screen::{Screen, Style},
};

use crossterm::{
    cursor, execute, queue,
    style::{
        Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
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

    // mouse: press/release + drag, SGR encoding
    stdout().write_all(b"\x1b[?1002h\x1b[?1006h")?;
    stdout().flush()?;
    Ok(())
}

pub fn exit() -> Result<()> {
    stdout().write_all(b"\x1b[?1006l\x1b[?1002l")?;

    execute!(
        stdout(),
        SetAttribute(Attribute::Reset),
        ResetColor,
        cursor::Show,
        Clear(ClearType::All),
        cursor::MoveTo(0, 0),
        terminal::LeaveAlternateScreen,
    )?;

    terminal::disable_raw_mode()?;
    stdout().flush()?;
    Ok(())
}

fn apply_style(out: &mut impl Write, s: &Style) -> Result<()> {
    queue!(out, SetAttribute(Attribute::Reset), ResetColor)?;
    if let Some(c) = s.fg {
        queue!(out, SetForegroundColor(c))?;
    }
    if let Some(c) = s.bg {
        queue!(out, SetBackgroundColor(c))?;
    }
    if s.bold {
        queue!(out, SetAttribute(Attribute::Bold))?;
    }
    if s.dim {
        queue!(out, SetAttribute(Attribute::Dim))?;
    }
    if s.italic {
        queue!(out, SetAttribute(Attribute::Italic))?;
    }
    if s.underline {
        queue!(out, SetAttribute(Attribute::Underlined))?;
    }
    if s.reverse {
        queue!(out, SetAttribute(Attribute::Reverse))?;
    }
    Ok(())
}

// Draw only inside the given pane rectangle.
// This function deliberately does not show or move the cursor.
pub fn render_region(screen: &Screen, rect: Rect, clear: bool) -> Result<()> {
    let mut out = stdout();
    let (terminal_width, terminal_height) = terminal::size()?;

    if rect.x >= terminal_width || rect.y >= terminal_height {
        return Ok(());
    }

    let width = rect.width.min(screen.cols()).min(terminal_width - rect.x);
    let height = rect.height.min(screen.rows()).min(terminal_height - rect.y);

    if width == 0 || height == 0 {
        return Ok(());
    }

    queue!(out, SetAttribute(Attribute::Reset), ResetColor)?;

    if clear {
        let spaces = " ".repeat(width as usize);
        for row in 0..height {
            queue!(out, cursor::MoveTo(rect.x, rect.y + row), Print(&spaces))?;
        }
    }

    let viewport_start = screen.viewport_start();
    let mut last = Style::default();

    for visible_row in 0..height as usize {
        let buffer_row = viewport_start + visible_row;
        queue!(out, cursor::MoveTo(rect.x, rect.y + visible_row as u16))?;

        for col in 0..width as usize {
            let (ch, st) = match screen.cell(buffer_row, col) {
                Some(c) => (c.character, c.style),
                None => (' ', Style::default()),
            };
            if st != last {
                apply_style(&mut out, &st)?;
                last = st;
            }
            queue!(out, Print(ch))?;
        }
    }

    queue!(out, SetAttribute(Attribute::Reset), ResetColor)?;
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
        cursor::MoveTo(rect.x + col as u16, rect.y + row as u16),
        cursor::Show,
    )?;

    out.flush()?;
    Ok(())
}

pub fn hide_cursor() -> Result<()> {
    execute!(stdout(), cursor::Hide)?;
    Ok(())
}

pub fn clear_all() -> Result<()> {
    execute!(
        stdout(),
        SetAttribute(Attribute::Reset),
        ResetColor,
        Clear(ClearType::All)
    )?;
    Ok(())
}

/// Scrollbar in column `x`. Thumb = visible part of the buffer.
pub fn render_scrollbar(screen: &Screen, x: u16, y: u16, height: u16) -> Result<()> {
    let mut out = stdout();
    let (tw, th) = terminal::size()?;
    if x >= tw || y >= th {
        return Ok(());
    }
    let h = height.min(th - y) as usize;
    if h == 0 {
        return Ok(());
    }

    let total = screen.buffer_rows().max(1);
    let visible = (screen.rows() as usize).min(total);
    let max_start = total - visible;
    let thumb_len = (visible * h / total).clamp(1, h);
    let track = h - thumb_len;
    let thumb_top = if max_start == 0 {
        0
    } else {
        (screen.viewport_start() * track + max_start / 2) / max_start
    };

    queue!(out, SetAttribute(Attribute::Reset), ResetColor)?;
    for row in 0..h {
        let thumb = row >= thumb_top && row < thumb_top + thumb_len;
        queue!(
            out,
            cursor::MoveTo(x, y + row as u16),
            Print(if thumb { '█' } else { '│' })
        )?;
    }
    out.flush()?;
    Ok(())
}

/// Horizontal divider for stacked splits.
pub fn render_hline(x: u16, y: u16, width: u16) -> Result<()> {
    let mut out = stdout();
    let (tw, th) = terminal::size()?;
    if x >= tw || y >= th {
        return Ok(());
    }
    let width = width.min(tw - x) as usize;
    queue!(
        out,
        SetForegroundColor(Color::DarkGrey),
        cursor::MoveTo(x, y),
        Print("─".repeat(width)),
        ResetColor
    )?;
    out.flush()?;
    Ok(())
}

/// Border + title for a floating window. `rect` is the outer rectangle.
pub fn render_box(rect: Rect, title: &str, focused: bool) -> Result<()> {
    let mut out = stdout();
    let (tw, th) = terminal::size()?;
    if rect.width < 3
        || rect.height < 2
        || rect.x.saturating_add(rect.width) > tw
        || rect.y.saturating_add(rect.height) > th
    {
        return Ok(());
    }

    let color = if focused {
        Color::Cyan
    } else {
        Color::DarkGrey
    };
    let inner = rect.width as usize - 2;
    let label: String = format!(" {} ", title).chars().take(inner).collect();
    let top = format!("┌{}{}┐", label, "─".repeat(inner - label.chars().count()));

    queue!(
        out,
        SetAttribute(Attribute::Reset),
        SetForegroundColor(color),
        cursor::MoveTo(rect.x, rect.y),
        Print(top)
    )?;

    for row in 1..rect.height - 1 {
        queue!(
            out,
            cursor::MoveTo(rect.x, rect.y + row),
            Print('│'),
            cursor::MoveTo(rect.x + rect.width - 1, rect.y + row),
            Print('│')
        )?;
    }

    queue!(
        out,
        cursor::MoveTo(rect.x, rect.y + rect.height - 1),
        Print(format!("└{}◢", "─".repeat(inner))),
        ResetColor
    )?;
    out.flush()?;
    Ok(())
}
