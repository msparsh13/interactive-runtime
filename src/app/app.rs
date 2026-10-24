use crate::constants::{SHIFT_PAGE_DOWN, SHIFT_PAGE_UP};
use crate::{
    error::Result,
    pane::pane::Pane,
    terminal::{
        layout::{Dir, Divider, Node, Placed},
        renderer::{self, Rect},
        screen::Screen,
    },
};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use nix::sys::signal::{SigSet, Signal};
use nix::sys::signalfd::SignalFd;
use nix::unistd::read;
use std::io;
use std::os::fd::AsFd;

const KEY_SPLIT_SIDE: u8 = 0x14; // Ctrl+T
const KEY_SPLIT_STACK: u8 = 0x1D; // Ctrl+]
const KEY_FLOAT: u8 = 0x0F; // Ctrl+O
const KEY_NEXT: u8 = 0x0E; // Ctrl+N

struct Entry {
    id: usize,
    pane: Pane,
    float: Option<Rect>, // outer rect (with border) if floating
}

#[derive(Clone)]
enum Drag {
    Divider {
        path: Vec<bool>,
        dir: Dir,
        area: Rect,
    },
    FloatMove {
        id: usize,
        dx: u16,
    },
    FloatResize {
        id: usize,
    },
}

struct Layout {
    tiled: Vec<Placed>,
    dividers: Vec<Divider>,
    floats: Vec<(usize, Rect)>, // bottom-to-top
}

#[derive(Clone, Copy)]
struct Mouse {
    button: u32,
    x: u16, // 0-based
    y: u16,
    press: bool,
    len: usize,
}

pub struct App {
    running: bool,
    entries: Vec<Entry>,
    root: Option<Node>,
    focused: usize, // pane id
    next_id: usize,
    needs_clear: bool,
    drag: Option<Drag>,
}

/// Last column of a pane's slot is the scrollbar.
fn content_width(width: u16) -> u16 {
    width.saturating_sub(1).max(1)
}

fn content_rect(r: Rect) -> Rect {
    Rect {
        width: content_width(r.width),
        ..r
    }
}

fn float_inner(outer: Rect) -> Rect {
    Rect {
        x: outer.x + 1,
        y: outer.y + 1,
        width: outer.width.saturating_sub(2),
        height: outer.height.saturating_sub(2),
    }
}

fn clamp_rect(r: Rect, cols: u16, rows: u16) -> Rect {
    let width = r.width.max(6).min(cols);
    let height = r.height.max(4).min(rows);
    Rect {
        x: r.x.min(cols - width),
        y: r.y.min(rows - height),
        width,
        height,
    }
}

fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x.saturating_add(r.width) && y >= r.y && y < r.y.saturating_add(r.height)
}

/// Parses ESC [ < b ; x ; y (M|m).
fn parse_sgr_mouse(rest: &[u8]) -> Option<Mouse> {
    if !rest.starts_with(b"\x1b[<") {
        return None;
    }
    let end = rest.iter().position(|&b| b == b'M' || b == b'm')?;
    let s = std::str::from_utf8(&rest[3..end]).ok()?;
    let mut it = s.split(';');
    let button = it.next()?.parse().ok()?;
    let x: u16 = it.next()?.parse().ok()?;
    let y: u16 = it.next()?.parse().ok()?;
    Some(Mouse {
        button,
        x: x.saturating_sub(1),
        y: y.saturating_sub(1),
        press: rest[end] == b'M',
        len: end + 1,
    })
}

impl App {
    pub fn new() -> Result<Self> {
        let (rows, cols) = Screen::terminal_size()?;
        let mut app = Self {
            running: true,
            entries: vec![Entry {
                id: 0,
                pane: Pane::new(rows, cols)?,
                float: None,
            }],
            root: Some(Node::Leaf(0)),
            focused: 0,
            next_id: 1,
            needs_clear: true,
            drag: None,
        };
        app.apply_layout()?;
        Ok(app)
    }

    // ---------- helpers ----------

    fn entry(&self, id: usize) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    fn entry_mut(&mut self, id: usize) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    fn spawn_pane(&mut self, rows: u16, cols: u16, float: Option<Rect>) -> Result<usize> {
        let pane = Pane::new(rows.max(1), cols.max(1))?;
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(Entry { id, pane, float });
        Ok(id)
    }

    fn focus(&mut self, id: usize) {
        self.focused = id;
        if let Some(i) = self.entries.iter().position(|e| e.id == id) {
            if self.entries[i].float.is_some() {
                // raise floating window to the top
                let e = self.entries.remove(i);
                self.entries.push(e);
            }
        }
    }

