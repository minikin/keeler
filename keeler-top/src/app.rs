//! The loop: one pass of it, what a key does, and the two cadences.
//!
//! A board that is watched rather than queried has to answer three
//! questions at once — what the streams say now, what `keeler-status` said
//! last, and what the person at the keyboard just asked for — and the three
//! run at different speeds. The streams are files, read in a millisecond, so
//! they are re-read every second. `keeler-status` is a `just` recipe, and
//! `just` costs about 2.4 s to start on this repository before the recipe's
//! own git queries begin: run inline it would freeze the board for longer
//! than the tick it was meant to fit inside, so it runs on a thread of its
//! own and its answer is collected whenever it turns up. The header shows
//! how old that answer is, which is the one honest thing a board with two
//! cadences can say about the slower one.
//!
//! **What is testable is separated from what is not, deliberately.**
//! [`on_key`] is a function of a board and a keypress. [`status_due`] is a
//! function of two numbers. [`step`] is one pass over a [`Surface`] and an
//! [`Events`], both of which a test can supply — so the loop itself is
//! driven by tests with a `TestBackend` and a scripted keyboard. What is
//! left, in [`run`], is a terminal, a guard and a `while`: no decisions, and
//! nothing a test could have driven anyway.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::board::{Board, Runs};
use crate::clock::Timestamp;
use crate::dispatch::Dispatch;
use crate::graph::GraphLine;
use crate::status::Status;

/// How long the board waits for a keypress before re-reading the streams.
///
/// It is both numbers at once: the longest a keypress waits to be acted on,
/// and how often a board nobody is touching refreshes. One second is the
/// second hand — the elapsed column counts in whole seconds, and a board
/// that refreshed more slowly would show one that visibly skipped.
pub const TICK: Duration = Duration::from_secs(1);

/// How old `keeler-status`'s answer may get before it is asked again.
pub const STATUS_AGE: u64 = 5;

/// What a keypress asked the board to do.
///
/// The keys the board acts on are few, and every one of them is here rather
/// than in the loop: the loop's job is to carry the answer out, and this is
/// what a test reads to know what a key meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Nothing the loop has to act on — the selection moved, or the key was
    /// not one of the board's.
    Nothing,
    /// Ask `keeler-status` again, now, without waiting for the cadence.
    Status,
    /// Leave.
    Quit,
}

/// Everything one pass of the loop reads and writes.
///
/// The two readers are here rather than rebuilt per pass, and that is what
/// makes a tick cheap: [`Runs`] holds how far into each stream the board has
/// got, so a refresh reads the bytes that arrived and none of the earlier
/// ones. The last report is held for the same reason — a tick re-assembles
/// the rows against the report it already has, which is what lets the two
/// cadences run at different speeds without the faster one waiting.
pub struct App {
    /// The board as the last pass left it.
    pub board: Board,
    /// The streams, one reader per task, kept between ticks.
    runs: Runs,
    /// The last report `keeler-status` gave.
    status: Status,
    /// The graph as the last tick read it.
    graph: Vec<GraphLine>,
    /// The project being watched — the graph is read from the ref that
    /// report named, in this repository.
    root: PathBuf,
    /// What the board runs outside itself.
    dispatch: Arc<dyn Dispatch>,
}

impl App {
    /// The board's state, from the first report and the first graph.
    #[must_use]
    pub fn new(
        dispatch: Arc<dyn Dispatch>,
        root: PathBuf,
        status: Status,
        graph: Vec<GraphLine>,
        answered: Timestamp,
    ) -> Self {
        let mut runs = Runs::default();
        let board = Board::assemble(&status, &graph, &mut runs, answered);
        Self {
            board,
            runs,
            status,
            graph,
            root,
            dispatch,
        }
    }

