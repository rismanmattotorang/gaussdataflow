//! Rendering. Pure function of [`App`] state — no IO, no mutation.

use chrono::{DateTime, Utc};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState,
    Tabs, Wrap,
};
use ratatui::Frame;

use crate::app::{App, HomeFocus, Overlay, Screen, Tab};

const ACCENT: Color = Color::Rgb(122, 162, 255);
const MUTED: Color = Color::Rgb(147, 160, 196);
const OK: Color = Color::Rgb(74, 222, 128);
const WARN: Color = Color::Rgb(251, 191, 36);
const ERR: Color = Color::Rgb(248, 113, 113);

pub fn draw(f: &mut Frame, app: &App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(f.area());

    draw_header(f, app, header);
    match app.screen {
        Screen::Home => draw_home(f, app, body),
        Screen::Workspace => draw_workspace(f, app, body),
        Screen::Connection => draw_connection(f, app, body),
    }
    draw_footer(f, app, footer);
    draw_overlay(f, app);
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let mut crumbs: Vec<Span> = vec![
        Span::styled(" ⚡ Gauss-DataFlow ", Style::new().fg(ACCENT).bold()),
        Span::styled("· ", Style::new().fg(MUTED)),
        Span::styled("Fleet", crumb_style(app.screen == Screen::Home)),
    ];
    if let Some(ws) = &app.workspace {
        crumbs.push(Span::styled(" › ", Style::new().fg(MUTED)));
        crumbs.push(Span::styled(
            ws.name.clone(),
            crumb_style(app.screen == Screen::Workspace),
        ));
    }
    if let Some(c) = &app.connection {
        crumbs.push(Span::styled(" › ", Style::new().fg(MUTED)));
        crumbs.push(Span::styled(
            c.name.clone(),
            crumb_style(app.screen == Screen::Connection),
        ));
    }
    let right = if app.online {
        Line::from(vec![
            Span::styled(if app.loading { "⟳ " } else { "" }, Style::new().fg(MUTED)),
            Span::styled("● ", Style::new().fg(OK)),
            Span::styled(format!("{} ", app.api_label), Style::new().fg(MUTED)),
        ])
    } else {
        Line::from(vec![
            Span::styled("● offline — retrying ", Style::new().fg(ERR).bold()),
            Span::styled(format!("{} ", app.api_label), Style::new().fg(MUTED)),
        ])
    };
    f.render_widget(Line::from(crumbs), area);
    f.render_widget(Paragraph::new(right).alignment(Alignment::Right), area);
}

fn crumb_style(active: bool) -> Style {
    if active {
        Style::new().bold()
    } else {
        Style::new().fg(MUTED)
    }
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    if let Some(n) = &app.notice {
        let style = if n.is_error {
            Style::new().fg(ERR)
        } else {
            Style::new().fg(OK)
        };
        f.render_widget(
            Paragraph::new(Span::styled(format!(" {}", n.text), style)),
            area,
        );
        return;
    }
    let hints: &[(&str, &str)] = match app.screen {
        Screen::Home => &[
            ("⇥", "switch pane"),
            ("↑↓", "select"),
            ("⏎", "open"),
            ("n", "new workspace"),
            ("r", "refresh"),
            ("?", "help"),
            ("q", "quit"),
        ],
        Screen::Workspace => &[
            ("⇥/1-4", "tabs"),
            ("↑↓", "select"),
            ("⏎", "open"),
            ("s", "sync"),
            ("p", "pause/resume"),
            ("esc", "back"),
            ("?", "help"),
        ],
        Screen::Connection => &[
            ("↑↓", "select"),
            ("⏎", "attempts"),
            ("s", "sync"),
            ("p", "pause/resume"),
            ("c", "cancel job"),
            ("v", "state"),
            ("esc", "back"),
        ],
    };
    let mut spans = Vec::new();
    for (key, label) in hints {
        spans.push(Span::styled(
            format!(" {key} "),
            Style::new().fg(ACCENT).bold(),
        ));
        spans.push(Span::styled(format!("{label} "), Style::new().fg(MUTED)));
    }
    f.render_widget(Line::from(spans), area);
}

