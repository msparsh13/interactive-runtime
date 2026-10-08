use std::io;
use std::os::fd::AsRawFd;
use std::os::fd::AsFd;
use nix::libc;
use nix::pty::Winsize;
use nix::libc::{TIOCGWINSZ, ioctl};
use crate::error::Result;


pub struct Screen {
    rows: u16,
    cols: u16,
}

impl Screen {
    pub fn new() -> Result<Self> {
        let (rows, cols) = Self::terminal_size()?;

        Ok(Self { rows, cols })
    }

    pub fn terminal_size() -> Result<(u16, u16)> {
        let stdout = io::stdout();

        let mut size = Winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        let result = unsafe {
            ioctl(
                stdout.as_raw_fd(),
                TIOCGWINSZ,
                &mut size,
            )
        };

        if result == -1 {
            return Err(io::Error::last_os_error().into());
        }

        Ok((size.ws_row, size.ws_col))
    }

    pub fn resize(&mut self) -> Result<()> {
        let (rows, cols) = Self::terminal_size()?;

        self.rows = rows;
        self.cols = cols;

        Ok(())
    }

    pub fn rows(&self) -> u16 {
        self.rows
    }

    pub fn cols(&self) -> u16 {
        self.cols
    }
}