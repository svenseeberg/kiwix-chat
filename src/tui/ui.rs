use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::app::{App, DisplayKind, DisplayMessage};

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

/// A run of text sharing one style. The unit of markdown-aware wrapping.
type Seg = (String, Style);

/// Build the fully wrapped, styled line list for the chat transcript.
fn build_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    for msg in &app.messages {
        match msg.kind {
            DisplayKind::Thinking => render_thinking(&mut lines, msg, app.show_thinking, width),
            // Only assistant answers get markdown formatting.
            DisplayKind::Assistant => render_message(&mut lines, msg, width, true),
            _ => render_message(&mut lines, msg, width, false),
        }
        // Blank separator line between messages.
        lines.push(Line::from(""));
    }
    lines
}

/// Style + short label for a (non-thinking) message kind.
fn style_for(kind: DisplayKind) -> (&'static str, Style) {
    match kind {
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
        // Thinking is rendered by render_thinking; never reached here.
        DisplayKind::Thinking => ("thinking", thinking_style()),
    }
}

fn thinking_style() -> Style {
    Style::default()
        .fg(Color::Magenta)
        .add_modifier(Modifier::ITALIC | Modifier::DIM)
}

fn code_inline_style() -> Style {
    Style::default().fg(Color::Yellow)
}

fn code_block_style() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::DIM)
}

/// Render a collapsible reasoning block. When collapsed it is a single header
/// line; when expanded (either still streaming, or the global toggle is on) the
/// reasoning text follows, dim and italic.
fn render_thinking(lines: &mut Vec<Line<'static>>, msg: &DisplayMessage, show: bool, width: usize) {
    let style = thinking_style();
    let expanded = show || !msg.collapsed;
    let arrow = if expanded { "▾" } else { "▸" };
    let header = if expanded {
        format!("{arrow} thinking")
    } else {
        let n = msg.text.lines().count().max(1);
        format!("{arrow} thinking ({n} lines) — Tab to expand")
    };
    lines.push(Line::from(Span::styled(header, style)));

    if expanded {
        let content_width = width.saturating_sub(2).max(1);
        for raw in msg.text.split('\n') {
            for wl in wrap_segments(&[(raw.to_string(), style)], content_width) {
                let mut spans = vec![Span::raw("  ")];
                spans.extend(wl.into_iter().map(|(t, s)| Span::styled(t, s)));
                lines.push(Line::from(spans));
            }
        }
    }
}

/// Render a labelled message, optionally applying simple markdown formatting.
fn render_message(
    lines: &mut Vec<Line<'static>>,
    msg: &DisplayMessage,
    width: usize,
    markdown: bool,
) {
    let (label, style) = style_for(msg.kind);
    let prefix = format!("{label}: ");
    let indent = " ".repeat(prefix.len().min(width.saturating_sub(1)));
    let content_width = width.saturating_sub(prefix.len()).max(1);

    let mut first = true;
    let mut in_code = false;
    for raw in msg.text.split('\n') {
        // Fenced code block delimiters toggle verbatim mode and aren't rendered.
        if markdown && raw.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }

        let (marker, marker_style, segs, hang) = if markdown && in_code {
            (
                String::new(),
                style,
                vec![(raw.to_string(), code_block_style())],
                0,
            )
        } else if markdown {
            let b = classify_block(raw, style);
            (
                b.marker,
                b.marker_style,
                parse_inline(&b.text, b.style),
                b.hang,
            )
        } else {
            (String::new(), style, vec![(raw.to_string(), style)], 0)
        };

        let wrap_w = content_width.saturating_sub(hang).max(1);
        for (i, wl) in wrap_segments(&segs, wrap_w).into_iter().enumerate() {
            let mut spans: Vec<Span<'static>> = Vec::new();
            if first {
                spans.push(Span::styled(prefix.clone(), style));
                first = false;
            } else {
                spans.push(Span::raw(indent.clone()));
            }
            if i == 0 && !marker.is_empty() {
                spans.push(Span::styled(marker.clone(), marker_style));
            } else if hang > 0 {
                spans.push(Span::raw(" ".repeat(hang)));
            }
            spans.extend(wl.into_iter().map(|(t, s)| Span::styled(t, s)));
            lines.push(Line::from(spans));
        }
    }
    // Guarantee at least the label line for an empty message.
    if first {
        lines.push(Line::from(Span::styled(prefix, style)));
    }
}

/// A single markdown block line decomposed into a leading marker and body text.
struct MdBlock {
    text: String,
    style: Style,
    marker: String,
    marker_style: Style,
    /// Columns to indent wrapped continuation lines by (marker width).
    hang: usize,
}

/// Classify one logical line as a heading, list item, blockquote, or paragraph.
fn classify_block(raw: &str, base: Style) -> MdBlock {
    let trimmed = raw.trim_start();

    // ATX heading: 1–6 leading '#' followed by a space.
    let hashes = trimmed.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
        return MdBlock {
            text: trimmed[hashes..].trim_start().to_string(),
            style: base.add_modifier(Modifier::BOLD),
            marker: String::new(),
            marker_style: base,
            hang: 0,
        };
    }

    // Blockquote.
    if let Some(rest) = trimmed.strip_prefix("> ") {
        let s = Style::default().fg(Color::DarkGray);
        return MdBlock {
            text: rest.to_string(),
            style: s,
            marker: "│ ".to_string(),
            marker_style: s,
            hang: 2,
        };
    }

    // Unordered list item.
    for p in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(p) {
            return MdBlock {
                text: rest.to_string(),
                style: base,
                marker: "• ".to_string(),
                marker_style: base,
                hang: 2,
            };
        }
    }

    // Ordered list item: digits then ". ".
    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        if let Some(rest) = trimmed[digits..].strip_prefix(". ") {
            let marker = format!("{}. ", &trimmed[..digits]);
            let hang = marker.chars().count();
            return MdBlock {
                text: rest.to_string(),
                style: base,
                marker,
                marker_style: base,
                hang,
            };
        }
    }

    // Plain paragraph (keep original leading whitespace).
    MdBlock {
        text: raw.to_string(),
        style: base,
        marker: String::new(),
        marker_style: base,
        hang: 0,
    }
}