// ---------- stats strip ----------

fn draw_stats(f: &mut Frame, app: &App, area: Rect, fleet: bool) {
    let stats = if fleet { &app.stats } else { &app.ws_stats };
    let Some(s) = stats else {
        f.render_widget(
            Paragraph::new(Span::styled("loading metrics…", Style::new().fg(MUTED)))
                .block(panel("Pulse")),
            area,
        );
        return;
    };
    let health = if s.jobs_failed_24h > 0 {
        Span::styled("degraded", Style::new().fg(WARN).bold())
    } else {
        Span::styled("healthy", Style::new().fg(OK).bold())
    };
    let cells: Vec<(String, Line)> = vec![
        (
            "Topology".into(),
            Line::from(vec![
                Span::styled(s.connections.to_string(), Style::new().bold()),
                Span::styled(" pipelines · ", Style::new().fg(MUTED)),
                Span::styled(s.sources.to_string(), Style::new().bold()),
                Span::styled(" src → ", Style::new().fg(MUTED)),
                Span::styled(s.destinations.to_string(), Style::new().bold()),
                Span::styled(" dst", Style::new().fg(MUTED)),
            ]),
        ),
        (
            "Queue".into(),
            Line::from(vec![
                Span::styled(s.jobs_running.to_string(), Style::new().fg(WARN).bold()),
                Span::styled(" running  ", Style::new().fg(MUTED)),
                Span::styled(s.jobs_pending.to_string(), Style::new().bold()),
                Span::styled(" pending", Style::new().fg(MUTED)),
            ]),
        ),
        (
            "Last 24h".into(),
            Line::from(vec![
                Span::styled(
                    format!("✓{}", s.jobs_succeeded_24h),
                    Style::new().fg(OK).bold(),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("✗{}", s.jobs_failed_24h),
                    Style::new().fg(ERR).bold(),
                ),
                Span::styled("  ", Style::new()),
                Span::styled(group_digits(s.records_synced_24h), Style::new().bold()),
                Span::styled(" records", Style::new().fg(MUTED)),
            ]),
        ),
        (
            "Health".into(),
            Line::from(vec![
                health,
                Span::styled(
                    match s.last_success_at {
                        Some(t) => format!("  last success {}", time_ago(t)),
                        None => "  no successful sync yet".to_string(),
                    },
                    Style::new().fg(MUTED),
                ),
            ]),
        ),
    ];
    let columns =
        Layout::horizontal(vec![Constraint::Ratio(1, cells.len() as u32); cells.len()]).split(area);
    for (i, (title, line)) in cells.into_iter().enumerate() {
        f.render_widget(Paragraph::new(line).block(panel(&title)), columns[i]);
    }
}

// ---------- screens ----------

fn draw_home(f: &mut Frame, app: &App, area: Rect) {
    let [stats_area, body] =
        Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(area);
    draw_stats(f, app, stats_area, true);

    let [left, right] =
        Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)]).areas(body);
    let ws_focused = app.home_focus == HomeFocus::Workspaces;

    let items: Vec<ListItem> = if app.workspaces.is_empty() {
        vec![ListItem::new(Span::styled(
            "No workspaces yet — press n to create one.",
            Style::new().fg(MUTED),
        ))]
    } else {
        app.workspaces
            .iter()
            .map(|w| {
                ListItem::new(Line::from(vec![
                    Span::raw(w.name.clone()),
                    Span::styled(
                        format!("  {}", time_ago(w.created_at)),
                        Style::new().fg(MUTED),
                    ),
                ]))
            })
            .collect()
    };
    let mut state = ListState::default().with_selected(Some(app.home_sel));
    f.render_stateful_widget(
        List::new(items)
            .block(focusable_panel(
                &format!("Workspaces ({})", app.workspaces.len()),
                ws_focused,
            ))
            .highlight_style(if ws_focused {
                highlight()
            } else {
                Style::new().fg(ACCENT)
            })
            .highlight_symbol(if ws_focused { "▌ " } else { "│ " }),
        left,
        &mut state,
    );

    let rows: Vec<Row> = app
        .home_jobs
        .iter()
        .map(|j| {
            Row::new(vec![
                Cell::from(format!("#{}", j.id)),
                Cell::from(j.connection_name.clone()),
                Cell::from(status_span(&j.status)),
                Cell::from(j.records_synced.map(group_digits).unwrap_or_default()),
                Cell::from(time_ago(j.created_at)).style(Style::new().fg(MUTED)),
            ])
        })
        .collect();
    let mut job_state =
        TableState::default().with_selected((!ws_focused).then_some(app.home_job_sel));
    f.render_stateful_widget(
        Table::new(
            rows,
            [
                Constraint::Length(7),
                Constraint::Min(16),
                Constraint::Length(10),
                Constraint::Length(12),
                Constraint::Length(14),
            ],
        )
        .header(table_header([
            "JOB",
            "CONNECTION",
            "STATUS",
            "RECORDS",
            "WHEN",
        ]))
        .row_highlight_style(highlight())
        .block(focusable_panel(
            "Recent activity — all workspaces · ⏎ attempts",
            !ws_focused,
        )),
        right,
        &mut job_state,
    );
}

