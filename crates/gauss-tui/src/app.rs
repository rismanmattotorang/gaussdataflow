//! Application state and the event loop. Input is handled here; rendering
//! lives in [`crate::ui`]; network IO happens on the fetch task so a slow or
//! down API never freezes the interface.

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;
use serde_json::Value;

use crate::api::{Actor, Connection, Job, JobDetail, JobOverview, PlatformStats, Workspace};
use crate::fetch::{Command, Update};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Workspace,
    Connection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Connections,
    Jobs,
    Sources,
    Destinations,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Connections, Tab::Jobs, Tab::Sources, Tab::Destinations];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Connections => "Connections",
            Tab::Jobs => "Jobs",
            Tab::Sources => "Sources",
            Tab::Destinations => "Destinations",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

/// One transient line in the footer: an action result or an error.
pub struct Notice {
    pub text: String,
    pub is_error: bool,
    pub at: Instant,
}

pub enum Overlay {
    None,
    Help,
    /// Text input for a new workspace name.
    Input(String),
    /// Committed state of the focused connection, pretty-printed.
    StateJson(String),
    /// Job drill-down with attempt history.
    JobDetail(JobDetail),
}

pub struct App {
    pub api_label: String,
    pub screen: Screen,
    pub overlay: Overlay,
    pub notice: Option<Notice>,

    // Home
    pub workspaces: Vec<Workspace>,
    pub stats: Option<PlatformStats>,
    pub home_jobs: Vec<JobOverview>,
    pub home_sel: usize,

    // Workspace
    pub workspace: Option<Workspace>,
    pub ws_stats: Option<PlatformStats>,
    pub tab: Tab,
    pub connections: Vec<Connection>,
    pub sources: Vec<Actor>,
    pub destinations: Vec<Actor>,
    pub ws_jobs: Vec<JobOverview>,
    pub tab_sel: [usize; 4],

    // Connection
    pub connection: Option<Connection>,
    pub conn_jobs: Vec<Job>,
    pub conn_state: Option<Value>,
    pub conn_sel: usize,

    pub last_refresh: Instant,
    pub loading: bool,

    commands: tokio::sync::mpsc::UnboundedSender<Command>,
    updates: std::sync::mpsc::Receiver<Update>,
    refresh_every: Duration,
}

impl App {
    pub fn new(
        api_label: String,
        commands: tokio::sync::mpsc::UnboundedSender<Command>,
        updates: std::sync::mpsc::Receiver<Update>,
        refresh_secs: u64,
    ) -> Self {
        Self {
            api_label,
            screen: Screen::Home,
            overlay: Overlay::None,
            notice: None,
            workspaces: Vec::new(),
            stats: None,
            home_jobs: Vec::new(),
            home_sel: 0,
            workspace: None,
            ws_stats: None,
            tab: Tab::Connections,
            connections: Vec::new(),
            sources: Vec::new(),
            destinations: Vec::new(),
            ws_jobs: Vec::new(),
            tab_sel: [0; 4],
            connection: None,
            conn_jobs: Vec::new(),
            conn_state: None,
            conn_sel: 0,
            last_refresh: Instant::now(),
            loading: true,
            commands,
            updates,
            refresh_every: Duration::from_secs(refresh_secs.max(1)),
        }
    }

