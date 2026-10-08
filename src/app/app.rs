use crate::{
    backend::pty::Pty,
    error::Result,
    terminal::{renderer, screen::Screen},
};
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use nix::sys::signal::{SigSet, Signal};
use nix::sys::signalfd::SignalFd;
use nix::unistd::read;

use std::io;
use std::os::fd::AsFd;

pub struct App {
    running: bool,
    pty: Pty,
    screen: Screen,
}

impl App {
    pub fn new() -> Result<Self> {
        let pty = Pty::spawn_shell()?;
        let screen = Screen::new()?;

        Ok(Self {
            running: true,
            pty,
            screen,
        })
    }

    pub fn run(&mut self) -> Result<()> {
        renderer::enter()?;

        let result = self.run_loop();

        renderer::exit()?;

        result
    }

    fn run_loop(&mut self) -> Result<()> {
        let mut buffer = [0u8; 4096];

        let stdin = io::stdin();

        // Convert SIGWINCH into a file-descriptor event.
        let mut mask = SigSet::empty();
        mask.add(Signal::SIGWINCH);
        mask.thread_block()?;

        let signal_fd = SignalFd::new(&mask)?;

        while self.running {
            let mut fds = [
                PollFd::new(stdin.as_fd(), PollFlags::POLLIN),
                PollFd::new(self.pty.fd(), PollFlags::POLLIN),
                PollFd::new(signal_fd.as_fd(), PollFlags::POLLIN),
            ];

            // Block until at least one file descriptor is ready.
            poll(&mut fds, PollTimeout::NONE)?;

            let stdin_events =
                fds[0].revents().unwrap_or(PollFlags::empty());

            let pty_events =
                fds[1].revents().unwrap_or(PollFlags::empty());

            let signal_events =
                fds[2].revents().unwrap_or(PollFlags::empty());

            // Keyboard input
            if stdin_events.contains(PollFlags::POLLIN) {
                let mut input = [0u8; 1024];

                let size = read(stdin.as_fd(), &mut input)?;

                if size > 0 {
                    self.pty.write(&input[..size])?;
                }
            }

            // PTY output
            if pty_events.contains(PollFlags::POLLIN) {
                if let Some(size) = self.pty.read(&mut buffer)? {
                    if size > 0 {
                        renderer::write(&buffer[..size])?;
                    }
                }
            }

            // Terminal resized
            if signal_events.contains(PollFlags::POLLIN) {
                if signal_fd.read_signal()?.is_some() {
                    self.screen.resize()?;

                    self.pty.resize(
                        self.screen.rows(),
                        self.screen.cols(),
                    )?;
                }
            }

            // PTY closed/error
            if pty_events.intersects(
                PollFlags::POLLHUP
                    | PollFlags::POLLERR
                    | PollFlags::POLLNVAL,
            ) {
                self.running = false;
            }
        }

        Ok(())
    }
}