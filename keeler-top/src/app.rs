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
use crate::graph::Graph;
use crate::status::Status;
use crate::terminal::{Guard, Screen};
use crate::theme::Theme;

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
    /// Stop the selected task's session, and mark that somebody meant to.
    Pause,
    /// Hand the selected task back to `keeler-resume`.
    Resume,
    /// Put the watcher in front of the selected task's session.
    Attach,
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
    /// The colours and glyphs this terminal asked for, read once in `main`
    /// and carried here because this is what the loop draws from — a theme
    /// re-read per frame would be the process's environment answering a
    /// question the renderer is meant to be handed the answer to.
    pub theme: Theme,
    /// The streams, one reader per task, kept between ticks.
    runs: Runs,
    /// The last report `keeler-status` gave.
    status: Status,
    /// The graph as the last tick read it, and the titles beside it.
    graph: Graph,
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
    /// And what the last keypress had to say: the lever that refused, or
    /// the reason nothing happened.
    ///
    /// A third sentence for the same reason there are two — a line written
    /// straight into the frame would be gone within the second. Every tick
    /// re-assembles the board, and a `keeler-resume` refusal that lasted
    /// one frame is a refusal nobody read. This one is cleared by the next
    /// keypress and by nothing else, and it is shown before either read's:
    /// those are the board talking about itself, and this is the answer to
    /// something the person did a moment ago.
    key_said: Option<String>,
}