    /// One tick: the graph, then the streams, then the rows again.
    ///
    /// The graph is re-read here and not on the slow cadence because it is
    /// the cheap read of the two and the one that changes without the report
    /// changing — a task branch landing on the feature branch unblocks its
    /// dependents while `keeler-status` still calls them `not spawned`.
    ///
    /// A graph the script refused leaves the last one standing, with the
    /// refusal in the status line. The alternative is a board that empties
    /// its own state column the moment somebody has a spec open in an
    /// editor, which is the reading it was drawn to avoid.
    pub fn tick(&mut self) {
        match crate::graph::read(
            self.dispatch.as_ref(),
            &self.root,
            &self.status.git_ref,
            &self.status.rel,
        ) {
            Ok(graph) => self.graph = graph,
            Err(refused) => self.board.message = refused,
        }
        let fresh = Board::assemble(
            &self.status,
            &self.graph,
            &mut self.runs,
            self.board.answered,
        );
        self.adopt(fresh);
    }

    /// A report the slow read came back with, whatever it says.
    ///
    /// A refusal keeps the board that is there and shows the words in the
    /// status line: the recipe refusing does not make the last answer
    /// untrue, and the header's age says how long ago it was true. A board
    /// that blanked itself would throw away the one reading it still has.
    pub fn receive(&mut self, answer: &Result<String, String>, now: Timestamp) {
        let report = match answer {
            Ok(report) => report,
            Err(refused) => {
                self.board.message.clone_from(refused);
                return;
            }
        };
        let Some(status) = crate::status::parse(report) else {
            self.board.message = "keeler-top: keeler-status printed no board to read.".to_string();
            return;
        };
        self.status = status;
        let fresh = Board::assemble(&self.status, &self.graph, &mut self.runs, now);
        self.adopt(fresh);
    }

    /// Puts a freshly assembled board in place of the one on screen, keeping
    /// the two things a re-assembly knows nothing about: which row the
    /// person is looking at, and what the last keypress said.
    ///
    /// The selection is clamped rather than kept, because the rows are the
    /// report's and the report can lose one — a task whose spec line was
    /// removed, or a graph read against a different ref.
    fn adopt(&mut self, fresh: Board) {
        let selected = self.board.selected.min(fresh.rows.len().saturating_sub(1));
        let message = std::mem::take(&mut self.board.message);
        self.board = Board {
            selected,
            message,
            ..fresh
        };
    }

    /// Moves the detail pane to another row, as far as there are rows.
    fn select(&mut self, row: usize) {
        self.board.selected = row.min(self.board.rows.len().saturating_sub(1));
    }
}

/// What a keypress does.
///
/// Pure, and that is the point: every key the board answers to is decided
/// here, where a test presses one and reads the board afterwards, rather
/// than inside a loop that needs a terminal to run at all.
pub fn on_key(app: &mut App, key: KeyEvent) -> Action {
    let selected = app.board.selected;
    match key.code {
        // Ctrl-C is here because raw mode is: the terminal no longer turns
        // it into a signal, so a board that ignored it would be one the
        // most universal way of leaving anything does not leave.
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Action::Quit,
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Char('r') => Action::Status,
        KeyCode::Char('j') | KeyCode::Down => {
            app.select(selected.saturating_add(1));
            Action::Nothing
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select(selected.saturating_sub(1));
            Action::Nothing
        }
        _ => Action::Nothing,
    }
}

/// Whether `keeler-status` is due another read.
///
/// Not while one is out: a recipe that takes longer than the cadence would
/// otherwise be asked again before it had answered, and a board that spawned
/// a `just` a second for a machine already too slow to answer in five is a
/// board making its own problem worse.
#[must_use]
pub fn status_due(age: u64, pending: bool) -> bool {
    !pending && age >= STATUS_AGE
}

/// `keeler-status`, asked on a thread of its own.
///
/// One read at a time, collected without waiting. The recipe is seconds
/// where a tick is one, so the board asks, keeps drawing, and takes the
/// answer on whichever pass it has arrived by.
pub struct StatusFeed {
    dispatch: Arc<dyn Dispatch>,
    pending: Option<Receiver<Result<String, String>>>,
}

impl StatusFeed {
    /// A feed that has not asked anything yet.
    #[must_use]
    pub fn new(dispatch: Arc<dyn Dispatch>) -> Self {
        Self {
            dispatch,
            pending: None,
        }
    }

    /// Asks again, unless a read is already out.
    pub fn ask(&mut self) {
        if self.pending.is_some() {
            return;
        }
        let (sender, receiver) = channel();
        let dispatch = Arc::clone(&self.dispatch);
        std::thread::spawn(move || {
            // Nobody is left to tell when the send fails: the board quit
            // while the recipe was still running, which is a thread with
            // nothing to do but end.
            let _ = sender.send(dispatch.status());
        });
        self.pending = Some(receiver);
    }

