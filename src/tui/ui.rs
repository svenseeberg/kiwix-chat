use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::app::{App, DisplayKind};

/// Render the whole UI: chat pane, input box, and status bar.
pub fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),    // chat
            Constraint::Length(3), // input
            Constraint::Length(1), // status
        ])
        .split(f.area());

    draw_chat(f, app, chunks[0]);
    draw_input(f, app, chunks[1]);
    draw_status(f, app, chunks[2]);
}

fn draw_chat(f: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title(" kiwix-chat ");
    let inner = block.inner(area);
    let width = inner.width.max(1) as usize;
    let height = inner.height as usize;

    let lines = build_lines(app, width);
    let total = lines.len();

    // Auto-follow the bottom, or clamp a user-set scroll offset.
    let max_scroll = total.saturating_sub(height) as u16;
    if app.follow {
        app.scroll = max_scroll;
    } else if app.scroll > max_scroll {
        app.scroll = max_scroll;
    }

    let paragraph = Paragraph::new(lines).block(block).scroll((app.scroll, 0));
    f.render_widget(paragraph, area);
}

/// Build the fully wrapped, styled line list for the chat transcript.
fn build_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    for msg in &app.messages {
        let (label, style) = match msg.kind {
            DisplayKind::User => (
                "You",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            DisplayKind::Assistant => ("AI", Style::default().fg(Color::Green)),
            DisplayKind::Tool => (
                "·",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ),
            DisplayKind::Error => (
                "!",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            DisplayKind::Info => ("i", Style::default().fg(Color::Yellow)),
        };

        let prefix = format!("{label}: ");
        let indent = " ".repeat(prefix.len().min(width.saturating_sub(1)));
        let content_width = width.saturating_sub(prefix.len()).max(1);

        let mut first = true;
        for raw_line in msg.text.split('\n') {
            for wrapped in wrap(raw_line, content_width) {
                if first {
                    lines.push(Line::from(vec![
                        Span::styled(prefix.clone(), style),
                        Span::styled(wrapped, style),
                    ]));
                    first = false;
                } else {
                    lines.push(Line::from(vec![
                        Span::raw(indent.clone()),
                        Span::styled(wrapped, style),
                    ]));
                }
            }
        }
        // Blank separator line between messages.
        lines.push(Line::from(""));
    }
    lines
}

/// Word-wrap a single logical line to `width` columns. Long words are hard-split.
fn wrap(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut current = String::new();
    for word in text.split(' ') {
        if word.chars().count() > width {
            // Flush current, then hard-split the long word.
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            let mut chunk = String::new();
            for c in word.chars() {
                if chunk.chars().count() == width {
                    out.push(std::mem::take(&mut chunk));
                }
                chunk.push(c);
            }
            current = chunk;
            continue;
        }
        let extra = if current.is_empty() { 0 } else { 1 };
        if current.chars().count() + extra + word.chars().count() > width {
            out.push(std::mem::take(&mut current));
            current.push_str(word);
        } else {
            if extra == 1 {
                current.push(' ');
            }
            current.push_str(word);
        }
    }
    if !current.is_empty() || out.is_empty() {
        out.push(current);
    }
    out
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let title = if app.busy {
        " thinking… (input disabled) "
    } else {
        " message "
    };
    let style = if app.busy {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(style);
    let text = format!("> {}", app.input);
    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, area);

    if !app.busy {
        // Place the cursor at the end of the input text.
        let x = area.x + 3 + app.input.chars().count() as u16;
        let y = area.y + 1;
        f.set_cursor_position((x.min(area.x + area.width.saturating_sub(2)), y));
    }
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let kiwix_state = if app.kiwix_reachable {
        Span::styled("kiwix✓", Style::default().fg(Color::Green))
    } else {
        Span::styled("kiwix✗", Style::default().fg(Color::Red))
    };
    let status = if app.busy { "busy" } else { "ready" };
    let line = Line::from(vec![
        Span::styled(
            format!(" {} ", app.llm.model()),
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::raw(" "),
        kiwix_state,
        Span::raw(format!("  lang:{}  ", app.lang)),
        Span::styled(status, Style::default().add_modifier(Modifier::DIM)),
        Span::raw("  PgUp/PgDn scroll · /quit"),
    ]);
    f.render_widget(Paragraph::new(line), area);
}