impl App {
    /// The board's state, from the first report and the first graph.
    #[must_use]
    pub fn new(
        dispatch: Arc<dyn Dispatch>,
        root: PathBuf,
        status: Status,
        graph: Graph,
        answered: Timestamp,
        theme: Theme,
    ) -> Self {
        let mut runs = Runs::default();
        let board = Board::assemble(&status, &graph, &mut runs, answered);
        Self {
            board,
            theme,
            runs,
            status,
            graph,
            root,
            dispatch,
            ticked: answered,
            graph_said: None,
            status_said: None,
            key_said: None,
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

    /// The line under the table: whatever the board has to say.
    ///
    /// The keypress first, because it is the answer to something somebody
    /// just did; then the report's refusal before the graph's — that is the
    /// slower read and the one whose absence costs more.
    fn said(&self) -> String {
        self.key_said
            .as_ref()
            .or(self.status_said.as_ref())
            .or(self.graph_said.as_ref())
            .cloned()
            .unwrap_or_default()
    }

    /// What a keypress had to say, kept until the next one has something of
    /// its own — `None` for a lever that did what it was asked, which puts
    /// whatever the two reads have to say back on the line.
    fn says(&mut self, said: Option<String>) {
        self.key_said = said;
        self.board.message = self.said();
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

    /// `p`: stops the selected task's session, and marks that somebody
    /// meant it.
    ///
    /// **Kill first, then mark.** A marker written before a kill that fails
    /// would be a claim the board cannot back: `keeler-status` would read
    /// `paused` off a session still running, and the wave would leave the
    /// task out for a reason that was never true. Between the kill and the
    /// next status read the row may say `died` for one refresh — the marker
    /// settles it on the read after, and that is the honest order of the
    /// two facts.
    pub fn pause(&mut self) {
        let Some(row) = self.board.selected_row() else {
            self.says(Some(NO_ROW.to_string()));
            return;
        };
        if !row.running() {
            let id = row.id.clone();
            self.says(Some(format!(
                "keeler-top: {id} is not running — there is nothing to pause."
            )));
            return;
        }
        let id = row.id.clone();
        let session = row.session(self.board.slug());
        let Some(marker) = row.marker() else {
            self.says(Some(format!(
                "keeler-top: {id}'s report names no log to write the marker beside."
            )));
            return;
        };
        let said = match self.dispatch.kill(&session) {
            Err(refused) => refused,
            Ok(()) => match std::fs::write(&marker, "") {
                Ok(()) => format!("keeler-top: {id} paused — R resumes it."),
                Err(err) => format!("keeler-top: {}: {err}", marker.display()),
            },
        };
        self.says(Some(said));
    }

    /// `R`: hands the selected task to `keeler-resume`.
    ///
    /// No gate of its own, deliberately. The recipe refuses a task still
    /// running, one that reached its gate, one that is done and one never
    /// spawned, in sentences written for whoever has to act on them — and a
    /// board with a second opinion about resumability would be a second
    /// answer to disagree with.
    ///
    /// # Errors
    ///
    /// A terminal the waiting board could not be drawn on.
    pub fn resume(&mut self, surface: &mut dyn Surface, now: Timestamp) -> Result<(), String> {
        let Some(row) = self.board.selected_row() else {
            self.says(Some(NO_ROW.to_string()));
            return Ok(());
        };
        let id = row.id.clone();
        // Said and drawn before the recipe runs. `keeler-resume` is a `just`
        // that runs a second `just`, a graph read and a `tmux new-session` —
        // seconds, on the thread the board draws from, where every other
        // pass is one. A board that went still and said nothing is one whose
        // watcher presses R again.
        self.says(Some(format!("keeler-top: resuming {id}…")));
        surface.draw(&self.board, self.theme, now)?;
        let (Ok(said) | Err(said)) = self.dispatch.resume(&id);
        self.says(Some(sentence(&said)));
        Ok(())
    }

    /// `Enter`: puts the watcher in front of the selected task's session.
    ///
    /// # Errors
    ///
    /// A screen that could not be given back, or could not be taken again —
    /// which ends the board, the guard restoring the terminal on the way
    /// out. tmux's own refusals are not that: they are a sentence in the
    /// status line and a board still up.
    pub fn attach(&mut self, surface: &mut dyn Surface, now: Timestamp) -> Result<(), String> {
        let Some(row) = self.board.selected_row() else {
            self.says(Some(NO_ROW.to_string()));
            return Ok(());
        };
        if !row.running() {
            let id = row.id.clone();
            self.says(Some(format!("keeler-top: {id} has no session to attach.")));
            return Ok(());
        }
        let session = row.session(self.board.slug());
        // Inside tmux the board is one client of a server that already has
        // the terminal: the client moves to the session and the board keeps
        // drawing on the screen it has. Giving that screen back for a
        // `switch-client` would be a board that blinked for no reason.
        if self.dispatch.in_tmux() {
            let said = refusal(self.dispatch.attach(&session, true));
            self.says(said);
            return Ok(());
        }
        let mut said = None;
        surface.away(&mut || said = refusal(self.dispatch.attach(&session, false)))?;
        // Whatever the run did while nobody was reading the board is the
        // first thing the frame after has to carry — an attach is minutes,
        // where every other pass of this loop is a second.
        self.tick(now);
        self.says(said);
        Ok(())
    }
}

/// What the status line says about a keypress that named no task, which is
/// a board with no rows at all — a spec whose tasks are still to be written.
const NO_ROW: &str = "keeler-top: there is no task on the board to act on.";

/// What a recipe said, cut to the one line a status line has room for.
///
/// The first sentence and not the last: `keeler-resume` prints what it did
/// and then the worktree, the session and the board under it, and what
/// happened is the line that says so.
fn sentence(said: &str) -> String {
    said.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim_end()
        .to_string()
}

/// The reason a lever gave, or nothing when it did what it was asked.
///
/// Silence on success is the point: the board has just come back from tmux
/// or moved a client, and a line under the table saying so would be the
/// board reporting its own success at what the watcher just watched happen.
fn refusal(answer: Result<(), String>) -> Option<String> {
    answer.err()
}

/// What a keypress does.
///
/// Pure, and that is the point: every key the board answers to is decided
/// here, where a test presses one and reads the board afterwards, rather
/// than inside a loop that needs a terminal to run at all.
pub fn on_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        // Ctrl-C is here because raw mode is: the terminal no longer turns
        // it into a signal, so a board that ignored it would be one the
        // most universal way of leaving anything does not leave.
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Action::Quit,
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Char('r') => Action::Status,
        // The three levers, and the only keys here that carry a guard:
        // they are the ones that do something outside the board, and
        // Ctrl-P is a chord half the world has bound to "previous". The
        // keys above and below move a cursor or end a session the person
        // is looking at; this one kills a running agent.
        KeyCode::Char('p') if plain(key) => Action::Pause,
        // Shifted, as `keeler-resume` is the heavier of the two: `p` stops
        // a run that can be started again, and `R` starts an agent.
        KeyCode::Char('R') if plain(key) => Action::Resume,
        KeyCode::Enter if plain(key) => Action::Attach,
        // Down and up the board as it is drawn, which is not the order the
        // report listed the tasks in — `Board::moved` says why.
        KeyCode::Char('j') | KeyCode::Down => {
            app.select(app.board.moved(true));
            Action::Nothing
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select(app.board.moved(false));
            Action::Nothing
        }
        _ => Action::Nothing,
    }
}

/// Whether a key was pressed on its own.
///
/// Shift aside, which is how `R` is typed at all — and which no terminal
/// turns into a chord of its own.
fn plain(key: KeyEvent) -> bool {
    key.modifiers.difference(KeyModifiers::SHIFT).is_empty()
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
    /// Draws one frame of the board, in the colours and glyphs this
    /// terminal asked for.
    ///
    /// The theme travels with the board rather than living on the surface:
    /// it is a value read once in `main`, and a surface that held one would
    /// be a second place for a test to have to arrange it.
    ///
    /// # Errors
    ///
    /// Whatever the terminal refused.
    fn draw(&mut self, board: &Board, theme: Theme, now: Timestamp) -> Result<(), String>;

    /// Hands the terminal to something else for as long as `body` runs, and
    /// takes it back after.
    ///
    /// # Errors
    ///
    /// A screen that would not be given back or taken again.
    fn away(&mut self, body: &mut dyn FnMut()) -> Result<(), String>;
}

impl<B: Backend> Surface for Terminal<B> {
    fn draw(&mut self, board: &Board, theme: Theme, now: Timestamp) -> Result<(), String> {
        Terminal::draw(self, |frame| crate::frame::render(frame, board, theme, now))
            .map(drop)
            .map_err(|err| format!("keeler-top: drawing the board: {err}"))
    }