    fn focus_next(&mut self) -> Result<()> {
        let mut ids: Vec<usize> = self.entries.iter().map(|e| e.id).collect();
        if ids.is_empty() {
            return Ok(());
        }
        ids.sort_unstable();
        let pos = ids.iter().position(|&i| i == self.focused).unwrap_or(0);
        self.focus(ids[(pos + 1) % ids.len()]);
        self.render_panes()
    }

    fn close(&mut self, id: usize) {
        self.entries.retain(|e| e.id != id);
        self.root = self.root.take().and_then(|r| r.remove(id));
        self.drag = None;
        if self.focused == id {
            self.focused = self.entries.last().map(|e| e.id).unwrap_or(0);
        }
        self.needs_clear = true;
    }

    // ---------- layout ----------

    fn compute_layout(&self) -> Result<Layout> {
        let (rows, cols) = Screen::terminal_size()?;
        let mut l = Layout {
            tiled: Vec::new(),
            dividers: Vec::new(),
            floats: Vec::new(),
        };

        if let Some(root) = &self.root {
            root.layout(
                Rect {
                    x: 0,
                    y: 0,
                    width: cols,
                    height: rows,
                },
                &mut Vec::new(),
                &mut l.tiled,
                &mut l.dividers,
            );
        }
        for e in &self.entries {
            if let Some(r) = e.float {
                l.floats.push((e.id, clamp_rect(r, cols, rows)));
            }
        }
        Ok(l)
    }

    /// Push the computed sizes into every Screen + PTY.
    fn apply_layout(&mut self) -> Result<()> {
        let l = self.compute_layout()?;
        for p in &l.tiled {
            self.resize_pane(p.id, p.rect)?;
        }
        for (id, outer) in &l.floats {
            self.resize_pane(*id, float_inner(*outer))?;
        }
        Ok(())
    }

    fn resize_pane(&mut self, id: usize, area: Rect) -> Result<()> {
        if let Some(e) = self.entry_mut(id) {
            e.pane
                .resize(area.height.max(1), content_width(area.width))?;
        }
        Ok(())
    }

    fn relayout_all(&mut self) -> Result<()> {
        self.needs_clear = true;
        self.apply_layout()
    }

    // ---------- creating panes ----------

    fn split_focused(&mut self, dir: Dir) -> Result<()> {
        let (rows, cols) = Screen::terminal_size()?;
        let id = self.spawn_pane(rows, cols, None)?;
        if self.entries.len() >= (cols as usize) / 2 {
            return Ok(());
        }

        let id = self.spawn_pane(0, 0, None)?;
        let focused = self.focused;

        let split_ok = match self.root.as_mut() {
            Some(r) => r.split_leaf(focused, dir, id),
            None => false,
        };
        if !split_ok {
            // focused pane is floating (or nothing tiled): split the whole layout
            self.root = Some(match self.root.take() {
                Some(old) => Node::Split {
                    dir,
                    ratio: 0.5,
                    a: Box::new(old),
                    b: Box::new(Node::Leaf(id)),
                },
                None => Node::Leaf(id),
            });
        }

        self.focus(id);
        self.relayout_all()?;
        self.render_panes()
    }

    fn new_float(&mut self) -> Result<()> {
        let (rows, cols) = Screen::terminal_size()?;
        let n = self.entries.iter().filter(|e| e.float.is_some()).count() as u16;
        let width = (cols * 3 / 5).max(20).min(cols);
        let height = (rows * 3 / 5).max(8).min(rows);
        let rect = clamp_rect(
            Rect {
                x: (cols - width) / 2 + n * 2,
                y: (rows - height) / 2 + n,
                width,
                height,
            },
            cols,
            rows,
        );
        let inner = float_inner(rect);
        let id = self.spawn_pane(inner.height, content_width(inner.width), Some(rect))?;
        self.focus(id);
        self.relayout_all()?;
        self.render_panes()
    }

    // ---------- rendering ----------