    pub fn run(mut self, mut terminal: DefaultTerminal) -> anyhow::Result<()> {
        self.send(Command::Home);
        loop {
            while let Ok(update) = self.updates.try_recv() {
                self.apply(update);
            }
            // Notices fade after a few seconds.
            if let Some(n) = &self.notice {
                if n.at.elapsed() > Duration::from_secs(6) {
                    self.notice = None;
                }
            }
            terminal.draw(|f| crate::ui::draw(f, &self))?;
            if event::poll(Duration::from_millis(120))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press && !self.on_key(key.code, key.modifiers) {
                        return Ok(());
                    }
                }
            }
            if self.last_refresh.elapsed() >= self.refresh_every {
                self.refresh();
            }
        }
    }

    fn send(&mut self, cmd: Command) {
        self.loading = true;
        let _ = self.commands.send(cmd);
    }

    fn refresh(&mut self) {
        self.last_refresh = Instant::now();
        match self.screen {
            Screen::Home => self.send(Command::Home),
            Screen::Workspace => {
                if let Some(ws) = &self.workspace {
                    let id = ws.id;
                    self.send(Command::Workspace(id));
                }
            }
            Screen::Connection => {
                if let Some(c) = &self.connection {
                    let id = c.id;
                    self.send(Command::Connection(id));
                }
            }
        }
    }

    fn apply(&mut self, update: Update) {
        self.loading = false;
        match update {
            Update::Home {
                workspaces,
                stats,
                jobs,
            } => {
                self.workspaces = workspaces;
                self.stats = Some(stats);
                self.home_jobs = jobs;
                self.home_sel = self.home_sel.min(self.workspaces.len().saturating_sub(1));
            }
            Update::Workspace {
                id,
                stats,
                connections,
                sources,
                destinations,
                jobs,
            } => {
                if self.workspace.as_ref().is_some_and(|w| w.id == id) {
                    self.ws_stats = Some(stats);
                    self.connections = connections;
                    self.sources = sources;
                    self.destinations = destinations;
                    self.ws_jobs = jobs;
                    self.clamp_tab_selection();
                }
            }
            Update::Connection { id, jobs, state } => {
                if self.connection.as_ref().is_some_and(|c| c.id == id) {
                    self.conn_jobs = jobs;
                    self.conn_state = state;
                    self.conn_sel = self.conn_sel.min(self.conn_jobs.len().saturating_sub(1));
                }
            }
            Update::JobDetail(detail) => self.overlay = Overlay::JobDetail(detail),
            Update::Notice(text) => {
                self.notice = Some(Notice {
                    text,
                    is_error: false,
                    at: Instant::now(),
                })
            }
            Update::Error(text) => {
                self.notice = Some(Notice {
                    text,
                    is_error: true,
                    at: Instant::now(),
                })
            }
        }
    }

    fn clamp_tab_selection(&mut self) {
        for tab in Tab::ALL {
            let len = self.tab_len(tab);
            let sel = &mut self.tab_sel[tab.index()];
            *sel = (*sel).min(len.saturating_sub(1));
        }
    }

    pub fn tab_len(&self, tab: Tab) -> usize {
        match tab {
            Tab::Connections => self.connections.len(),
            Tab::Jobs => self.ws_jobs.len(),
            Tab::Sources => self.sources.len(),
            Tab::Destinations => self.destinations.len(),
        }
    }

    /// Returns false when the app should exit.
    fn on_key(&mut self, code: KeyCode, mods: KeyModifiers) -> bool {
        // Overlays capture input first.
        match &mut self.overlay {
            Overlay::Input(buf) => {
                match code {
                    KeyCode::Esc => self.overlay = Overlay::None,
                    KeyCode::Enter => {
                        let name = buf.trim().to_string();
                        self.overlay = Overlay::None;
                        if !name.is_empty() {
                            self.send(Command::CreateWorkspace(name));
                        }
                    }
                    KeyCode::Backspace => {
                        buf.pop();
                    }
                    KeyCode::Char(c) if !mods.contains(KeyModifiers::CONTROL) => buf.push(c),
                    _ => {}
                }
                return true;
            }
            Overlay::Help | Overlay::StateJson(_) | Overlay::JobDetail(_) => {
                if matches!(
                    code,
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter | KeyCode::Char('?')
                ) {
                    self.overlay = Overlay::None;
                }
                return true;
            }
            Overlay::None => {}
        }

        if code == KeyCode::Char('c') && mods.contains(KeyModifiers::CONTROL) {
            return false;
        }
        match code {
            KeyCode::Char('q') => return false,
            KeyCode::Char('?') => self.overlay = Overlay::Help,
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Esc | KeyCode::Backspace => self.go_back(),
            _ => match self.screen {
                Screen::Home => self.on_key_home(code),
                Screen::Workspace => self.on_key_workspace(code),
                Screen::Connection => self.on_key_connection(code),
            },
        }
        true
    }

    fn go_back(&mut self) {
        match self.screen {
            Screen::Home => {}
            Screen::Workspace => {
                self.screen = Screen::Home;
                self.workspace = None;
                self.refresh();
            }
            Screen::Connection => {
                self.screen = if self.workspace.is_some() {
                    Screen::Workspace
                } else {
                    Screen::Home
                };
                self.connection = None;
                self.refresh();
            }
        }
    }

    fn on_key_home(&mut self, code: KeyCode) {
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.home_sel = self.home_sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                self.home_sel = (self.home_sel + 1).min(self.workspaces.len().saturating_sub(1))
            }
            KeyCode::Enter => {
                if let Some(ws) = self.workspaces.get(self.home_sel).cloned() {
                    let id = ws.id;
                    self.workspace = Some(ws);
                    self.screen = Screen::Workspace;
                    self.tab = Tab::Connections;
                    self.connections.clear();
                    self.sources.clear();
                    self.destinations.clear();
                    self.ws_jobs.clear();
                    self.ws_stats = None;
                    self.send(Command::Workspace(id));
                }
            }
            KeyCode::Char('n') => self.overlay = Overlay::Input(String::new()),
            _ => {}
        }
    }

    fn on_key_workspace(&mut self, code: KeyCode) {
        let sel = self.tab_sel[self.tab.index()];
        match code {
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                self.tab = Tab::ALL[(self.tab.index() + 1) % Tab::ALL.len()];
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                self.tab = Tab::ALL[(self.tab.index() + Tab::ALL.len() - 1) % Tab::ALL.len()];
            }
            KeyCode::Char(c @ '1'..='4') => {
                self.tab = Tab::ALL[c as usize - '1' as usize];
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.tab_sel[self.tab.index()] = sel.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.tab_sel[self.tab.index()] =
                    (sel + 1).min(self.tab_len(self.tab).saturating_sub(1))
            }
            KeyCode::Enter => match self.tab {
                Tab::Connections => {
                    if let Some(conn) = self.connections.get(sel).cloned() {
                        self.open_connection(conn);
                    }
                }
                Tab::Jobs => {
                    if let Some(job) = self.ws_jobs.get(sel) {
                        let id = job.id;
                        self.send(Command::JobDetail(id));
                    }
                }
                _ => {}
            },
            KeyCode::Char('s') => {
                if self.tab == Tab::Connections {
                    if let Some(conn) = self.connections.get(sel) {
                        let id = conn.id;
                        self.send(Command::TriggerSync(id));
                    }
                }
            }
            _ => {}
        }
    }

    fn open_connection(&mut self, conn: Connection) {
        let id = conn.id;
        self.connection = Some(conn);
        self.screen = Screen::Connection;
        self.conn_jobs.clear();
        self.conn_state = None;
        self.conn_sel = 0;
        self.send(Command::Connection(id));
    }

    fn on_key_connection(&mut self, code: KeyCode) {
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.conn_sel = self.conn_sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                self.conn_sel = (self.conn_sel + 1).min(self.conn_jobs.len().saturating_sub(1))
            }
            KeyCode::Enter => {
                if let Some(job) = self.conn_jobs.get(self.conn_sel) {
                    let id = job.id;
                    self.send(Command::JobDetail(id));
                }
            }
            KeyCode::Char('s') => {
                if let Some(c) = &self.connection {
                    let id = c.id;
                    self.send(Command::TriggerSync(id));
                }
            }
            KeyCode::Char('c') => {
                if let (Some(job), Some(conn)) =
                    (self.conn_jobs.get(self.conn_sel), &self.connection)
                {
                    if matches!(job.status.as_str(), "pending" | "running") {
                        let (job, connection) = (job.id, conn.id);
                        self.send(Command::CancelJob { job, connection });
                    } else {
                        self.notice = Some(Notice {
                            text: format!("job #{} is already {}", job.id, job.status),
                            is_error: true,
                            at: Instant::now(),
                        });
                    }
                }
            }
            KeyCode::Char('v') => {
                let pretty = match &self.conn_state {
                    Some(v) => serde_json::to_string_pretty(v)
                        .unwrap_or_else(|_| "<unprintable>".to_string()),
                    None => "No committed state yet — run a sync first.".to_string(),
                };
                self.overlay = Overlay::StateJson(pretty);
            }
            _ => {}
        }
    }
}
