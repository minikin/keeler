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
    /// When the streams were last read, which is what the tick is due from.
    ticked: Timestamp,
    /// What the last read of the graph refused with, if it did.
    graph_said: Option<String>,
    /// And of the report.
    ///
    /// Two, rather than one line written over by whichever read spoke last,
    /// because they are asked on cadences five seconds apart: a graph
    /// refusal written into one line would be wiped by the report arriving,
    /// and a report refusal by the tick a second later. Each read owns its
    /// own sentence, and clears it when the read stops refusing.
    status_said: Option<String>,
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
            ticked: answered,
            graph_said: None,
            status_said: None,
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
    /// editor, which is the reading it was drawn to avoid — and a refusal
    /// that has gone takes its sentence with it, or one bad second would
    /// leave a line under the table for the rest of the session.
    pub fn tick(&mut self, now: Timestamp) {
        self.ticked = now;
        self.graph_said = match crate::graph::read(
            self.dispatch.as_ref(),
            &self.root,
            &self.status.git_ref,
            &self.status.rel,
        ) {
            Ok(graph) => {
                self.graph = graph;
                None
            }
            Err(refused) => Some(refused),
        };
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
    /// untrue, and the header's age says how long ago it was true — which is
    /// why `answered` moves on a report and not on a refusal. A board that
    /// blanked itself would throw away the one reading it still has.
    pub fn receive(&mut self, answer: &Result<String, String>, now: Timestamp) {
        let read = match answer {
            Ok(report) => crate::status::parse(report)
                .ok_or_else(|| "keeler-top: keeler-status printed no board to read.".to_string()),
            Err(refused) => Err(refused.clone()),
        };
        match read {
            Ok(status) => {
                self.status = status;
                self.status_said = None;
                let fresh = Board::assemble(&self.status, &self.graph, &mut self.runs, now);
                self.adopt(fresh);
            }
            Err(refused) => {
                self.status_said = Some(refused);
                self.board.message = self.said();
            }
        }
    }

    /// The line under the table: whatever the board has to say about its own
    /// reads, the report's refusal before the graph's — that is the slower
    /// read and the one whose absence costs more.
    fn said(&self) -> String {
        self.status_said
            .as_ref()
            .or(self.graph_said.as_ref())
            .cloned()
            .unwrap_or_default()
    }

    /// Puts a freshly assembled board in place of the one on screen, keeping
    /// the one thing a re-assembly knows nothing about — which row the
    /// person is looking at — and saying again whatever there is to say.
    ///
    /// The selection is clamped rather than kept, because the rows are the
    /// report's and the report can lose one — a task whose spec line was
    /// removed, or a graph read against a different ref.
    fn adopt(&mut self, fresh: Board) {
        let selected = self.board.selected.min(fresh.rows.len().saturating_sub(1));
        let message = self.said();
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
    /// When the last read came back, whatever it came back with.
    ///
    /// The feed's own clock, and not the board's `answered`. That one is the
    /// age of the last *report*, which is what the header is about and which
    /// a refusal must not reset — but a cadence measured from it would ask
    /// again on every pass for as long as the recipe kept refusing, which is
    /// a `just` a second on a machine that has just said it cannot answer.
    answered: Timestamp,
}

impl StatusFeed {
    /// A feed whose last answer is the one the board opened with.
    #[must_use]
    pub fn new(dispatch: Arc<dyn Dispatch>, answered: Timestamp) -> Self {
        Self {
            dispatch,
            pending: None,
            answered,
        }
    }

    /// Whether the cadence is due another read.
    #[must_use]
    pub fn due(&self, now: Timestamp) -> bool {
        status_due(now.seconds_since(self.answered), self.pending())
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
    pub fn take(&mut self, now: Timestamp) -> Option<Result<String, String>> {
        let answer = match self.pending.as_ref()?.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                Err("keeler-top: the keeler-status read ended without an answer.".to_string())
            }
        };
        self.pending = None;
        self.answered = now;
        Some(answer)
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

/// What a pass's wait ended with.
///
/// Three answers and not two, because the terminal sends more than keys and
/// a board that could not tell them apart would treat every one as the tick.
/// Dragging a window edge is dozens of resizes a second, and a tick is a
/// `git show`, a `bash`, and four `git` calls per task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Woke {
    /// A key was pressed.
    Key(KeyEvent),
    /// The terminal sent something else — a resize, a mouse, a paste. Worth
    /// the frame the next pass draws, and nothing else.
    Other,
    /// Nothing arrived: the wait ran out, which is the tick.
    Elapsed,
}

/// Where the board's keypresses come from.
pub trait Events {
    /// What the next `timeout` ends with.
    ///
    /// # Errors
    ///
    /// Whatever the terminal refused while it was being read.
    fn next(&mut self, timeout: Duration) -> Result<Woke, String>;
}

/// The real keyboard.
#[derive(Debug, Clone, Copy)]
pub struct Keys;

impl Events for Keys {
    // Outside the mutation gate, like [`waited`] under it and the three
    // calls in `terminal.rs`: crossterm reads the keyboard from the
    // controlling terminal, and a test process under nextest has none. The
    // one decision either of them makes — what an event counts as — was
    // moved into [`woke_of`], which is a function of an event and is tested.
    #[cfg_attr(test, mutants::skip)]
    fn next(&mut self, timeout: Duration) -> Result<Woke, String> {
        waited(timeout).map(|event| woke_of(event.as_ref()))
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

/// What one wait ended with: whatever the terminal sent, or the tick when it
/// sent nothing.
///
/// A press and not a release: Windows terminals report both, and a board
/// that read them alike would move the selection two rows for every `j`.
///
/// The whole of what [`Keys::next`] decides, here rather than there, because
/// there is the one place no test can reach.
fn woke_of(event: Option<&Event>) -> Woke {
    match event {
        Some(Event::Key(key)) if key.kind == KeyEventKind::Press => Woke::Key(*key),
        Some(_) => Woke::Other,
        None => Woke::Elapsed,
    }
}

/// Whether the streams are due another read.
///
/// A wait that ran out is a second with nothing in it, which is the tick as
/// the board has always meant it. The age is the other half, and it is what
/// keeps the columns moving when something else keeps waking the loop: a
/// finger held on `j` is a keypress every few milliseconds and a window
/// being dragged is a resize every few, and on either of them a board that
/// only ticked on the timeout would stop reading the streams altogether.
#[must_use]
pub fn tick_due(woke: Woke, since_tick: u64) -> bool {
    matches!(woke, Woke::Elapsed) || since_tick >= TICK.as_secs()
}

/// One pass of the loop: collect, ask, draw, wait, tick.
///
/// The order is the contract. Collecting first means an answer that arrived
/// while the board was waiting is on screen before the frame is drawn rather
/// than a second later. Drawing before waiting means the pass that ends in a
/// keypress has already shown what the last one did. And the tick comes
/// after the wait, so the bytes it reads are the ones that arrived during it
/// — the frame carrying them is the next pass's, one draw away.
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
    if let Some(answer) = feed.take(now) {
        app.receive(&answer, now);
    }
    if feed.due(now) {
        feed.ask();
    }
    surface.draw(&app.board, now)?;
    let woke = events.next(TICK)?;
    if tick_due(woke, now.seconds_since(app.ticked)) {
        app.tick(now);
    }
    let Woke::Key(key) = woke else {
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
    let mut feed = StatusFeed::new(dispatch, app.board.answered);
    looping(&mut terminal, &mut Keys, app, &mut feed)
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
    fn collected(feed: &mut StatusFeed, now: Timestamp) -> Result<String, String> {
        for _ in 0..1_000 {
            if let Some(answer) = feed.take(now) {
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
        let opened = Timestamp::from_epoch_seconds(1_000);
        let mut feed = StatusFeed::new(Arc::clone(&dispatch) as Arc<dyn Dispatch>, opened);
        assert!(!feed.pending());

        feed.ask();
        assert!(feed.pending());
        // Asking again while one is out is not a second recipe.
        feed.ask();
        let answer = collected(&mut feed, Timestamp::from_epoch_seconds(1_006));

        assert_eq!(answer, Ok("graph: s.md on HEAD\n".to_string()));
        assert!(
            !feed.pending(),
            "the answer arrived and the read stayed out"
        );
        assert_eq!(*dispatch.asked.lock().expect("the count"), 1);
    }

    #[test]
    fn a_read_whose_thread_died_is_a_refusal_and_not_a_board_waiting_for_ever() {
        let mut feed = StatusFeed::new(Arc::new(Panics), Timestamp::from_epoch_seconds(1_000));

        feed.ask();
        let answer = collected(&mut feed, Timestamp::from_epoch_seconds(1_006));

        assert!(answer.is_err(), "a thread that died answered the board");
        assert!(!feed.pending(), "the board went on waiting for a dead read");
    }

    #[test]
    fn a_read_that_refused_starts_the_cadence_again_rather_than_asking_at_once() {
        // The board's own `answered` is the age of the last *report*, and a
        // refusal is not one — so a cadence measured from it would find the
        // answer five seconds old on every pass and spawn a `just` a second
        // at a machine that has just said it cannot answer.
        let opened = Timestamp::from_epoch_seconds(1_000);
        let mut feed = StatusFeed::new(Arc::new(Panics), opened);
        assert!(!feed.due(Timestamp::from_epoch_seconds(1_004)));
        assert!(feed.due(Timestamp::from_epoch_seconds(1_005)));

        feed.ask();
        let refused = collected(&mut feed, Timestamp::from_epoch_seconds(1_005));

        assert!(refused.is_err(), "the fixture answered");
        assert!(
            !feed.due(Timestamp::from_epoch_seconds(1_009)),
            "a refusal was asked again before the cadence came round",
        );
        assert!(feed.due(Timestamp::from_epoch_seconds(1_010)));
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
    }

    #[test]
    fn a_refusal_that_has_gone_takes_its_sentence_with_it() {
        // A `git show` that lost a race, a spec uncommitted for a second
        // while somebody rebases: a line written under the table by one bad
        // read and never taken away is a board reporting, for the rest of
        // the session, something that stopped being true immediately.
        let mut app = app(THREE);

        app.receive(
            &Err("keeler-status: not a git repository".to_string()),
            Timestamp::from_epoch_seconds(1_001),
        );
        assert_eq!(app.board.message, "keeler-status: not a git repository");

        app.receive(&Ok(THREE.to_string()), Timestamp::from_epoch_seconds(1_002));

        assert_eq!(
            app.board.message, "",
            "the read stopped refusing and its sentence stayed under the table",
        );
    }

    #[test]
    fn the_report_refusing_is_not_wiped_by_the_tick_a_second_later() {
        // The two reads are five seconds apart, and this fixture's graph
        // read refuses too — it has no repository to read from. One line
        // written over by whichever spoke last would show a `keeler-status`
        // refusal for the one second before the next tick, and never again.
        let mut app = app(THREE);
        app.receive(
            &Err("keeler-status: not a git repository".to_string()),
            Timestamp::from_epoch_seconds(1_001),
        );

        app.tick(Timestamp::from_epoch_seconds(1_002));

        assert_eq!(
            app.board.message, "keeler-status: not a git repository",
            "the graph's refusal wrote over the report's, which is the slower read",
        );
    }

    #[test]
    fn the_streams_are_read_on_the_timeout_and_again_once_a_second_has_passed() {
        use super::{Woke, tick_due};

        // The tick as the board has always meant it: a second with nothing
        // in it.
        assert!(tick_due(Woke::Elapsed, 0));
        // And the half that keeps the columns moving under something that
        // wakes the loop faster than the timeout ever fires.
        assert!(!tick_due(Woke::Other, 0), "a resize storm was a tick each");
        assert!(!tick_due(Woke::Key(press(KeyCode::Char('j'))), 0));
        assert!(tick_due(Woke::Other, 1));
        assert!(tick_due(Woke::Key(press(KeyCode::Char('j'))), 1));
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
        fn next(&mut self, _timeout: Duration) -> Result<super::Woke, String> {
            Ok(super::Woke::Elapsed)
        }
    }

    #[test]
    fn a_release_is_not_a_press_and_nothing_else_is_a_key_at_all() {
        use super::Woke;
        use ratatui::crossterm::event::{Event, KeyEventKind};

        let pressed = press(KeyCode::Char('j'));
        assert_eq!(
            super::woke_of(Some(&Event::Key(pressed))),
            Woke::Key(pressed)
        );

        let released = KeyEvent {
            kind: KeyEventKind::Release,
            ..pressed
        };
        assert_eq!(
            super::woke_of(Some(&Event::Key(released))),
            Woke::Other,
            "one j moved the selection two rows",
        );
        // A resize is worth a frame and not a tick: dragging a window edge
        // is dozens a second, and a tick is a `git show`, a `bash` and four
        // `git` calls per task.
        assert_eq!(super::woke_of(Some(&Event::Resize(80, 24))), Woke::Other);
        // And nothing at all is the wait running out, which is the tick.
        assert_eq!(super::woke_of(None), Woke::Elapsed);
    }

    #[test]
    fn a_terminal_that_would_not_answer_says_which_half_of_it_did_not() {
        for (refused, half) in [
            (
                super::keyboard(&std::io::Error::other("no tty")),
                "keyboard",
            ),
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
        let mut feed = StatusFeed::new(
            Arc::new(Answers::default()),
            Timestamp::from_epoch_seconds(1_000),
        );

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