    /// A terminal holding no screen of its own has nothing to give back —
    /// but what ran on it drew over it all the same.
    fn away(&mut self, body: &mut dyn FnMut()) -> Result<(), String> {
        body();
        redrawn(self)
    }
}

/// The board's own terminal: the screen it holds, and the buffer it draws
/// on.
///
/// The two are one thing here because `Enter` needs them together. Handing
/// the terminal to tmux is giving the screen back, and taking it again is
/// not enough by itself: the frame after is drawn onto a screen tmux has
/// written all over, and what the board knows about that screen is what it
/// drew on it a minute ago.
pub struct Live<S: Screen, B: Backend> {
    terminal: Terminal<B>,
    guard: Guard<S>,
}

impl<S: Screen, B: Backend> Live<S, B> {
    /// A board drawing on the screen it holds.
    #[must_use]
    pub fn new(terminal: Terminal<B>, guard: Guard<S>) -> Self {
        Self { terminal, guard }
    }
}

impl<S: Screen, B: Backend> Surface for Live<S, B> {
    fn draw(&mut self, board: &Board, theme: Theme, now: Timestamp) -> Result<(), String> {
        Surface::draw(&mut self.terminal, board, theme, now)
    }

    fn away(&mut self, body: &mut dyn FnMut()) -> Result<(), String> {
        self.guard.away(body).map_err(|err| screen(&err))?;
        redrawn(&mut self.terminal)
    }
}

/// Throws away what the board last drew, so the next frame is drawn whole.
///
/// ratatui draws by difference against the frame before it, and whatever
/// had the terminal while the board was away wrote over that frame without
/// telling it. Without this the board would come back and redraw the few
/// cells that had changed, onto somebody else's output.
fn redrawn<B: Backend>(terminal: &mut Terminal<B>) -> Result<(), String> {
    terminal
        .clear()
        .map_err(|err| format!("keeler-top: drawing the board: {err}"))
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
    surface.draw(&app.board, app.theme, now)?;
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
        Action::Pause => {
            app.pause();
            Ok(true)
        }
        Action::Resume => app.resume(surface, now).map(|()| true),
        Action::Attach => app.attach(surface, now).map(|()| true),
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
    let guard = crate::terminal::Guard::new(crate::terminal::Tty).map_err(|err| screen(&err))?;
    let terminal = Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))
        .map_err(|err| screen(&err))?;
    let mut feed = StatusFeed::new(dispatch, app.board.answered);
    // The guard goes into the surface rather than beside it: `Enter` hands
    // the terminal to tmux, which is the screen and the buffer together.
    looping(&mut Live::new(terminal, guard), &mut Keys, app, &mut feed)
}

