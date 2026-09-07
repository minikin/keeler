//! Entering the board's screen, and leaving it whatever happens.
//!
//! Two ways out of a live board, and both of them have to put the terminal
//! back. The ordinary one is `q`, or an error the loop gives up on: the
//! guard's `Drop` runs and the screen is restored on the way past. The other
//! is a panic, and there `Drop` is not enough — the panic *message* is
//! printed by the panic hook, which runs before anything unwinds, so a board
//! that only restored on drop would print its own crash onto the alternate
//! screen and then throw that screen away. The hook here restores first and
//! prints after, which is the whole of the difference between a stack trace
//! and a terminal that has apparently hung.
//!
//! What the terminal actually is sits behind [`Screen`]. Raw mode and the
//! alternate screen are the one part of the board no test can drive — a test
//! process has no terminal to put into raw mode — so the trait is where the
//! guard is tested and [`Tty`] is the single implementation that touches a
//! real one.

use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

/// What entering and leaving the board's screen does.
pub trait Screen {
    /// Takes the terminal over: raw mode, and a screen of the board's own.
    ///
    /// # Errors
    ///
    /// Whatever the terminal refused. A board that could not take the screen
    /// must not draw on the one it failed to take.
    fn enter(&mut self) -> std::io::Result<()>;

    /// Gives it back.
    ///
    /// # Errors
    ///
    /// Whatever the terminal refused — which the guard can only ignore, it
    /// being on the way out of a scope by then.
    fn leave(&mut self) -> std::io::Result<()>;
}

/// The terminal the board is actually run on.
#[derive(Debug, Clone, Copy)]
pub struct Tty;

impl Screen for Tty {
    // Out of the mutation gate's reach, and this is the whole of what is:
    // `enable_raw_mode` reads and rewrites the termios of a real terminal,
    // and a test process has none — under nextest every test is a process
    // whose stdout is a pipe. What the mutation gate would replace this
    // with is `Ok(())`, which is what a test could observe either way. The
    // decisions that could be wrong were moved out of here rather than
    // excused: which order the two run in is [`restore`]'s, and what holds
    // them is [`Guard`], and both are tested against a double.
    #[cfg_attr(test, mutants::skip)]
    fn enter(&mut self) -> std::io::Result<()> {
        enable_raw_mode()?;
        execute!(std::io::stdout(), EnterAlternateScreen)
    }

    #[cfg_attr(test, mutants::skip)]
    fn leave(&mut self) -> std::io::Result<()> {
        restore()
    }
}

/// Puts the terminal back the way it was found.
///
/// The alternate screen first and raw mode after, which is the order
/// [`Tty::enter`] runs backwards, and both are attempted whatever the first
/// one answered: a terminal left in raw mode because the screen switch
/// failed is one whose shell no longer echoes what is typed into it.
///
/// # Errors
///
/// The first refusal of the two, the other having been attempted anyway.
///
/// Outside the mutation gate for the reason [`Tty::enter`] gives: both calls
/// are a real terminal's, and a test process has none.
#[cfg_attr(test, mutants::skip)]
pub fn restore() -> std::io::Result<()> {
    let left = execute!(std::io::stdout(), LeaveAlternateScreen);
    let cooked = disable_raw_mode();
    left.and(cooked)
}

/// The screen, held for as long as the board is drawing on it.
///
/// The value is the whole point: it cannot be constructed without entering,
/// and it cannot go out of scope — by return, by `?`, or by a panic
/// unwinding past it — without leaving.
#[derive(Debug)]
pub struct Guard<S: Screen> {
    screen: S,
}

impl<S: Screen> Guard<S> {
    /// Enters the screen, or gives back the reason it could not be entered.
    ///
    /// # Errors
    ///
    /// The terminal's own refusal. Nothing is restored on that path because
    /// nothing was taken: a guard that failed to enter is not returned, so
    /// its `Drop` never runs.
    pub fn new(mut screen: S) -> std::io::Result<Self> {
        screen.enter()?;
        Ok(Self { screen })
    }
}

impl<S: Screen> Drop for Guard<S> {
    fn drop(&mut self) {
        let _ = self.screen.leave();
    }
}

/// Restores the terminal before the panic message is printed.
///
/// `restore` runs first and the hook that was already installed runs after,
/// so the message lands on the screen the reader can still see. Chaining
/// rather than replacing: the hook in place is the one that prints, and on a
/// machine where somebody has installed their own it is theirs.
pub fn restore_on_panic(restore: impl Fn() + Sync + Send + 'static) {
    let printed = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        restore();
        printed(panic);
    }));
}

#[cfg(test)]
mod tests {
    use super::{Guard, Screen, restore_on_panic};
    use std::sync::{Arc, Mutex};

    /// A screen that records what was asked of it, and can be told to refuse.
    #[derive(Debug, Default)]
    struct Recorder {
        done: Arc<Mutex<Vec<&'static str>>>,
        refuses: bool,
    }

    impl Screen for Recorder {
        fn enter(&mut self) -> std::io::Result<()> {
            if self.refuses {
                return Err(std::io::Error::other("no terminal here"));
            }
            self.done.lock().expect("the recorder").push("enter");
            Ok(())
        }

        fn leave(&mut self) -> std::io::Result<()> {
            self.done.lock().expect("the recorder").push("leave");
            Ok(())
        }
    }

    #[test]
    fn the_screen_is_entered_by_holding_the_guard_and_left_by_dropping_it() {
        let done = Arc::new(Mutex::new(Vec::new()));

        {
            let _guard = Guard::new(Recorder {
                done: Arc::clone(&done),
                refuses: false,
            })
            .expect("the recorder entered");
            assert_eq!(*done.lock().expect("the recorder"), ["enter"]);
        }

        assert_eq!(
            *done.lock().expect("the recorder"),
            ["enter", "leave"],
            "the screen was not given back",
        );
    }

    #[test]
    fn a_screen_that_could_not_be_entered_is_not_a_screen_to_give_back() {
        let done = Arc::new(Mutex::new(Vec::new()));

        let refused = Guard::new(Recorder {
            done: Arc::clone(&done),
            refuses: true,
        })
        .expect_err("the recorder refused");

        assert_eq!(refused.to_string(), "no terminal here");
        assert!(
            done.lock().expect("the recorder").is_empty(),
            "a terminal nobody took was put back",
        );
    }

    #[test]
    fn the_panic_hook_restores_the_terminal_before_the_message_is_printed() {
        // The hook is process-wide, and nextest gives every test a process
        // of its own — which is what makes installing one here safe.
        let order = Arc::new(Mutex::new(Vec::new()));
        let printing = Arc::clone(&order);
        std::panic::set_hook(Box::new(move |_| {
            printing.lock().expect("the order").push("printed");
        }));
        let restoring = Arc::clone(&order);
        restore_on_panic(move || {
            restoring.lock().expect("the order").push("restored");
        });

        let ended = std::panic::catch_unwind(|| panic!("the board fell over"));

        assert!(ended.is_err(), "the fixture did not panic");
        // Read out before asserting, never asserted on through the guard:
        // the hook installed above locks this list, and a failing
        // `assert_eq!` holding the lock would panic into a hook that waits
        // for it — a deadlock where a test failure should be.
        let seen = order.lock().expect("the order").clone();
        assert_eq!(
            seen,
            ["restored", "printed"],
            "the message was printed onto a screen about to be thrown away",
        );
    }
}