    fn render_panes(&mut self) -> Result<()> {
        let l = self.compute_layout()?;

        renderer::hide_cursor()?;
        if self.needs_clear {
            renderer::clear_all()?;
            self.needs_clear = false;
        }

        for p in &l.tiled {
            if let Some(e) = self.entry(p.id) {
                renderer::render_region(e.pane.screen(), content_rect(p.rect), true)?;
                if p.rect.width > 1 {
                    renderer::render_scrollbar(
                        e.pane.screen(),
                        p.rect.x + p.rect.width - 1,
                        p.rect.y,
                        p.rect.height,
                    )?;
                }
            }
        }

        for d in &l.dividers {
            if d.dir == Dir::Stacked {
                renderer::render_hline(d.rect.x, d.rect.y, d.rect.width)?;
            }
        }

        for (id, outer) in &l.floats {
            if let Some(e) = self.entry(*id) {
                let inner = float_inner(*outer);
                renderer::render_box(*outer, &format!("term {}", id), *id == self.focused)?;
                renderer::render_region(e.pane.screen(), content_rect(inner), true)?;
                if inner.width > 1 {
                    renderer::render_scrollbar(
                        e.pane.screen(),
                        inner.x + inner.width - 1,
                        inner.y,
                        inner.height,
                    )?;
                }
            }
        }

        let focus_rect = l
            .floats
            .iter()
            .find(|(id, _)| *id == self.focused)
            .map(|(_, o)| content_rect(float_inner(*o)))
            .or_else(|| {
                l.tiled
                    .iter()
                    .find(|p| p.id == self.focused)
                    .map(|p| content_rect(p.rect))
            });

        match (focus_rect, self.entry(self.focused)) {
            (Some(r), Some(e)) => renderer::render_cursor(e.pane.screen(), r)?,
            _ => renderer::hide_cursor()?,
        }
        Ok(())
    }

    // ---------- mouse ----------

    fn pane_at(&self, l: &Layout, x: u16, y: u16) -> Option<usize> {
        for (id, r) in l.floats.iter().rev() {
            if contains(*r, x, y) {
                return Some(*id);
            }
        }
        l.tiled
            .iter()
            .find(|p| contains(p.rect, x, y))
            .map(|p| p.id)
    }

    fn handle_mouse(&mut self, m: Mouse) -> Result<()> {
        // strip shift/alt/ctrl bits
        let button = m.button & !0x1C;

        match button {
            64 | 65 => {
                let l = self.compute_layout()?;
                if let Some(id) = self.pane_at(&l, m.x, m.y) {
                    if let Some(e) = self.entry_mut(id) {
                        if button == 64 {
                            e.pane.scroll_view_up(3);
                        } else {
                            e.pane.scroll_view_down(3);
                        }
                    }
                }
                Ok(())
            }
            0 if m.press => self.mouse_down(m.x, m.y),
            0 => {
                self.drag = None;
                Ok(())
            }
            32 => self.mouse_drag(m.x, m.y),
            _ => Ok(()),
        }
    }

    fn mouse_down(&mut self, x: u16, y: u16) -> Result<()> {
        let l = self.compute_layout()?;

        // 1. floating windows, topmost first
        if let Some((id, r)) = l
            .floats
            .iter()
            .rev()
            .find(|(_, r)| contains(*r, x, y))
            .copied()
        {
            self.focus(id);
            if y == r.y {
                self.drag = Some(Drag::FloatMove { id, dx: x - r.x });
            } else if x == r.x + r.width - 1 && y == r.y + r.height - 1 {
                self.drag = Some(Drag::FloatResize { id });
            }
            return self.render_panes();
        }

        // 2. dividers between tiled panes
        if let Some(d) = l.dividers.iter().rev().find(|d| contains(d.rect, x, y)) {
            self.drag = Some(Drag::Divider {
                path: d.path.clone(),
                dir: d.dir,
                area: d.area,
            });
            return Ok(());
        }

        // 3. click to focus
        if let Some(id) = self.pane_at(&l, x, y) {
            self.focus(id);
        }
        self.render_panes()
    }

    fn mouse_drag(&mut self, x: u16, y: u16) -> Result<()> {
        let (rows, cols) = Screen::terminal_size()?;

        match self.drag.clone() {
            Some(Drag::Divider { path, dir, area }) => {
                let ratio = match dir {
                    Dir::SideBySide => {
                        (x.saturating_sub(area.x) + 1) as f32 / area.width.max(1) as f32
                    }
                    Dir::Stacked => y.saturating_sub(area.y) as f32 / area.height.max(1) as f32,
                };
                if let Some(Node::Split { ratio: r, .. }) =
                    self.root.as_mut().and_then(|n| n.at_mut(&path))
                {
                    *r = ratio.clamp(0.0, 1.0);
                }
            }
            Some(Drag::FloatMove { id, dx }) => {
                if let Some(r) = self.entry_mut(id).and_then(|e| e.float.as_mut()) {
                    r.x = x.saturating_sub(dx);
                    r.y = y;
                    *r = clamp_rect(*r, cols, rows);
                }
                if self.root.is_none() {
                    self.needs_clear = true; // nothing underneath to repaint the old spot
                }
            }
            Some(Drag::FloatResize { id }) => {
                if let Some(r) = self.entry_mut(id).and_then(|e| e.float.as_mut()) {
                    r.width = (x + 1).saturating_sub(r.x);
                    r.height = (y + 1).saturating_sub(r.y);
                    *r = clamp_rect(*r, cols, rows);
                }
                if self.root.is_none() {
                    self.needs_clear = true;
                }
            }
            None => return Ok(()),
        }

        self.apply_layout()?;
        self.render_panes()
    }

