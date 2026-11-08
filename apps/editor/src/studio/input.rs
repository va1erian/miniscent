//! Play's input on its way to the render thread, without the command queue:
//! nothing is dropped, and the UI thread never waits for the render thread.

use std::cell::RefCell;
use std::sync::{Arc, Mutex, TryLockError};

use mscent_platform::Event;

/// Events the render thread takes at the start of its next frame.
pub type Inbox = Arc<Mutex<Vec<Event>>>;

/// One way into the render thread's input. Play's pane holds one and hands
/// each event over as it arrives, as the game reads its input right before it
/// steps: waiting for the next UI tick would add up to a frame. A mouse sends
/// hundreds of deltas a second, which a bounded queue would drop, along with
/// the commands that share it.
pub struct Input {
    inbox: Inbox,
    /// What could not be handed over yet, as the render thread held the inbox.
    pending: RefCell<Vec<Event>>,
}

impl Input {
    pub fn new(inbox: Inbox) -> Self {
        Self {
            inbox,
            pending: RefCell::new(Vec::new()),
        }
    }

    /// Hands `events` over, after any that were waiting. While the render thread
    /// holds the inbox (for as long as a swap) they wait for the next call, which
    /// the UI tick makes with none.
    pub fn send(&self, events: &[Event]) {
        let mut pending = self.pending.borrow_mut();
        pending.extend_from_slice(events);
        if pending.is_empty() {
            return;
        }
        let mut inbox = match self.inbox.try_lock() {
            Ok(inbox) => inbox,
            Err(TryLockError::Poisoned(p)) => p.into_inner(),
            Err(TryLockError::WouldBlock) => return,
        };
        inbox.append(&mut pending);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOVE: Event = Event::MouseMotion { dx: 1.0, dy: 2.0 };

    #[test]
    fn events_are_handed_over_at_once_and_nothing_is_sent_for_none() {
        let inbox = Inbox::default();
        let input = Input::new(inbox.clone());
        input.send(&[]);
        input.send(&[Event::FocusLost, MOVE]);
        assert_eq!(*inbox.lock().unwrap(), [Event::FocusLost, MOVE]);
    }

    #[test]
    fn events_wait_while_the_inbox_is_held_and_none_is_lost() {
        let inbox = Inbox::default();
        let input = Input::new(inbox.clone());
        let held = inbox.lock().unwrap();
        for _ in 0..1000 {
            input.send(&[MOVE]);
        }
        drop(held);
        input.send(&[]);
        assert_eq!(inbox.lock().unwrap().len(), 1000, "every one, in order");
    }
}
