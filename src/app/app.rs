use crate::constants::{SHIFT_PAGE_DOWN, SHIFT_PAGE_UP};
use crate::{
    error::Result,
    pane::pane::Pane,
    terminal::{renderer, screen::Screen},
};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use nix::sys::signal::{SigSet, Signal};
use nix::sys::signalfd::SignalFd;
use nix::unistd::read;
use std::io;
use std::os::fd::AsFd;

pub struct App {
    running: bool,
    panes: Vec<Pane>,
    focused: usize,
}

/// Single source of truth for horizontal layout.
/// Returns `(start_column, width)` for pane `index` out of `count`
/// when the terminal is `cols` wide. Used by both `resize_panes()`
/// and `render_panes()` so PTY sizes always match drawn rectangles.
fn pane_columns(index: usize, count: usize, cols: u16) -> (u16, u16) {
    let start = (index * cols as usize) / count;
    let end = ((index + 1) * cols as usize) / count;
    let width = (end - start).max(1);

    (start as u16, width as u16)
}

impl App {
    pub fn new() -> Result<Self> {
        let (rows, cols) = Screen::terminal_size()?;
        let pane = Pane::new(rows, cols)?;

        let mut app = Self {
            running: true,
            panes: vec![pane],
            focused: 0,
        };

        app.resize_panes(rows, cols)?;
        Ok(app)
    }

    pub fn create_new_terminal(&mut self) -> Result<()> {
        let (rows, cols) = Screen::terminal_size()?;

        // Every pane needs at least one terminal column.
        if self.panes.len() >= (cols as usize) / 2 {
            return Ok(());
        }

        self.panes.push(Pane::new(rows, cols)?);
        self.focused = self.panes.len() - 1;

        // Resize every pane (including the new one) to the new layout,
        // then redraw everything and place the cursor in the focused pane.
        self.resize_panes(rows, cols)?;
        self.render_panes()
    }

    fn focus_next(&mut self) -> Result<()> {
        if self.panes.is_empty() {
            return Ok(());
        }

        self.focused = (self.focused + 1) % self.panes.len();

        // Redraw so the cursor moves to the newly focused pane.
        self.render_panes()
    }

    fn resize_panes(&mut self, rows: u16, cols: u16) -> Result<()> {
        let count = self.panes.len();

        if count == 0 {
            return Ok(());
        }

        for (i, pane) in self.panes.iter_mut().enumerate() {
            let (_, width) = pane_columns(i, count, cols);

            // Reserve the rightmost column of the pane for the scrollbar,
            // but never shrink the content below 1 column.
            let content_width = if width > 1 { width - 1 } else { 1 };

            // Pane::resize updates both the Screen and the PTY.
            pane.resize(rows, content_width)?;
        }

        Ok(())
    }

    fn pane_rect(index: usize, count: usize, rows: u16, cols: u16) -> renderer::Rect {
        let (x, width) = pane_columns(index, count, cols);

        let content_width = if index + 1 < count {
            width.saturating_sub(2).max(1)
        } else {
            width
        };

        renderer::Rect {
            x,
            y: 0,
            width: content_width,
            height: rows,
        }
    }

    fn render_panes(&self) -> Result<()> {
        let (rows, cols) = Screen::terminal_size()?;
        let count = self.panes.len();

        if count == 0 {
            return Ok(());
        }

        renderer::hide_cursor()?;

        for (i, pane) in self.panes.iter().enumerate() {
            renderer::render_region(pane.screen(), Self::pane_rect(i, count, rows, cols), true)?;
        }

        // Draw boundaries between panes.
        for i in 1..count {
            let (x, _) = pane_columns(i, count, cols);

            renderer::render_vertical_separator(self.panes[i].screen(), x, 0, rows)?;
        }

        let focused = self.focused.min(count - 1);

        renderer::render_cursor(
            self.panes[focused].screen(),
            Self::pane_rect(focused, count, rows, cols),
        )?;

        Ok(())
    }

    fn flush_pending(&mut self, pending: &mut Vec<u8>) -> Result<()> {
        if !pending.is_empty() {
            self.panes[self.focused].write_input(pending)?;
            pending.clear();
        }
        Ok(())
    }

