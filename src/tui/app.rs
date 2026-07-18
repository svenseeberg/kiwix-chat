use std::sync::Arc;

use anyhow::Result;
use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::Mutex;

use crate::agent::{run_turn, AgentEvent, SYSTEM_PROMPT};
use crate::kiwix::KiwixClient;
use crate::llm::{ChatMessage, LlmClient};

/// A message as displayed in the chat pane.
#[derive(Debug, Clone)]
pub struct DisplayMessage {
    pub kind: DisplayKind,
    pub text: String,
    /// For `Thinking` blocks: whether this block was collapsed once finished.
    /// The global `show_thinking` toggle can override this for display.
    pub collapsed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayKind {
    User,
    Assistant,
    Thinking,
    Tool,
    Error,
    Info,
}

/// Everything needed to run the chat session.
pub struct App {
    pub messages: Vec<DisplayMessage>,
    pub input: String,
    /// Vertical scroll offset (in wrapped lines from the top).
    pub scroll: u16,
    /// When true, the view auto-scrolls to the newest content.
    pub follow: bool,
    /// True while an agent turn is in progress (input is disabled).
    pub busy: bool,
    pub should_quit: bool,
    /// Index of the assistant message currently receiving streamed tokens.
    current_assistant: Option<usize>,
    /// Index of the thinking block currently receiving streamed reasoning.
    current_reasoning: Option<usize>,
    /// When true, all thinking blocks are shown expanded (toggled with Tab).
    pub show_thinking: bool,

    // Session context
    pub lang: String,
    pub max_rounds: usize,
    pub llm: LlmClient,
    pub kiwix: KiwixClient,
    pub kiwix_reachable: bool,
    /// Full LLM conversation history, shared with the running agent task.
    conversation: Arc<Mutex<Vec<ChatMessage>>>,
}

impl App {
    pub fn new(
        llm: LlmClient,
        kiwix: KiwixClient,
        kiwix_reachable: bool,
        lang: String,
        max_rounds: usize,
    ) -> Self {
        let conversation = Arc::new(Mutex::new(vec![ChatMessage::system(SYSTEM_PROMPT)]));
        let mut app = Self {
            messages: Vec::new(),
            input: String::new(),
            scroll: 0,
            follow: true,
            busy: false,
            should_quit: false,
            current_assistant: None,
            current_reasoning: None,
            show_thinking: false,
            lang,
            max_rounds,
            llm,
            kiwix,
            kiwix_reachable,
            conversation,
        };
        app.push(
            DisplayKind::Info,
            format!(
                "Connected to LLM '{}' at {}. Kiwix: {} ({}). Type a question and press Enter. \
                 Tab toggles thinking. Commands: /lang <code>, /clear, /quit. Ctrl+C to exit.",
                app.llm.model(),
                app.llm.base(),
                app.kiwix.base(),
                if app.kiwix_reachable {
                    "reachable"
                } else {
                    "UNREACHABLE"
                },
            ),
        );
        app
    }

    fn push(&mut self, kind: DisplayKind, text: impl Into<String>) {
        self.messages.push(DisplayMessage {
            kind,
            text: text.into(),
            collapsed: false,
        });
        self.follow = true;
    }

    /// Collapse the thinking block currently streaming (if any) and stop tracking it.
    fn finalize_reasoning(&mut self) {
        if let Some(idx) = self.current_reasoning.take() {
            self.messages[idx].collapsed = true;
        }
    }

    /// Handle an event coming from the running agent task.
    fn on_agent_event(&mut self, ev: AgentEvent) {
        match ev {
            AgentEvent::Token(t) => {
                self.finalize_reasoning();
                match self.current_assistant {
                    Some(idx) => self.messages[idx].text.push_str(&t),
                    None => {
                        self.messages.push(DisplayMessage {
                            kind: DisplayKind::Assistant,
                            text: t,
                            collapsed: false,
                        });
                        self.current_assistant = Some(self.messages.len() - 1);
                    }
                }
            }
            AgentEvent::Reasoning(r) => {
                self.current_assistant = None;
                match self.current_reasoning {
                    Some(idx) => self.messages[idx].text.push_str(&r),
                    None => {
                        self.messages.push(DisplayMessage {
                            kind: DisplayKind::Thinking,
                            text: r,
                            collapsed: false,
                        });
                        self.current_reasoning = Some(self.messages.len() - 1);
                    }
                }
            }
            AgentEvent::ToolFinished { summary } => {
                self.finalize_reasoning();
                self.current_assistant = None;
                self.push(DisplayKind::Tool, summary);
            }
            AgentEvent::Done => {
                self.finalize_reasoning();
                self.current_assistant = None;
                self.busy = false;
            }
            AgentEvent::Error(e) => {
                self.finalize_reasoning();
                self.current_assistant = None;
                self.push(DisplayKind::Error, e);
                self.busy = false;
            }
        }
        self.follow = true;
    }