/// A terminal the board could not take over.
fn screen(err: &std::io::Error) -> String {
    format!("keeler-top: the terminal: {err}")
}

#[cfg(test)]
mod tests {
    use super::{Action, App, Events, Live, StatusFeed, Surface, on_key, status_due};
    use crate::board::Board;
    use crate::clock::Timestamp;
    use crate::dispatch::Dispatch;
    use crate::terminal::{Guard, Screen};
    use crate::theme::Theme;
    use ratatui::Terminal;
    use ratatui::backend::Backend as _;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// A dispatch that answers with what it was given, and keeps what it
    /// was asked.
    #[derive(Debug, Default)]
    struct Answers {
        report: String,
        asked: Mutex<usize>,
        /// Every lever that was pulled, in order.
        done: Mutex<Vec<String>>,
        /// What a lever refuses with, for the tests that want a refusal.
        refuses: Option<String>,
        /// What `keeler-resume` printed when it did not refuse.
        said: String,
        /// Whether the board is drawing inside tmux.
        inside: bool,
        /// Whether the read ends without answering, which is what a thread
        /// that panicked leaves behind.
        panics: bool,
    }

    impl Answers {
        fn note(&self, what: &str) {
            self.done.lock().expect("the levers").push(what.to_string());
        }

        fn done(&self) -> Vec<String> {
            self.done.lock().expect("the levers").clone()
        }

        /// What a lever that answers nothing gives back.
        fn answered(&self) -> Result<(), String> {
            self.refuses.clone().map_or(Ok(()), Err)
        }
    }

    impl Dispatch for Answers {
        fn status(&self) -> Result<String, String> {
            assert!(!self.panics, "the recipe took the thread with it");
            *self.asked.lock().expect("the count") += 1;
            Ok(self.report.clone())
        }

        fn graph(&self, _spec: &Path) -> Result<String, String> {
            Ok(String::new())
        }

        fn kill(&self, session: &str) -> Result<(), String> {
            self.note(&format!("kill {session}"));
            self.answered()
        }

        fn resume(&self, task: &str) -> Result<String, String> {
            self.note(&format!("resume {task}"));
            self.refuses
                .clone()
                .map_or_else(|| Ok(self.said.clone()), Err)
        }

        fn in_tmux(&self) -> bool {
            self.inside
        }

        fn attach(&self, session: &str, inside: bool) -> Result<(), String> {
            self.note(&format!("attach {session} inside={inside}"));
            self.answered()
        }
    }

    /// A board over a report, with no graph and no runs to read.
    fn app(report: &str) -> App {
        over(Arc::new(Answers::default()), report)
    }