    // ---------- input ----------

    fn flush_pending(&mut self, pending: &mut Vec<u8>) -> Result<()> {
        if !pending.is_empty() {
            let id = self.focused;
            if let Some(e) = self.entry_mut(id) {
                e.pane.write_input(pending)?;
            }
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
                let id = self.focused;
                if let Some(e) = self.entry_mut(id) {
                    e.pane.scroll_page_up();
                }
                i += SHIFT_PAGE_UP.len();
                continue;
            }

            if rest.starts_with(SHIFT_PAGE_DOWN) {
                self.flush_pending(&mut pending)?;
                let id = self.focused;
                if let Some(e) = self.entry_mut(id) {
                    e.pane.scroll_page_down();
                }
                i += SHIFT_PAGE_DOWN.len();
                continue;
            }

            if let Some(m) = parse_sgr_mouse(rest) {
                self.flush_pending(&mut pending)?;
                self.handle_mouse(m)?;
                i += m.len;
                continue;
            }

            match input[i] {
                KEY_SPLIT_SIDE => {
                    self.flush_pending(&mut pending)?;
                    self.split_focused(Dir::SideBySide)?;
                }
                KEY_SPLIT_STACK => {
                    self.flush_pending(&mut pending)?;
                    self.split_focused(Dir::Stacked)?;
                }
                KEY_FLOAT => {
                    self.flush_pending(&mut pending)?;
                    self.new_float()?;
                }
                KEY_NEXT => {
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

    // ---------- main loop ----------

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
            // Snapshot ids: indices can change while handling input.
            let ids: Vec<usize> = self.entries.iter().map(|e| e.id).collect();

            let mut fds = Vec::with_capacity(ids.len() + 2);
            fds.push(PollFd::new(stdin.as_fd(), PollFlags::POLLIN));
            for e in &self.entries {
                fds.push(PollFd::new(e.pane.pty().fd(), PollFlags::POLLIN));
            }
            fds.push(PollFd::new(signal_fd.as_fd(), PollFlags::POLLIN));

            poll(&mut fds, PollTimeout::NONE)?;

            let events: Vec<PollFlags> = fds
                .iter()
                .map(|fd| fd.revents().unwrap_or(PollFlags::empty()))
                .collect();
            drop(fds);

            // Keyboard / mouse.
            if events[0].contains(PollFlags::POLLIN) {
                let mut input = [0u8; 1024];
                let size = read(stdin.as_fd(), &mut input)?;
                if size > 0 {
                    self.handle_input(&input[..size])?;
                }
            }

            // PTY output.
            let mut closed = Vec::new();
            for (k, id) in ids.iter().enumerate() {
                let ev = events[k + 1];
                let Some(e) = self.entries.iter_mut().find(|e| e.id == *id) else {
                    continue;
                };

                if ev.contains(PollFlags::POLLIN) {
                    if let Some(size) = e.pane.pty().read(&mut buffer)? {
                        if size > 0 {
                            e.pane.process_output(&buffer[..size]);
                        }
                    }
                }

                if ev.intersects(PollFlags::POLLHUP | PollFlags::POLLERR | PollFlags::POLLNVAL) {
                    closed.push(*id);
                }
            }

            let had_closed = !closed.is_empty();
            for id in closed {
                self.close(id);
            }

            if self.entries.is_empty() {
                self.running = false;
                break;
            }

            if had_closed {
                self.relayout_all()?;
            }

            let signal_index = ids.len() + 1;
            if events[signal_index].contains(PollFlags::POLLIN)
                && signal_fd.read_signal()?.is_some()
            {
                self.relayout_all()?;
            }

            self.render_panes()?;
        }

        Ok(())
    }
}