    /// Whether a read is out, and the board is waiting for it.
    #[must_use]
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Whatever has come back since the last look — never a wait.
    ///
    /// A channel that closed without an answer is a refusal rather than
    /// nothing: the read's thread panicked, and a feed that went on calling
    /// that pending would never ask again for as long as the board was up.
    pub fn take(&mut self) -> Option<Result<String, String>> {
        match self.pending.as_ref()?.try_recv() {
            Ok(answer) => {
                self.pending = None;
                Some(answer)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                Some(Err(
                    "keeler-top: the keeler-status read ended without an answer.".to_string(),
                ))
            }
        }
    }
}

/// What a pass of the loop draws on.
///
/// A trait rather than the terminal itself so that the loop is one function
/// and not one per backend: the real board draws on a terminal and the tests
/// draw on ratatui's `TestBackend`, and the pass between them is the same
/// code either way.
pub trait Surface {
    /// Draws one frame of the board.
    ///
    /// # Errors
    ///
    /// Whatever the terminal refused.
    fn draw(&mut self, board: &Board, now: Timestamp) -> Result<(), String>;
}

impl<B: Backend> Surface for Terminal<B> {
    fn draw(&mut self, board: &Board, now: Timestamp) -> Result<(), String> {
        Terminal::draw(self, |frame| crate::frame::render(frame, board, now))
            .map(drop)
            .map_err(|err| format!("keeler-top: drawing the board: {err}"))
    }
}

/// Where the board's keypresses come from.
pub trait Events {
    /// The next keypress, or nothing when `timeout` passed without one.
    ///
    /// # Errors
    ///
    /// Whatever the terminal refused while it was being read.
    fn next(&mut self, timeout: Duration) -> Result<Option<KeyEvent>, String>;
}

/// The real keyboard.
#[derive(Debug, Clone, Copy)]
pub struct Keys;

impl Events for Keys {
    // Outside the mutation gate, like [`waited`] under it and the three
    // calls in `terminal.rs`: crossterm reads the keyboard from the
    // controlling terminal, and a test process under nextest has none. The
    // one decision either of them makes — what counts as a keypress — was
    // moved into [`press`], which is a function of an event and is tested.
    #[cfg_attr(test, mutants::skip)]
    fn next(&mut self, timeout: Duration) -> Result<Option<KeyEvent>, String> {
        Ok(waited(timeout)?.as_ref().and_then(press))
    }
}

/// The next terminal event, or nothing when the timeout ran out first.
#[cfg_attr(test, mutants::skip)]
fn waited(timeout: Duration) -> Result<Option<Event>, String> {
    if event::poll(timeout).map_err(|err| keyboard(&err))? {
        return event::read().map(Some).map_err(|err| keyboard(&err));
    }
    Ok(None)
}

/// A terminal that could not be read from.
fn keyboard(err: &std::io::Error) -> String {
    format!("keeler-top: reading the keyboard: {err}")
}

/// The keypress in a terminal event, if it is one.
///
/// A press and not a release: Windows terminals report both, and a board
/// that read them alike would move the selection two rows for every `j`.
/// Everything else a terminal sends — a resize, a mouse, a paste — is no
/// keypress, and reaches the loop as the timeout does: one more pass, one
/// more frame, drawn at whatever size the window now is.
fn press(event: &Event) -> Option<KeyEvent> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => Some(*key),
        _ => None,
    }
}

