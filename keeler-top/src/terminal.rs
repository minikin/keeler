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

use std::sync::atomic::{AtomicBool, Ordering};

use ratatui::crossterm::cursor::Show;
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

/// Whether the board has the screen.
///
/// Process-wide because the panic hook is. The hook cannot reach the guard —
/// it is installed before one exists and outlives every one — so on a panic
/// both of them would give the same screen back, and `ESC[?1049l` sent twice
/// is not idempotent: the second one restores the cursor to where the last
/// entry saved it, which is above the message the first one made room for.
/// Whoever gets here first gives the screen back; the other finds it gone.
static IN_SCREEN: AtomicBool = AtomicBool::new(false);

/// Claims the screen back, and says whether there was one to claim.
fn claimed() -> bool {
    IN_SCREEN.swap(false, Ordering::SeqCst)
}

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
/// `Show` because `Terminal::draw` hides the cursor on every frame that sets
/// no position, and this board sets none. Leaving the alternate screen
/// restores where the cursor is and not whether it can be seen, so a board
/// that did not show it again would hand back a shell whose prompt has no
/// cursor in front of it.
///
/// # Errors
///
/// The first refusal of the two, the other having been attempted anyway.
///
/// Outside the mutation gate for the reason [`Tty::enter`] gives: both calls
/// are a real terminal's, and a test process has none.
#[cfg_attr(test, mutants::skip)]
pub fn restore() -> std::io::Result<()> {
    let left = execute!(std::io::stdout(), LeaveAlternateScreen, Show);
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
        IN_SCREEN.store(true, Ordering::SeqCst);
        Ok(Self { screen })
    }

    /// Gives the screen back for as long as `body` runs, and takes it again
    /// after.
    ///
    /// What `Enter` is made of: `tmux attach` is a program the watcher is
    /// *in*, and it wants the terminal the board is holding — raw mode and
    /// the alternate screen both. Dropping the guard and building another
    /// would give the screen back and take it again, but the flag is the
    /// point: the panic hook and the guard agree by way of it about which
    /// of them restores, and a board that left it saying `true` while tmux
    /// had the terminal would give a screen back that was already gone.
    ///
    /// # Errors
    ///
    /// A screen that would not be given back — and then `body` does not
    /// run, because whatever it is would run onto the board's own screen —
    /// or one that would not be taken again.
    pub fn away(&mut self, body: &mut dyn FnMut()) -> std::io::Result<()> {
        IN_SCREEN.store(false, Ordering::SeqCst);
        self.screen.leave()?;
        body();
        self.screen.enter()?;
        IN_SCREEN.store(true, Ordering::SeqCst);
        Ok(())
    }
}

impl<S: Screen> Drop for Guard<S> {
    fn drop(&mut self) {
        if claimed() {
            let _ = self.screen.leave();
        }
    }
}

