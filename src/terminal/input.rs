use crate::error::Result;

use crossterm::event::{self, Event, KeyCode, KeyEvent};

use std::io;
use std::os::fd::{AsFd, BorrowedFd};

pub enum InputEvent {
    Key(KeyEvent),
    Resize(u16, u16),
    Quit,
}

pub fn stdin_fd() -> BorrowedFd<'static> {
    // stdin (fd 0)
    unsafe { BorrowedFd::borrow_raw(0) }
}

pub fn read_event() -> Result<InputEvent> {
    let event = event::read()?;

    match event {
        Event::Key(key) => {
            if key.code == KeyCode::Char('c')
                && key.modifiers.contains(event::KeyModifiers::CONTROL)
            {
                return Ok(InputEvent::Quit);
            }

            Ok(InputEvent::Key(key))
        }

        Event::Resize(width, height) => Ok(InputEvent::Resize(width, height)),

        _ => read_event(),
    }
}
