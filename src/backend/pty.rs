use crate::error::Result;

use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::libc::{TIOCSWINSZ, ioctl};
use nix::pty::{ForkptyResult, Winsize, forkpty};
use nix::unistd::{execvp, read, write};
use std::os::fd::AsRawFd;

use std::ffi::CString;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

pub struct Pty {
    master: OwnedFd,
}

impl Pty {
    pub fn spawn_shell() -> Result<Self> {
        let winsize = Winsize {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        match unsafe { forkpty(Some(&winsize), None)? } {
            ForkptyResult::Parent { master, .. } => {
                let flags = fcntl(&master, FcntlArg::F_GETFL)?;
                let flags = OFlag::from_bits_truncate(flags);

                fcntl(&master, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;

                Ok(Self { master })
            }

            ForkptyResult::Child => {
                let shell = CString::new("/bin/sh")?;

                execvp(&shell, &[shell.clone()])?;

                unreachable!();
            }
        }
    }

    pub fn fd(&self) -> BorrowedFd<'_> {
        self.master.as_fd()
    }

    pub fn read(&self, buffer: &mut [u8]) -> Result<Option<usize>> {
        match read(&self.master, buffer) {
            Ok(size) => Ok(Some(size)),

            Err(nix::errno::Errno::EAGAIN) => Ok(None),

            Err(error) => Err(error.into()),
        }
    }

    pub fn write(&self, buffer: &[u8]) -> Result<usize> {
        Ok(write(&self.master, buffer)?)
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        let size = Winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        let result = unsafe { ioctl(self.master.as_raw_fd(), TIOCSWINSZ, &size) };

        if result == -1 {
            return Err(std::io::Error::last_os_error().into());
        }

        Ok(())
    }
}
