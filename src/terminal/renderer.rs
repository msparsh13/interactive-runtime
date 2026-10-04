use crate::error::Result;

use crossterm::{
    cursor, execute,
    terminal::{self, ClearType},
};

use std::io::{Write, stdout};

pub fn enter() -> Result<()> {
    terminal::enable_raw_mode()?;

    execute!(
        stdout(),
        terminal::EnterAlternateScreen,
        cursor::Hide,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0),
    )?;

    stdout().flush()?;

    Ok(())
}

pub fn exit() -> Result<()> {
    terminal::disable_raw_mode()?;

    execute!(stdout(), cursor::Show, terminal::LeaveAlternateScreen,)?;

    stdout().flush()?;

    Ok(())
}

pub fn write(data: &[u8]) -> Result<()> {
    std::io::stdout().write_all(data)?;
    std::io::stdout().flush()?;

    Ok(())
}