fn draw_workspace(f: &mut Frame, app: &App, area: Rect) {
    let [stats_area, tabs_area, body] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);
    draw_stats(f, app, stats_area, false);

    let titles: Vec<Line> = Tab::ALL
        .iter()
        .map(|t| {
            Line::from(format!(
                " {} {} ({}) ",
                t.index() + 1,
                t.title(),
                app.tab_len(*t)
            ))
        })
        .collect();
    f.render_widget(
        Tabs::new(titles)
            .select(app.tab.index())
            .style(Style::new().fg(MUTED))
            .highlight_style(Style::new().fg(ACCENT).bold()),
        tabs_area,
    );

    let sel = app.tab_sel[app.tab.index()];
    let mut state = TableState::default().with_selected(Some(sel));
    match app.tab {
        Tab::Connections => {
            let rows: Vec<Row> = app
                .connections
                .iter()
                .map(|c| {
                    let last = app
                        .ws_jobs
                        .iter()
                        .find(|j| j.connection_id == c.id)
                        .map(|j| (j.status.clone(), time_ago(j.created_at)));
                    Row::new(vec![
                        Cell::from(c.name.clone()),
                        Cell::from(status_span(&c.status)),
                        Cell::from(c.schedule_label()).style(Style::new().fg(MUTED)),
                        Cell::from(match &last {
                            Some((s, _)) => status_span(s),
                            None => Span::styled("never ran", Style::new().fg(MUTED)),
                        }),
                        Cell::from(last.map(|(_, t)| t).unwrap_or_default())
                            .style(Style::new().fg(MUTED)),
                    ])
                })
                .collect();
            f.render_stateful_widget(
                Table::new(
                    rows,
                    [
                        Constraint::Min(18),
                        Constraint::Length(10),
                        Constraint::Length(16),
                        Constraint::Length(11),
                        Constraint::Length(14),
                    ],
                )
                .header(table_header([
                    "NAME", "STATUS", "SCHEDULE", "LAST JOB", "WHEN",
                ]))
                .row_highlight_style(highlight())
                .block(panel("Connections — ⏎ open · s sync · p pause/resume")),
                body,
                &mut state,
            );
        }
        Tab::Jobs => {
            let rows: Vec<Row> = app
                .ws_jobs
                .iter()
                .map(|j| {
                    Row::new(vec![
                        Cell::from(format!("#{}", j.id)),
                        Cell::from(j.connection_name.clone()),
                        Cell::from(status_span(&j.status)),
                        Cell::from(j.records_synced.map(group_digits).unwrap_or_default()),
                        Cell::from(duration_label(j.started_at, j.completed_at))
                            .style(Style::new().fg(MUTED)),
                        Cell::from(time_ago(j.created_at)).style(Style::new().fg(MUTED)),
                    ])
                })
                .collect();
            f.render_stateful_widget(
                Table::new(
                    rows,
                    [
                        Constraint::Length(7),
                        Constraint::Min(16),
                        Constraint::Length(10),
                        Constraint::Length(12),
                        Constraint::Length(10),
                        Constraint::Length(14),
                    ],
                )
                .header(table_header([
                    "JOB",
                    "CONNECTION",
                    "STATUS",
                    "RECORDS",
                    "DURATION",
                    "WHEN",
                ]))
                .row_highlight_style(highlight())
                .block(panel("Jobs — ⏎ attempt history")),
                body,
                &mut state,
            );
        }
        Tab::Sources | Tab::Destinations => {
            let actors = if app.tab == Tab::Sources {
                &app.sources
            } else {
                &app.destinations
            };
            let rows: Vec<Row> = actors
                .iter()
                .map(|a| {
                    Row::new(vec![
                        Cell::from(a.name.clone()),
                        Cell::from(time_ago(a.created_at)).style(Style::new().fg(MUTED)),
                    ])
                })
                .collect();
            f.render_stateful_widget(
                Table::new(rows, [Constraint::Min(20), Constraint::Length(16)])
                    .header(table_header(["NAME", "CREATED"]))
                    .row_highlight_style(highlight())
                    .block(panel(app.tab.title())),
                body,
                &mut state,
            );
        }
    }
}