    /// Handle a key press. Returns true if the display may need a redraw.
    fn on_key(&mut self, key: KeyEvent, tx: &UnboundedSender<AgentEvent>) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        // Ctrl+C always quits.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        match key.code {
            KeyCode::PageUp => {
                self.follow = false;
                self.scroll = self.scroll.saturating_sub(5);
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(5);
            }
            KeyCode::Tab => {
                // Expand/collapse all reasoning blocks (works even while busy).
                self.show_thinking = !self.show_thinking;
                self.follow = false;
            }
            _ if self.busy => {} // ignore input edits while a turn runs
            KeyCode::Enter => self.submit(tx),
            KeyCode::Char(c) => self.input.push(c),
            KeyCode::Backspace => {
                self.input.pop();
            }
            _ => {}
        }
    }

    /// Handle a mouse event (wheel scrolling of the transcript).
    fn on_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.follow = false;
                self.scroll = self.scroll.saturating_sub(3);
            }
            MouseEventKind::ScrollDown => {
                self.scroll = self.scroll.saturating_add(3);
            }
            _ => {}
        }
    }

    fn submit(&mut self, tx: &UnboundedSender<AgentEvent>) {
        let text = self.input.trim().to_string();
        self.input.clear();
        if text.is_empty() {
            return;
        }
        if let Some(cmd) = text.strip_prefix('/') {
            self.handle_command(cmd);
            return;
        }

        self.push(DisplayKind::User, text.clone());
        self.busy = true;
        self.current_assistant = None;

        let llm = self.llm.clone();
        let kiwix = self.kiwix.clone();
        let lang = self.lang.clone();
        let max_rounds = self.max_rounds;
        let conversation = self.conversation.clone();
        let tx = tx.clone();

        tokio::spawn(async move {
            let mut guard = conversation.lock().await;
            guard.push(ChatMessage::user(text));
            run_turn(&llm, &kiwix, &lang, max_rounds, &mut guard, &tx).await;
        });
    }

    fn handle_command(&mut self, cmd: &str) {
        let mut parts = cmd.split_whitespace();
        match parts.next() {
            Some("quit") | Some("q") | Some("exit") => self.should_quit = true,
            Some("clear") => {
                self.messages.clear();
                self.current_assistant = None;
                self.current_reasoning = None;
                let conversation = self.conversation.clone();
                tokio::spawn(async move {
                    let mut guard = conversation.lock().await;
                    guard.truncate(1); // keep the system prompt
                });
                self.push(DisplayKind::Info, "Conversation cleared.");
            }
            Some("lang") => match parts.next() {
                Some(code) => {
                    self.lang = code.to_string();
                    self.push(
                        DisplayKind::Info,
                        format!("Search language set to '{code}'."),
                    );
                }
                None => self.push(
                    DisplayKind::Info,
                    format!("Current search language: '{}'.", self.lang),
                ),
            },
            Some(other) => self.push(
                DisplayKind::Error,
                format!("Unknown command '/{other}'. Try /lang, /clear, /quit."),
            ),
            None => {}
        }
    }
}

/// Set up the terminal, run the event loop, and restore the terminal on exit.
pub async fn run(mut app: App) -> Result<()> {
    let mut terminal = init_terminal()?;
    let (tx, rx) = unbounded_channel::<AgentEvent>();
    let result = event_loop(&mut terminal, &mut app, tx, rx).await;
    restore_terminal(&mut terminal)?;
    result
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    tx: UnboundedSender<AgentEvent>,
    mut rx: UnboundedReceiver<AgentEvent>,
) -> Result<()> {
    let mut input_events = EventStream::new();

    terminal.draw(|f| ui::draw(f, app))?;

    loop {
        tokio::select! {
            // Terminal input.
            maybe_event = input_events.next() => {
                match maybe_event {
                    Some(Ok(Event::Key(key))) => app.on_key(key, &tx),
                    Some(Ok(Event::Mouse(mouse))) => app.on_mouse(mouse),
                    Some(Ok(Event::Resize(_, _))) => {}
                    Some(Err(e)) => return Err(e.into()),
                    None => app.should_quit = true,
                    _ => {}
                }
            }
            // Agent updates. Drain as many as are ready to batch redraws.
            Some(ev) = rx.recv() => {
                app.on_agent_event(ev);
                while let Ok(ev) = rx.try_recv() {
                    app.on_agent_event(ev);
                }
            }
        }

        if app.should_quit {
            break;
        }
        terminal.draw(|f| ui::draw(f, app))?;
    }
    Ok(())
}

fn init_terminal() -> Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    use crossterm::event::EnableMouseCapture;
    use crossterm::terminal::{enable_raw_mode, EnterAlternateScreen};
    let mut stdout = std::io::stdout();
    enable_raw_mode()?;
    crossterm::execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    use crossterm::event::DisableMouseCapture;
    use crossterm::terminal::{disable_raw_mode, LeaveAlternateScreen};
    disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

use crate::tui::ui;