    fn handle_input(&mut self, input: &[u8]) -> Result<()> {
        let mut pending = Vec::new();
        let mut i = 0;

        while i < input.len() {
            let rest = &input[i..];

            if rest.starts_with(SHIFT_PAGE_UP) {
                self.flush_pending(&mut pending)?;
                self.panes[self.focused].scroll_page_up();
                i += SHIFT_PAGE_UP.len();
                continue;
            }

            if rest.starts_with(SHIFT_PAGE_DOWN) {
                self.flush_pending(&mut pending)?;
                self.panes[self.focused].scroll_page_down();
                i += SHIFT_PAGE_DOWN.len();
                continue;
            }

            match input[i] {
                0x14 => {
                    self.flush_pending(&mut pending)?;
                    self.create_new_terminal()?;
                }
                0x0E => {
                    self.flush_pending(&mut pending)?;
                    self.focus_next()?;
                }
                byte => pending.push(byte),
            }

            i += 1;
        }

        self.flush_pending(&mut pending)?;
        self.render_panes()
    }

    pub fn run(&mut self) -> Result<()> {
        renderer::enter()?;
        let result = self.run_loop();
        let exit_result = renderer::exit();

        result?;
        exit_result
    }

    fn run_loop(&mut self) -> Result<()> {
        let mut buffer = [0u8; 4096];
        let stdin = io::stdin();

        let mut mask = SigSet::empty();
        mask.add(Signal::SIGWINCH);
        mask.thread_block()?;

        let signal_fd = SignalFd::new(&mask)?;

        while self.running {
            let pane_count = self.panes.len();

            let mut fds = Vec::with_capacity(pane_count + 2);
            fds.push(PollFd::new(stdin.as_fd(), PollFlags::POLLIN));

            for pane in &self.panes {
                fds.push(PollFd::new(pane.pty().fd(), PollFlags::POLLIN));
            }

            fds.push(PollFd::new(signal_fd.as_fd(), PollFlags::POLLIN));

            poll(&mut fds, PollTimeout::NONE)?;

            // Copy readiness flags before mutating the panes.
            let events: Vec<PollFlags> = fds
                .iter()
                .map(|fd| fd.revents().unwrap_or(PollFlags::empty()))
                .collect();

            drop(fds);

            // Keyboard input.
            if events[0].contains(PollFlags::POLLIN) {
                let mut input = [0u8; 1024];
                let size = read(stdin.as_fd(), &mut input)?;

                if size > 0 {
                    self.handle_input(&input[..size])?;
                }
            }

            // Process output from each PTY polled this iteration.
            let mut closed = Vec::new();

            for i in 0..pane_count {
                let pane_events = events[i + 1];

                if pane_events.contains(PollFlags::POLLIN) {
                    if let Some(size) = self.panes[i].pty().read(&mut buffer)? {
                        if size > 0 {
                            self.panes[i].process_output(&buffer[..size]);
                        }
                    }
                }

                if pane_events
                    .intersects(PollFlags::POLLHUP | PollFlags::POLLERR | PollFlags::POLLNVAL)
                {
                    closed.push(i);
                }
            }

            // Remove closed panes from highest index downward.
            // Remove closed panes from highest index downward.
            let had_closed_panes = !closed.is_empty();

            for i in closed.into_iter().rev() {
                self.panes.remove(i);

                if i < self.focused {
                    self.focused -= 1;
                }
            }

            if self.panes.is_empty() {
                self.running = false;
                break;
            }

            self.focused = self.focused.min(self.panes.len() - 1);

            // Recalculate pane widths and shift remaining panes left.
            if had_closed_panes {
                let (rows, cols) = Screen::terminal_size()?;
                self.resize_panes(rows, cols)?;
            }

            let signal_index = pane_count + 1;

            if events[signal_index].contains(PollFlags::POLLIN)
                && signal_fd.read_signal()?.is_some()
            {
                let (rows, cols) = Screen::terminal_size()?;
                self.resize_panes(rows, cols)?;
            }

            self.render_panes()?;
        }

        Ok(())
    }
}