fn draw_connection(f: &mut Frame, app: &App, area: Rect) {
    let Some(conn) = &app.connection else { return };
    let [info_area, body] =
        Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(area);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            status_span(&conn.status),
            Span::styled("   schedule ", Style::new().fg(MUTED)),
            Span::raw(conn.schedule_label()),
            Span::styled("   state ", Style::new().fg(MUTED)),
            Span::raw(if app.conn_state.is_some() {
                "committed (v to inspect)"
            } else {
                "none yet"
            }),
        ]))
        .block(panel(&conn.name)),
        info_area,
    );

    let rows: Vec<Row> = app
        .conn_jobs
        .iter()
        .map(|j| {
            Row::new(vec![
                Cell::from(format!("#{}", j.id)),
                Cell::from(j.job_type.clone()),
                Cell::from(status_span(&j.status)),
                Cell::from(duration_label(j.started_at, j.completed_at))
                    .style(Style::new().fg(MUTED)),
                Cell::from(time_ago(j.created_at)).style(Style::new().fg(MUTED)),
            ])
        })
        .collect();
    let mut state = TableState::default().with_selected(Some(app.conn_sel));
    f.render_stateful_widget(
        Table::new(
            rows,
            [
                Constraint::Length(7),
                Constraint::Length(8),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Min(14),
            ],
        )
        .header(table_header(["JOB", "TYPE", "STATUS", "DURATION", "WHEN"]))
        .row_highlight_style(highlight())
        .block(panel(
            "Job history — s sync · p pause/resume · c cancel · ⏎ attempts",
        )),
        body,
        &mut state,
    );
}

// ---------- overlays ----------