/// One pass of the loop: collect, ask, draw, wait.
///
/// The order is the contract. Collecting first means an answer that arrived
/// while the board was waiting is on screen before the frame is drawn rather
/// than a second later. Drawing before waiting means the pass that ends in a
/// keypress has already shown what the last one did. And the wait is the
/// tick: a second with nothing in it is a second's worth of new stream
/// bytes, which is what the board is for.
///
/// # Errors
///
/// A terminal that could not be drawn on or read from. Both end the board —
/// with the guard dropped on the way out, so the terminal is the reader's
/// again before the message reaches it.
pub fn step(
    surface: &mut dyn Surface,
    events: &mut dyn Events,
    app: &mut App,
    feed: &mut StatusFeed,
    now: Timestamp,
) -> Result<bool, String> {
    if let Some(answer) = feed.take() {
        app.receive(&answer, now);
    }
    if status_due(now.seconds_since(app.board.answered), feed.pending()) {
        feed.ask();
    }
    surface.draw(&app.board, now)?;
    let Some(key) = events.next(TICK)? else {
        app.tick();
        return Ok(true);
    };
    match on_key(app, key) {
        Action::Quit => Ok(false),
        Action::Status => {
            feed.ask();
            Ok(true)
        }
        Action::Nothing => Ok(true),
    }
}

/// Passes until one of them says to stop.
///
/// # Errors
///
/// Whatever ended a pass.
pub fn looping(
    surface: &mut dyn Surface,
    events: &mut dyn Events,
    app: &mut App,
    feed: &mut StatusFeed,
) -> Result<(), String> {
    while step(surface, events, app, feed, Timestamp::now())? {}
    Ok(())
}

/// The live board: the terminal, the guard around it, and the loop.
///
/// Everything this does that could hold a mistake has been moved out of it.
/// What is left is the three lines no test can run — a real terminal in raw
/// mode, and a panic hook that has to be installed process-wide to be worth
/// anything.
///
/// # Errors
///
/// A terminal that could not be entered, drawn on or read from.
///
/// Outside the mutation gate, and the last of the six that are: every line
/// of it is a real terminal or the process-wide hook that has to be
/// installed to be worth anything. What it composes is tested — [`looping`]
/// over a `TestBackend` and a scripted keyboard, [`Guard`] over a recording
/// screen, [`crate::terminal::restore_on_panic`] over a recording hook — and
/// what is left here is the wiring between them.
#[cfg_attr(test, mutants::skip)]
pub fn run(app: &mut App, dispatch: Arc<dyn Dispatch>) -> Result<(), String> {
    crate::terminal::restore_on_panic(|| drop(crate::terminal::restore()));
    let _guard = crate::terminal::Guard::new(crate::terminal::Tty).map_err(|err| screen(&err))?;
    let mut terminal = Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))
        .map_err(|err| screen(&err))?;
    looping(
        &mut terminal,
        &mut Keys,
        app,
        &mut StatusFeed::new(dispatch),
    )
}

/// A terminal the board could not take over.
fn screen(err: &std::io::Error) -> String {
    format!("keeler-top: the terminal: {err}")
}

#[cfg(test)]
mod tests {
    use super::{Action, App, Events, StatusFeed, Surface, on_key, status_due};
    use crate::board::Board;
    use crate::clock::Timestamp;
    use crate::dispatch::Dispatch;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// A dispatch that answers with what it was given, and counts.
    #[derive(Debug, Default)]
    struct Answers {
        report: String,
        asked: Mutex<usize>,
    }

    impl Dispatch for Answers {
        fn status(&self) -> Result<String, String> {
            *self.asked.lock().expect("the count") += 1;
            Ok(self.report.clone())
        }

        fn graph(&self, _spec: &Path) -> Result<String, String> {
            Ok(String::new())
        }
    }

    /// A dispatch whose reads end without answering, which is what a thread
    /// that panicked leaves behind.
    #[derive(Debug)]
    struct Panics;

    impl Dispatch for Panics {
        fn status(&self) -> Result<String, String> {
            panic!("the recipe took the thread with it");
        }

        fn graph(&self, _spec: &Path) -> Result<String, String> {
            Ok(String::new())
        }
    }

    /// A board over a report, with no graph and no runs to read.
    fn app(report: &str) -> App {
        let status = crate::status::parse(report).expect("the fixture's report has a header");
        App::new(
            Arc::new(Answers::default()),
            PathBuf::from("/nowhere"),
            status,
            Vec::new(),
            Timestamp::from_epoch_seconds(1_000),
        )
    }