/// Restores the terminal before the panic message is printed.
///
/// `restore` runs first and the hook that was already installed runs after,
/// so the message lands on the screen the reader can still see. Chaining
/// rather than replacing: the hook in place is the one that prints, and on a
/// machine where somebody has installed their own it is theirs.
///
/// Two things it does not do. It does not restore for a panic on any other
/// thread — `keeler-status` is read on one of its own, and a panic there is
/// no reason to pull the screen out from under a board that is still drawing
/// on it; that message belongs on the board. And it does not restore a
/// screen the guard has already given back, nor leave one for the guard to
/// give back after it: whichever of the two runs first is the one that runs.
pub fn restore_on_panic(restore: impl Fn() + Sync + Send + 'static) {
    let board = std::thread::current().id();
    let printed = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        if std::thread::current().id() == board && claimed() {
            restore();
        }
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
    fn a_screen_handed_over_and_taken_back_is_still_the_guards_to_give_back() {
        let done = Arc::new(Mutex::new(Vec::new()));

        {
            let mut guard = Guard::new(Recorder {
                done: Arc::clone(&done),
                refuses: false,
            })
            .expect("the recorder entered");
            let mut ran = false;
            guard.away(&mut || ran = true).expect("the recorder");
            assert!(ran, "the body never ran between the two");
            assert_eq!(
                *done.lock().expect("the recorder"),
                ["enter", "leave", "enter"]
            );
        }

        assert_eq!(
            *done.lock().expect("the recorder"),
            ["enter", "leave", "enter", "leave"],
            "the screen taken back at the end was not given back again",
        );
    }

    #[test]
    fn a_body_that_fell_over_leaves_the_screen_where_it_was_handed_to_it() {
        // tmux crashing, or a `q` that ends the board from inside the
        // attach: the screen is already the reader's, and a guard unwinding
        // past would send `ESC[?1049l` at a terminal that is not in the
        // alternate screen — which restores the cursor to where the last
        // entry saved it, over whatever has been printed since.
        let done = Arc::new(Mutex::new(Vec::new()));

        let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut guard = Guard::new(Recorder {
                done: Arc::clone(&done),
                refuses: false,
            })
            .expect("the recorder entered");
            guard.away(&mut || panic!("tmux fell over"))
        }));

        assert!(ended.is_err(), "the fixture did not panic");
        assert_eq!(
            *done.lock().expect("the recorder"),
            ["enter", "leave"],
            "a screen the board had already given back was given back twice",
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

    /// The two hooks these scenarios need: one standing in for the hook that
    /// prints, and the board's restore chained ahead of it. Both write to
    /// one list, so the order of everything that happens is one assertion.
    ///
    /// The hook is process-wide, and nextest gives every test a process of
    /// its own — which is what makes installing one here safe.
    fn hooked() -> Arc<Mutex<Vec<&'static str>>> {
        let order = Arc::new(Mutex::new(Vec::new()));
        let printing = Arc::clone(&order);
        std::panic::set_hook(Box::new(move |_| {
            printing.lock().expect("the order").push("printed");
        }));
        let restoring = Arc::clone(&order);
        restore_on_panic(move || {
            restoring.lock().expect("the order").push("restored");
        });
        order
    }

    /// What was recorded, read out before anything is asserted about it: the
    /// hooks above lock this list, and a failing `assert_eq!` holding the
    /// lock would panic into a hook that waits for it — a deadlock where a
    /// test failure should be.
    fn seen(order: &Arc<Mutex<Vec<&'static str>>>) -> Vec<&'static str> {
        order.lock().expect("the order").clone()
    }

    #[test]
    fn a_screen_the_hook_gave_back_is_not_given_back_again_by_the_guard() {
        let order = hooked();

        let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = Guard::new(Recorder {
                done: Arc::clone(&order),
                refuses: false,
            })
            .expect("the recorder entered");
            panic!("the board fell over");
        }));

        assert!(ended.is_err(), "the fixture did not panic");
        assert_eq!(
            seen(&order),
            ["enter", "restored", "printed"],
            "the screen was given back twice, the second time over the message",
        );
    }

    #[test]
    fn a_panic_on_another_thread_leaves_the_boards_screen_alone() {
        // `keeler-status` is read on a thread of its own. A panic there ends
        // that read; it does not end the board, and the board is still
        // drawing on the screen the message has to land on.
        let order = hooked();
        let _guard = Guard::new(Recorder {
            done: Arc::clone(&order),
            refuses: false,
        })
        .expect("the recorder entered");

        let read = std::thread::spawn(|| panic!("the recipe took the thread with it"));

        assert!(read.join().is_err(), "the fixture did not panic");
        assert_eq!(
            seen(&order),
            ["enter", "printed"],
            "a read that died took the board's screen with it",
        );
    }

    #[test]
    fn the_panic_hook_restores_the_terminal_before_the_message_is_printed() {
        let order = hooked();
        // No guard, so there is a screen for the hook to claim: the scenario
        // above is the other half, where the guard held it.
        super::IN_SCREEN.store(true, super::Ordering::SeqCst);

        let ended = std::panic::catch_unwind(|| panic!("the board fell over"));

        assert!(ended.is_err(), "the fixture did not panic");
        assert_eq!(
            seen(&order),
            ["restored", "printed"],
            "the message was printed onto a screen about to be thrown away",
        );
    }
}