fn draw_overlay(f: &mut Frame, app: &App) {
    match &app.overlay {
        Overlay::None => {}
        Overlay::Help => {
            let area = centered(58, 18, f.area());
            f.render_widget(Clear, area);
            let lines = vec![
                help_line("↑↓ / jk", "move selection"),
                help_line("⏎", "open / drill down"),
                help_line("esc / ⌫", "back"),
                help_line("⇥", "switch pane (fleet) / tab (workspace)"),
                help_line("1-4", "jump to workspace tab"),
                help_line("s", "trigger sync for connection"),
                help_line("p", "pause / resume connection"),
                help_line("c", "cancel selected job"),
                help_line("v", "inspect committed state (↑↓ scrolls)"),
                help_line("n", "new workspace (on fleet screen)"),
                help_line("r", "refresh now"),
                help_line("q / ctrl-c", "quit"),
                Line::default(),
                Line::from(Span::styled(
                    "Data auto-refreshes; mutations show up immediately.",
                    Style::new().fg(MUTED),
                )),
            ];
            f.render_widget(
                Paragraph::new(lines)
                    .block(panel("Keys"))
                    .wrap(Wrap { trim: false }),
                area,
            );
        }
        Overlay::Input(buf) => {
            let area = centered(48, 3, f.area());
            f.render_widget(Clear, area);
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::raw(buf.clone()),
                    Span::styled("▏", Style::new().fg(ACCENT)),
                ]))
                .block(panel("New workspace name — ⏎ create · esc cancel")),
                area,
            );
        }
        Overlay::StateJson { text, scroll } => {
            let area = centered(72, 20, f.area());
            f.render_widget(Clear, area);
            // Clamp so the last page stays on screen instead of scrolling
            // into emptiness.
            let lines = text.lines().count() as u16;
            let visible = area.height.saturating_sub(2);
            let offset = (*scroll).min(lines.saturating_sub(visible));
            f.render_widget(
                Paragraph::new(text.as_str())
                    .scroll((offset, 0))
                    .block(panel("Committed state — ↑↓ scroll · esc close"))
                    .wrap(Wrap { trim: false }),
                area,
            );
        }
        Overlay::JobDetail(detail) => {
            let area = centered(64, 14, f.area());
            f.render_widget(Clear, area);
            let rows: Vec<Row> = detail
                .attempts
                .iter()
                .map(|a| {
                    Row::new(vec![
                        Cell::from(format!("{}", a.attempt_number)),
                        Cell::from(status_span(&a.status)),
                        Cell::from(a.records_synced.map(group_digits).unwrap_or_default()),
                        Cell::from(duration_label(Some(a.created_at), a.ended_at))
                            .style(Style::new().fg(MUTED)),
                    ])
                })
                .collect();
            f.render_widget(
                Table::new(
                    rows,
                    [
                        Constraint::Length(4),
                        Constraint::Length(10),
                        Constraint::Length(12),
                        Constraint::Min(10),
                    ],
                )
                .header(table_header(["#", "STATUS", "RECORDS", "DURATION"]))
                .block(panel(&format!(
                    "Job #{} — {} — esc to close",
                    detail.id, detail.status
                ))),
                area,
            );
        }
    }
}

fn help_line(key: &str, action: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:>10}  "), Style::new().fg(ACCENT).bold()),
        Span::raw(action.to_string()),
    ])
}

// ---------- shared bits ----------

fn panel(title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::Rgb(35, 44, 77)))
        .title(Span::styled(format!(" {title} "), Style::new().fg(MUTED)))
}

/// A panel whose border and title light up when its pane owns the keyboard.
fn focusable_panel(title: &str, focused: bool) -> Block<'static> {
    if focused {
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(ACCENT))
            .title(Span::styled(
                format!(" {title} "),
                Style::new().fg(ACCENT).bold(),
            ))
    } else {
        panel(title)
    }
}

fn table_header<const N: usize>(titles: [&'static str; N]) -> Row<'static> {
    Row::new(
        titles
            .into_iter()
            .map(|t| Cell::from(Span::styled(t, Style::new().fg(MUTED).bold()))),
    )
}

fn highlight() -> Style {
    Style::new()
        .bg(Color::Rgb(26, 34, 64))
        .add_modifier(Modifier::BOLD)
}

fn status_span(status: &str) -> Span<'static> {
    let color = match status {
        "succeeded" | "active" => OK,
        "running" | "pending" => WARN,
        "failed" => ERR,
        _ => MUTED,
    };
    Span::styled(status.to_string(), Style::new().fg(color))
}

fn centered(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width.saturating_sub(2));
    let h = height.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

fn group_digits(n: i64) -> String {
    let raw = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in raw.chars().enumerate() {
        if i > 0 && (raw.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

fn time_ago(t: DateTime<Utc>) -> String {
    let secs = (Utc::now() - t).num_seconds().max(0);
    match secs {
        0..=59 => format!("{secs}s ago"),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

fn duration_label(start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) -> String {
    let Some(start) = start else {
        return String::new();
    };
    let end = end.unwrap_or_else(Utc::now);
    let secs = (end - start).num_seconds().max(0);
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60)
    }
}