    /// The report the key tests move about in.
    const THREE: &str = "graph: s.md on HEAD\nT1     done\nT2     done\nT3     done\n";

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn j_and_k_walk_the_rows_and_stop_at_both_ends() {
        let mut app = app(THREE);

        assert_eq!(on_key(&mut app, press(KeyCode::Char('j'))), Action::Nothing);
        assert_eq!(app.board.selected, 1);
        on_key(&mut app, press(KeyCode::Char('j')));
        assert_eq!(app.board.selected, 2);
        // The last row is the last row: a board that wrapped would put the
        // pane somewhere the eye did not follow it to.
        on_key(&mut app, press(KeyCode::Char('j')));
        assert_eq!(app.board.selected, 2);

        on_key(&mut app, press(KeyCode::Char('k')));
        assert_eq!(app.board.selected, 1);
        on_key(&mut app, press(KeyCode::Char('k')));
        on_key(&mut app, press(KeyCode::Char('k')));
        assert_eq!(app.board.selected, 0);
    }

    #[test]
    fn the_arrows_move_the_selection_as_the_letters_do() {
        let mut app = app(THREE);

        on_key(&mut app, press(KeyCode::Down));
        assert_eq!(app.board.selected, 1);
        on_key(&mut app, press(KeyCode::Up));
        assert_eq!(app.board.selected, 0);
    }

    #[test]
    fn a_board_with_no_rows_has_nowhere_to_move_to() {
        let mut app = app("graph: s.md on HEAD\n");

        on_key(&mut app, press(KeyCode::Char('j')));

        assert_eq!(app.board.selected, 0, "the pane left the board");
    }

