use std::{sync::mpsc, thread, time::{Duration, Instant}};

use anyhow::Result;
use crossterm::event::{self, KeyEvent, MouseEvent, Event as CrosstermEvent};

/// Terminal events
#[derive(Clone, Copy, Debug)]
pub enum Event {
    /// Terminal Tick
    Tick,
    /// Key Press,
    Key(KeyEvent),
    /// Mouse click/scroll
    Mouse(MouseEvent),
    /// Terminal resize
    Resize(u16, u16),
}


/// Terminal event handler
#[derive(Debug)]
pub struct EventHandler {
    /// Kept alive so the channel stays open; the spawned thread holds a clone.
    #[allow(dead_code)]
    sender: mpsc::Sender<Event>,
    receiver: mpsc::Receiver<Event>,
    /// The thread exits cleanly when the receiver is dropped (channel closes).
    #[allow(dead_code)]
    handler: thread::JoinHandle<()>,
}

impl EventHandler {
    /// Constructs a new instance of [`EventHandler`]
    pub fn new(tick_rate: u64) -> Self {
        let tick_rate = Duration::from_millis(tick_rate);
        let (sender, receiver) = mpsc::channel();
        let handler = {
            let sender = sender.clone();
            thread::spawn(move || {
                let mut last_tick = Instant::now();
                loop {
                    let timeout = tick_rate
                        .checked_sub(last_tick.elapsed())
                        .unwrap_or(tick_rate);

                    match event::poll(timeout) {
                        Err(_) => break,
                        Ok(true) => {
                            let ev = match event::read() {
                                Ok(e) => e,
                                Err(_) => break,
                            };
                            let result = match ev {
                                CrosstermEvent::Key(e) => {
                                    if e.kind == event::KeyEventKind::Press {
                                        sender.send(Event::Key(e))
                                    } else {
                                        Ok(()) // ignore KeyEventKind::Release
                                    }
                                }
                                CrosstermEvent::Mouse(e) => sender.send(Event::Mouse(e)),
                                CrosstermEvent::Resize(w, h) => sender.send(Event::Resize(w, h)),
                                _ => Ok(()),
                            };
                            if result.is_err() {
                                break;
                            }
                        }
                        Ok(false) => {}
                    }

                    if last_tick.elapsed() >= tick_rate {
                        if sender.send(Event::Tick).is_err() {
                            break;
                        }
                        last_tick = Instant::now();
                    }
                }
            })
        };

        Self { sender, receiver, handler }
    }

    /// Receive the next event from the handler thread
    pub fn next(&self) -> Result<Event> {
        Ok(self.receiver.recv()?)
    }
}