/// Parse inline `**bold**`, `*italic*`, and `` `code` `` into styled segments.
/// Non-recursive (no nested emphasis); unmatched delimiters are treated as text.
fn parse_inline(text: &str, base: Style) -> Vec<Seg> {
    let ch: Vec<char> = text.chars().collect();
    let n = ch.len();
    let mut segs: Vec<Seg> = Vec::new();
    let mut cur = String::new();
    let mut i = 0;
    while i < n {
        let c = ch[i];
        // Inline code.
        if c == '`' {
            if let Some(close) = (i + 1..n).find(|&j| ch[j] == '`') {
                let code: String = ch[i + 1..close].iter().collect();
                if !cur.is_empty() {
                    segs.push((std::mem::take(&mut cur), base));
                }
                if !code.is_empty() {
                    segs.push((code, code_inline_style()));
                }
                i = close + 1;
                continue;
            }
        }
        // Bold.
        if c == '*' && i + 1 < n && ch[i + 1] == '*' {
            if let Some(close) = find_double_star(&ch, i + 2) {
                let inner: String = ch[i + 2..close].iter().collect();
                if !inner.is_empty() {
                    if !cur.is_empty() {
                        segs.push((std::mem::take(&mut cur), base));
                    }
                    segs.push((inner, base.add_modifier(Modifier::BOLD)));
                    i = close + 2;
                    continue;
                }
            }
        }
        // Italic.
        if c == '*' && i + 1 < n && !ch[i + 1].is_whitespace() {
            if let Some(close) = find_single_star(&ch, i + 1) {
                let inner: String = ch[i + 1..close].iter().collect();
                if !inner.is_empty() {
                    if !cur.is_empty() {
                        segs.push((std::mem::take(&mut cur), base));
                    }
                    segs.push((inner, base.add_modifier(Modifier::ITALIC)));
                    i = close + 1;
                    continue;
                }
            }
        }
        cur.push(c);
        i += 1;
    }
    if !cur.is_empty() {
        segs.push((cur, base));
    }
    if segs.is_empty() {
        segs.push((String::new(), base));
    }
    segs
}

/// Index of the next `**` at or after `from`.
fn find_double_star(ch: &[char], from: usize) -> Option<usize> {
    let mut j = from;
    while j + 1 < ch.len() {
        if ch[j] == '*' && ch[j + 1] == '*' {
            return Some(j);
        }
        j += 1;
    }
    None
}

/// Index of the next single `*` closing an emphasis span (preceded by a
/// non-space and not part of a `**`), searching from `from` (which is > 0).
fn find_single_star(ch: &[char], from: usize) -> Option<usize> {
    let n = ch.len();
    let mut j = from;
    while j < n {
        if ch[j] == '*' && !ch[j - 1].is_whitespace() && !(j + 1 < n && ch[j + 1] == '*') {
            return Some(j);
        }
        j += 1;
    }
    None
}

/// Word-wrap a styled segment list to `width` columns, preserving each segment's
/// style. Words longer than `width` are hard-split.
fn wrap_segments(segs: &[Seg], width: usize) -> Vec<Vec<Seg>> {
    let width = width.max(1);
    let mut lines: Vec<Vec<Seg>> = Vec::new();
    let mut line: Vec<Seg> = Vec::new();
    let mut line_w = 0usize;
    let mut word: Vec<Seg> = Vec::new();
    let mut word_w = 0usize;

    for (text, style) in segs {
        for c in text.chars() {
            if c == ' ' {
                if word_w > 0 {
                    place_word(&mut lines, &mut line, &mut line_w, &word, word_w, width);
                    word.clear();
                    word_w = 0;
                }
            } else {
                append_char(&mut word, c, *style);
                word_w += 1;
                if word_w == width {
                    // Hard-split an over-long word onto its own line(s).
                    if !line.is_empty() {
                        lines.push(std::mem::take(&mut line));
                        line_w = 0;
                    }
                    lines.push(std::mem::take(&mut word));
                    word_w = 0;
                }
            }
        }
    }
    if word_w > 0 {
        place_word(&mut lines, &mut line, &mut line_w, &word, word_w, width);
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// Append one styled word to the current line, wrapping first if it doesn't fit.
fn place_word(
    lines: &mut Vec<Vec<Seg>>,
    line: &mut Vec<Seg>,
    line_w: &mut usize,
    word: &[Seg],
    word_w: usize,
    width: usize,
) {
    if *line_w > 0 && *line_w + 1 + word_w > width {
        lines.push(std::mem::take(line));
        *line_w = 0;
    }
    if *line_w > 0 {
        append_char(line, ' ', Style::default());
        *line_w += 1;
    }
    for (t, s) in word {
        for c in t.chars() {
            append_char(line, c, *s);
        }
    }
    *line_w += word_w;
}

/// Append a char to a segment list, extending the last run if the style matches.
fn append_char(dst: &mut Vec<Seg>, c: char, style: Style) {
    if let Some(last) = dst.last_mut() {
        if last.1 == style {
            last.0.push(c);
            return;
        }
    }
    dst.push((c.to_string(), style));
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
        Span::raw("  PgUp/PgDn scroll · Tab thinking · /quit"),
    ]);
    f.render_widget(Paragraph::new(line), area);
}