    #[test]
    fn the_keys_that_do_something_are_the_only_ones_that_do() {
        let mut app = app(THREE);

        assert_eq!(on_key(&mut app, press(KeyCode::Char('q'))), Action::Quit);
        assert_eq!(
            on_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            ),
            Action::Quit,
            "raw mode swallowed the signal and the board ignored the key",
        );
        // Without the modifier it is a letter like any other.
        assert_eq!(on_key(&mut app, press(KeyCode::Char('c'))), Action::Nothing);
        assert_eq!(on_key(&mut app, press(KeyCode::Char('r'))), Action::Status);
        assert_eq!(on_key(&mut app, press(KeyCode::Esc)), Action::Nothing);
        assert_eq!(on_key(&mut app, press(KeyCode::Char('x'))), Action::Nothing);
    }

    #[test]
    fn the_report_is_asked_for_again_at_five_seconds_and_never_while_one_is_out() {
        assert!(!status_due(4, false), "asked before the answer was stale");
        assert!(status_due(5, false));
        assert!(status_due(500, false));
        assert!(
            !status_due(500, true),
            "a slow recipe was asked a second time",
        );
    }

    /// The feed's answer, waited for the way a test may wait for one:
    /// bounded, so a feed that never answers fails here rather than holding
    /// the suite open until something else times it out.
    fn collected(feed: &mut StatusFeed) -> Result<String, String> {
        for _ in 0..1_000 {
            if let Some(answer) = feed.take() {
                return answer;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("the feed never answered");
    }

    #[test]
    fn a_feeds_answer_is_collected_without_ever_waiting_for_it() {
        let dispatch = Arc::new(Answers {
            report: "graph: s.md on HEAD\n".to_string(),
            asked: Mutex::new(0),
        });
        let mut feed = StatusFeed::new(Arc::clone(&dispatch) as Arc<dyn Dispatch>);
        assert!(!feed.pending());

        feed.ask();
        assert!(feed.pending());
        // Asking again while one is out is not a second recipe.
        feed.ask();
        let answer = collected(&mut feed);

        assert_eq!(answer, Ok("graph: s.md on HEAD\n".to_string()));
        assert!(
            !feed.pending(),
            "the answer arrived and the read stayed out"
        );
        assert_eq!(*dispatch.asked.lock().expect("the count"), 1);
    }

    #[test]
    fn a_read_whose_thread_died_is_a_refusal_and_not_a_board_waiting_for_ever() {
        let mut feed = StatusFeed::new(Arc::new(Panics));

        feed.ask();
        let answer = collected(&mut feed);

        assert!(answer.is_err(), "a thread that died answered the board");
        assert!(!feed.pending(), "the board went on waiting for a dead read");
    }

    #[test]
    fn a_report_the_board_cannot_read_leaves_the_one_it_had() {
        let mut app = app(THREE);
        app.board.selected = 2;
        let before = app.board.rows.clone();

        app.receive(
            &Ok("cargo: could not compile keeler-top".to_string()),
            Timestamp::from_epoch_seconds(2_000),
        );

        assert_eq!(app.board.rows, before, "the board threw away its own rows");
        assert_eq!(
            app.board.answered,
            Timestamp::from_epoch_seconds(1_000),
            "an answer that never came reset the header's age",
        );
        assert!(!app.board.message.is_empty(), "the refusal was not shown");
    }

    #[test]
    fn a_recipe_that_refused_is_relayed_in_its_own_words() {
        let mut app = app(THREE);

        app.receive(
            &Err("keeler-status: not a git repository".to_string()),
            Timestamp::from_epoch_seconds(2_000),
        );

        assert_eq!(app.board.message, "keeler-status: not a git repository");
        assert_eq!(app.board.rows.len(), 3);
    }

    #[test]
    fn a_report_that_lost_a_task_brings_the_selection_back_with_it() {
        let mut app = app(THREE);
        app.board.selected = 2;
        app.board.message = "something the last key said".to_string();

        app.receive(
            &Ok("graph: s.md on HEAD\nT1     done\n".to_string()),
            Timestamp::from_epoch_seconds(2_000),
        );

        assert_eq!(app.board.rows.len(), 1);
        assert_eq!(
            app.board.selected, 0,
            "the pane is about a row that is gone"
        );
        assert_eq!(
            app.board.answered,
            Timestamp::from_epoch_seconds(2_000),
            "the header's age did not reset on a fresh answer",
        );
        assert_eq!(
            app.board.message, "something the last key said",
            "the status line was cleared by a refresh nobody asked for",
        );
    }

    /// A surface that keeps what it was asked to draw.
    #[derive(Debug, Default)]
    struct Frames(Vec<Board>);

    impl Surface for Frames {
        fn draw(&mut self, board: &Board, _now: Timestamp) -> Result<(), String> {
            self.0.push(board.clone());
            Ok(())
        }
    }

    /// A keyboard nobody is at: every wait times out.
    struct Idle;

    impl Events for Idle {
        fn next(&mut self, _timeout: Duration) -> Result<Option<KeyEvent>, String> {
            Ok(None)
        }
    }

    #[test]
    fn a_release_is_not_a_press_and_nothing_else_is_a_key_at_all() {
        use ratatui::crossterm::event::{Event, KeyEventKind};

        let pressed = press(KeyCode::Char('j'));
        assert_eq!(super::press(&Event::Key(pressed)), Some(pressed));

        let released = KeyEvent {
            kind: KeyEventKind::Release,
            ..pressed
        };
        assert_eq!(
            super::press(&Event::Key(released)),
            None,
            "one j moved the selection two rows",
        );
        // A resize is a pass of its own, and a pass with no key in it is the
        // tick — which is a redraw at whatever size the window now is.
        assert_eq!(super::press(&Event::Resize(80, 24)), None);
    }

    #[test]
    fn a_terminal_that_would_not_answer_says_which_half_of_it_did_not() {
        for (refused, half) in [
            (super::keyboard(&std::io::Error::other("no tty")), "keyboard"),
            (super::screen(&std::io::Error::other("no tty")), "terminal"),
        ] {
            assert!(
                refused.starts_with("keeler-top: ")
                    && refused.contains(half)
                    && refused.contains("no tty"),
                "the refusal names neither {half} nor what it said: {refused}",
            );
        }
    }

    #[test]
    fn a_pass_draws_before_it_waits() {
        // Which is what makes the board's answer to a keypress visible when
        // the next key is pressed rather than a second afterwards.
        let mut app = app(THREE);
        let mut frames = Frames::default();
        let mut feed = StatusFeed::new(Arc::new(Answers::default()));

        let carry_on = super::step(
            &mut frames,
            &mut Idle,
            &mut app,
            &mut feed,
            Timestamp::from_epoch_seconds(1_000),
        );

        assert_eq!(carry_on, Ok(true));
        assert_eq!(frames.0.len(), 1);
        assert_eq!(frames.0[0].rows.len(), 3);
    }
}