    /// The same board, over answers a scenario has arranged.
    fn over(dispatch: Arc<Answers>, report: &str) -> App {
        let status = crate::status::parse(report).expect("the fixture's report has a header");
        App::new(
            dispatch,
            PathBuf::from("/nowhere"),
            status,
            crate::graph::Graph::default(),
            Timestamp::from_epoch_seconds(1_000),
            // Said outright rather than read from the process: the suite
            // runs its tests in threads of one process, and a theme taken
            // from the environment would be whatever the machine running
            // them happens to export.
            Theme::new(true, false),
        )
    }

    /// The theme every board below is drawn through, said outright for the
    /// reason [`over`] gives.
    const THEME: Theme = Theme::new(true, false);

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
            ..Answers::default()
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
        let mut feed = StatusFeed::new(
            Arc::new(Answers {
                panics: true,
                ..Answers::default()
            }),
            Timestamp::from_epoch_seconds(1_000),
        );

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
        let mut feed = StatusFeed::new(
            Arc::new(Answers {
                panics: true,
                ..Answers::default()
            }),
            opened,
        );
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
        fn draw(&mut self, board: &Board, _theme: Theme, _now: Timestamp) -> Result<(), String> {
            self.0.push(board.clone());
            Ok(())
        }

        fn away(&mut self, body: &mut dyn FnMut()) -> Result<(), String> {
            body();
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

    /// A report line for a task the recipe found a session for.
    const RUNNING: &str = "graph: specs/01-foo.md on feat/01-foo\nT1     running          log /r/t1.log  worktree /w\n";

    #[test]
    fn the_three_levers_are_keys_of_their_own_and_nothing_else_is() {
        let mut app = app(RUNNING);

        assert_eq!(on_key(&mut app, press(KeyCode::Char('p'))), Action::Pause);
        assert_eq!(
            on_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT),
            ),
            Action::Resume,
            "the key that has to be typed shifted was refused for being shifted",
        );
        assert_eq!(on_key(&mut app, press(KeyCode::Enter)), Action::Attach);
        // Unshifted, `r` is the read the board already had: a resume is the
        // heavier of the two, and it is the one that has to be reached for.
        assert_eq!(on_key(&mut app, press(KeyCode::Char('r'))), Action::Status);
        assert_eq!(on_key(&mut app, press(KeyCode::Char('P'))), Action::Nothing);
    }

    #[test]
    fn a_chord_is_not_a_lever() {
        // Ctrl-P is "previous" in half the world's key bindings, and here it
        // would kill a running agent. The three keys that reach outside the
        // board are the three that ask to have been pressed on their own.
        let mut app = app(RUNNING);

        for code in [KeyCode::Char('p'), KeyCode::Char('R'), KeyCode::Enter] {
            for modifiers in [KeyModifiers::CONTROL, KeyModifiers::ALT] {
                assert_eq!(
                    on_key(&mut app, KeyEvent::new(code, modifiers)),
                    Action::Nothing,
                    "{code:?} with {modifiers:?} pulled a lever",
                );
            }
        }
    }

    #[test]
    fn a_board_with_no_rows_has_no_task_to_pull_a_lever_on() {
        // Every one of the three is about the selected row, and a spec whose
        // tasks are still to be written has none.
        let dispatch = Arc::new(Answers::default());
        let empty = "graph: specs/01-foo.md on feat/01-foo\n";

        for lever in [Action::Pause, Action::Resume, Action::Attach] {
            let mut app = over(Arc::clone(&dispatch), empty);
            let now = Timestamp::from_epoch_seconds(1_000);
            match lever {
                Action::Pause => app.pause(),
                Action::Resume => app
                    .resume(&mut Frames::default(), now)
                    .expect("no row is not a terminal that refused"),
                _ => app
                    .attach(&mut Frames::default(), now)
                    .expect("no row is not a terminal that refused"),
            }
            assert_eq!(app.board.message, super::NO_ROW, "on {lever:?}");
        }
        assert_eq!(
            dispatch.done(),
            Vec::<String>::new(),
            "a lever was pulled on a task that is not there",
        );
    }

    #[test]
    fn a_running_task_whose_line_carries_no_log_has_nowhere_to_write_the_marker() {
        // The recipe prints the log beside every state that has a run, so
        // this is a report the board should not meet — and the answer to
        // one it does is a sentence, not a marker in the working directory.
        let dispatch = Arc::new(Answers::default());
        let mut app = over(
            Arc::clone(&dispatch),
            "graph: specs/01-foo.md on feat/01-foo\nT1     running\n",
        );

        app.pause();

        assert_eq!(
            dispatch.done(),
            Vec::<String>::new(),
            "a session was killed with nothing to record it with",
        );
        assert!(
            app.board.message.contains("T1") && app.board.message.contains("log"),
            "the refusal does not say what is missing: {}",
            app.board.message,
        );
    }

    #[test]
    fn a_marker_that_could_not_be_written_is_said_rather_than_swallowed() {
        // A run directory removed while the board was up: the kill happened
        // and the marker did not, which is the one case where `keeler-status`
        // will say `died` about a pause somebody made on purpose.
        let dispatch = Arc::new(Answers::default());
        let mut app = over(
            Arc::clone(&dispatch),
            "graph: specs/01-foo.md on feat/01-foo\n\
             T1     running          log /keeler-top-no-such-directory/t1.log  worktree /w\n",
        );

        app.pause();

        assert_eq!(dispatch.done(), ["kill keeler-01-foo-t1"]);
        assert!(
            app.board
                .message
                .contains("/keeler-top-no-such-directory/t1.paused"),
            "the refusal does not name the file that was not written: {}",
            app.board.message,
        );
    }

    #[test]
    fn what_a_recipe_said_is_cut_to_the_line_that_says_what_happened() {
        assert_eq!(
            super::sentence(
                "keeler-resume: re-running T1 in the worktree it has\n  worktree: /w\n"
            ),
            "keeler-resume: re-running T1 in the worktree it has",
        );
        // The recipes print a blank line before their hints often enough
        // that a status line reading "the first line" would show one.
        assert_eq!(
            super::sentence("\n\nsomething happened\n"),
            "something happened"
        );
        assert_eq!(super::sentence(""), "");
    }

    #[test]
    fn a_lever_that_did_what_it_was_asked_says_nothing() {
        // The watcher has just come back from tmux, or watched their client
        // move: a line under the table reporting it would be the board
        // congratulating itself on what they were looking at.
        assert_eq!(super::refusal(Ok(())), None);
        assert_eq!(
            super::refusal(Err("no server running".to_string())),
            Some("no server running".to_string()),
        );
    }

    #[test]
    fn what_a_lever_said_outlives_the_tick_a_second_later() {
        // Every tick re-assembles the board, and the line under the table
        // with it. A refusal drawn once and blanked before the next second
        // is a refusal nobody read — and `p` and `R` are pressed precisely
        // when something is going wrong.
        let dispatch = Arc::new(Answers {
            refuses: Some("can't find session: =keeler-01-foo-t1".to_string()),
            ..Answers::default()
        });
        let mut app = over(Arc::clone(&dispatch), RUNNING);

        app.pause();
        let said = app.board.message.clone();
        app.tick(Timestamp::from_epoch_seconds(1_001));

        assert_eq!(said, "can't find session: =keeler-01-foo-t1");
        assert_eq!(app.board.message, said, "the tick took the answer away");

        // And a lever that did what it was asked takes the last sentence
        // away with it, rather than leaving a refusal under a board it is
        // no longer about.
        let mut app = over(
            Arc::new(Answers::default()),
            &format!("{RUNNING}T2     not spawned\n"),
        );
        app.board.selected = 1;
        app.pause();
        assert!(app.board.message.contains("T2"));

        app.board.selected = 0;
        app.attach(&mut Frames::default(), Timestamp::from_epoch_seconds(1_002))
            .expect("a recording surface does not refuse");

        // What is left is whatever the two reads have to say — here the
        // graph refusing, this fixture standing outside a repository — and
        // not a word about T2.
        assert!(
            !app.board.message.contains("T2"),
            "a lever that succeeded left the last one's sentence up: {}",
            app.board.message,
        );
        assert!(app.board.message.contains("no graph to read"));
    }

    #[test]
    fn the_board_says_it_is_resuming_before_the_recipe_takes_the_thread() {
        // `keeler-resume` is a `just` that runs a second `just`, and it runs
        // here rather than on a thread of its own — so the frame that says
        // so has to be drawn before it starts, or the board simply stops for
        // seconds with the row still reading `paused`.
        let dispatch = Arc::new(Answers {
            said: "keeler-resume: re-running T1 in the worktree it has\n".to_string(),
            ..Answers::default()
        });
        let mut app = over(Arc::clone(&dispatch), RUNNING);
        let mut frames = Frames::default();

        app.resume(&mut frames, Timestamp::from_epoch_seconds(1_000))
            .expect("a recording surface does not refuse");

        assert_eq!(
            frames.0.len(),
            1,
            "the board went still without drawing a word about it",
        );
        assert_eq!(frames.0[0].message, "keeler-top: resuming T1…");
        assert_eq!(dispatch.done(), ["resume T1"]);
        assert_eq!(
            app.board.message,
            "keeler-resume: re-running T1 in the worktree it has",
        );
    }

    /// A screen that answers everything and remembers nothing — the guard's
    /// own behaviour is `terminal.rs`'s to test.
    #[derive(Debug, Clone, Copy)]
    struct Blind;

    impl Screen for Blind {
        fn enter(&mut self) -> std::io::Result<()> {
            Ok(())
        }

        fn leave(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Whether anything at all is on a terminal.
    fn blank(terminal: &Terminal<ratatui::backend::TestBackend>) -> bool {
        let buffer = terminal.backend().buffer().clone();
        buffer.content().iter().all(|cell| cell.symbol() == " ")
    }

    #[test]
    fn a_screen_given_back_and_taken_again_is_drawn_again_whole() {
        // ratatui draws by difference against the frame before it. tmux
        // wrote over that frame while the board was away and said nothing
        // about it, so a board that came back and drew the difference would
        // draw nothing at all onto somebody else's output.
        let board = app(RUNNING).board;
        let now = Timestamp::from_epoch_seconds(1_000);
        let mut live = Live::new(
            Terminal::new(ratatui::backend::TestBackend::new(60, 6)).expect("a terminal"),
            Guard::new(Blind).expect("the blind screen entered"),
        );
        live.draw(&board, THEME, now).expect("a frame");
        assert!(!blank(&live.terminal));

        // What tmux left behind, in the one form a test backend has for it.
        live.terminal.backend_mut().clear().expect("the backend");
        live.away(&mut || {}).expect("the blind screen");
        live.draw(&board, THEME, now).expect("a frame");

        assert!(
            !blank(&live.terminal),
            "the board came back and redrew nothing",
        );
    }

    #[test]
    fn a_terminal_holding_no_screen_of_its_own_has_none_to_give_back() {
        // Which is not the same as having nothing to do: `--once` and every
        // test backend still have a frame drawn over by whatever ran.
        let mut terminal =
            Terminal::new(ratatui::backend::TestBackend::new(20, 3)).expect("a terminal");
        let board = app(RUNNING).board;
        let now = Timestamp::from_epoch_seconds(1_000);
        Surface::draw(&mut terminal, &board, THEME, now).expect("a frame");
        let mut ran = false;

        Surface::away(&mut terminal, &mut || ran = true).expect("a terminal that answers");

        assert!(ran, "the body never ran");
        assert!(blank(&terminal), "the frame drawn over was kept");
    }
}
