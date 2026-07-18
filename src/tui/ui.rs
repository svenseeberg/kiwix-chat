use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::app::{App, DisplayKind, DisplayMessage};

/// Muted palette — soft, low-saturation tones so no single color reads as loud.
const FG_TEXT: Color = Color::Rgb(0xCC, 0xCC, 0xCC); // soft white body text
const ACCENT_USER: Color = Color::Rgb(0x7A, 0xA2, 0xC0); // muted blue
const ACCENT_AI: Color = Color::Rgb(0x8F, 0xB0, 0x8F); // sage green
const ACCENT_TOOL: Color = Color::Rgb(0xC2, 0xA5, 0x6B); // muted gold
const ACCENT_CODE: Color = Color::Rgb(0xC2, 0xA5, 0x6B); // muted gold
const ACCENT_INFO: Color = Color::Rgb(0xB0, 0x9A, 0x6B); // muted amber
const ACCENT_ERR: Color = Color::Rgb(0xC0, 0x7A, 0x7A); // muted red
const ACCENT_LINK: Color = Color::Rgb(0x8A, 0xB4, 0xC8); // muted cyan for links
const FG_DIM: Color = Color::Rgb(0x80, 0x80, 0x80); // grey for subdued text

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
                .fg(ACCENT_USER)
                .add_modifier(Modifier::BOLD),
        ),
        DisplayKind::Assistant => ("AI", Style::default().fg(ACCENT_AI)),
        DisplayKind::Tool => (
            "·",
            Style::default()
                .fg(ACCENT_TOOL)
                .add_modifier(Modifier::ITALIC),
        ),
        DisplayKind::Error => (
            "!",
            Style::default().fg(ACCENT_ERR).add_modifier(Modifier::BOLD),
        ),
        DisplayKind::Info => ("i", Style::default().fg(ACCENT_INFO)),
        // Thinking is rendered by render_thinking; never reached here.
        DisplayKind::Thinking => ("thinking", thinking_style()),
    }
}

fn thinking_style() -> Style {
    Style::default()
        .fg(FG_DIM)
        .add_modifier(Modifier::ITALIC | Modifier::DIM)
}

fn code_inline_style() -> Style {
    Style::default().fg(ACCENT_CODE)
}

fn code_block_style() -> Style {
    Style::default().fg(ACCENT_CODE).add_modifier(Modifier::DIM)
}

/// Style for the visible label of a link (underlined so it reads as a link).
fn link_label_style() -> Style {
    Style::default()
        .fg(ACCENT_LINK)
        .add_modifier(Modifier::UNDERLINED)
}

/// Style for the trailing raw URL. Left un-underlined so terminals that
/// auto-detect URLs can linkify the clean text.
fn link_url_style() -> Style {
    Style::default().fg(ACCENT_LINK)
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
    // The label keeps its accent color; assistant answers render their body in
    // soft white so the accent stays a small marker rather than tinting the text.
    let body = if matches!(msg.kind, DisplayKind::Assistant) {
        style.fg(FG_TEXT)
    } else {
        style
    };
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
                body,
                vec![(raw.to_string(), code_block_style())],
                0,
            )
        } else if markdown {
            let normalized = normalize_math(raw);
            let b = classify_block(&normalized, body);
            (
                b.marker,
                b.marker_style,
                parse_inline(&b.text, b.style),
                b.hang,
            )
        } else {
            (String::new(), body, vec![(raw.to_string(), body)], 0)
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
        let s = Style::default().fg(FG_DIM);
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

/// Best-effort conversion of stray LaTeX math into plain Unicode text.
///
/// The system prompt already asks the model to avoid LaTeX; this is a conservative
/// fallback for when it doesn't. It strips math delimiters and maps a handful of
/// common commands to Unicode. Lines with neither `\` nor `$` are returned untouched,
/// which keeps ordinary prose and citation URLs unaffected.
fn normalize_math(s: &str) -> String {
    if !s.contains('\\') && !s.contains('$') {
        return s.to_string();
    }
    // Inline/display math delimiters carry no meaning once rendered as text.
    let mut out = s
        .replace("\\(", "")
        .replace("\\)", "")
        .replace("\\[", "")
        .replace("\\]", "");
    out = strip_dollar_math(&out);
    out = replace_wrapped(&out, "\\frac", 2, |args| {
        format!("({})/({})", args[0], args[1])
    });
    out = replace_wrapped(&out, "\\sqrt", 1, |args| format!("√({})", args[0]));
    // Longer command names must precede any of their own prefixes (e.g. \leq before \le).
    for (cmd, sym) in COMMAND_SYMBOLS {
        out = out.replace(cmd, sym);
    }
    out = replace_superscripts(&out);
    out
}

/// LaTeX command → Unicode replacements, ordered so longer names come before prefixes.
const COMMAND_SYMBOLS: &[(&str, &str)] = &[
    ("\\times", "×"),
    ("\\cdot", "·"),
    ("\\div", "÷"),
    ("\\pm", "±"),
    ("\\mp", "∓"),
    ("\\leq", "≤"),
    ("\\le", "≤"),
    ("\\geq", "≥"),
    ("\\ge", "≥"),
    ("\\neq", "≠"),
    ("\\approx", "≈"),
    ("\\equiv", "≡"),
    ("\\infty", "∞"),
    ("\\sum", "∑"),
    ("\\prod", "∏"),
    ("\\int", "∫"),
    ("\\partial", "∂"),
    ("\\nabla", "∇"),
    ("\\alpha", "α"),
    ("\\beta", "β"),
    ("\\gamma", "γ"),
    ("\\delta", "δ"),
    ("\\epsilon", "ε"),
    ("\\theta", "θ"),
    ("\\lambda", "λ"),
    ("\\mu", "μ"),
    ("\\pi", "π"),
    ("\\rho", "ρ"),
    ("\\sigma", "σ"),
    ("\\tau", "τ"),
    ("\\phi", "φ"),
    ("\\omega", "ω"),
    ("\\Delta", "Δ"),
    ("\\Sigma", "Σ"),
    ("\\Omega", "Ω"),
    ("\\Rightarrow", "⇒"),
    ("\\rightarrow", "→"),
    ("\\to", "→"),
    ("\\leftarrow", "←"),
    ("\\ldots", "…"),
    ("\\dots", "…"),
    ("\\left", ""),
    ("\\right", ""),
    ("\\quad", "  "),
    ("\\,", " "),
    ("\\;", " "),
    ("\\!", ""),
];

/// Remove `$$…$$` display delimiters, and `$…$` inline delimiters only when the enclosed
/// text carries a LaTeX signal (`\`, `^`, `_`, `{`). This leaves currency like `$5` intact.
fn strip_dollar_math(s: &str) -> String {
    let s = s.replace("$$", "");
    let ch: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < ch.len() {
        if ch[i] == '$' {
            if let Some(close) = (i + 1..ch.len()).find(|&j| ch[j] == '$') {
                let inner: String = ch[i + 1..close].iter().collect();
                let mathy = inner.contains('\\')
                    || inner.contains('^')
                    || inner.contains('_')
                    || inner.contains('{');
                if mathy {
                    out.push_str(&inner);
                    i = close + 1;
                    continue;
                }
            }
        }
        out.push(ch[i]);
        i += 1;
    }
    out
}

/// Replace occurrences of `cmd` followed by `argc` `{…}` groups using `render`.
/// Argument contents are normalized recursively so nested commands are handled.
fn replace_wrapped(
    s: &str,
    cmd: &str,
    argc: usize,
    render: impl Fn(&[String]) -> String,
) -> String {
    if !s.contains(cmd) {
        return s.to_string();
    }
    let ch: Vec<char> = s.chars().collect();
    let target: Vec<char> = cmd.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < ch.len() {
        if ch[i..].starts_with(target.as_slice()) {
            let mut j = i + target.len();
            let mut args: Vec<String> = Vec::with_capacity(argc);
            while args.len() < argc {
                match take_brace(&ch, j) {
                    Some((inner, next)) => {
                        args.push(normalize_math(&inner));
                        j = next;
                    }
                    None => break,
                }
            }
            if args.len() == argc {
                out.push_str(&render(&args));
                i = j;
                continue;
            }
        }
        out.push(ch[i]);
        i += 1;
    }
    out
}

/// Read a `{…}` group starting at `start`, returning its contents and the index past `}`.
fn take_brace(ch: &[char], start: usize) -> Option<(String, usize)> {
    if start >= ch.len() || ch[start] != '{' {
        return None;
    }
    let mut depth = 1;
    let mut inner = String::new();
    let mut j = start + 1;
    while j < ch.len() {
        match ch[j] {
            '{' => {
                depth += 1;
                inner.push('{');
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((inner, j + 1));
                }
                inner.push('}');
            }
            c => inner.push(c),
        }
        j += 1;
    }
    None
}

/// Convert `^2`, `^n`, and `^{…}` (when every char maps) into Unicode superscripts.
fn replace_superscripts(s: &str) -> String {
    if !s.contains('^') {
        return s.to_string();
    }
    let ch: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < ch.len() {
        if ch[i] == '^' && i + 1 < ch.len() {
            if ch[i + 1] == '{' {
                if let Some((inner, next)) = take_brace(&ch, i + 1) {
                    let mapped: Option<String> = inner.chars().map(superscript).collect();
                    if let Some(m) = mapped.filter(|m| !m.is_empty()) {
                        out.push_str(&m);
                        i = next;
                        continue;
                    }
                }
            } else if let Some(m) = superscript(ch[i + 1]) {
                out.push(m);
                i += 2;
                continue;
            }
        }
        out.push(ch[i]);
        i += 1;
    }
    out
}

/// Unicode superscript for a small set of characters, if one exists.
fn superscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '+' => '⁺',
        '-' => '⁻',
        'n' => 'ⁿ',
        'i' => 'ⁱ',
        _ => return None,
    })
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
        // Markdown link: [label](url). Rendered as "label (url)" so the raw URL
        // stays visible and terminals that auto-detect URLs can linkify it.
        if c == '[' {
            if let Some((label, url, next)) = parse_link(&ch, i) {
                if !cur.is_empty() {
                    segs.push((std::mem::take(&mut cur), base));
                }
                if !label.is_empty() {
                    segs.push((label, link_label_style()));
                }
                segs.push((" (".to_string(), base));
                segs.push((url, link_url_style()));
                segs.push((")".to_string(), base));
                i = next;
                continue;
            }
        }
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

/// Parse a `[label](url)` link starting at `start` (the `[`).
/// Returns `(label, url, next_index)` where `next_index` is just past the `)`.
/// Returns `None` (so the `[` is treated as literal text) if the syntax doesn't match
/// or the URL is empty. Labels/URLs containing `]`, `(`, or `)` are not supported.
fn parse_link(ch: &[char], start: usize) -> Option<(String, String, usize)> {
    let n = ch.len();
    // Closing ']' of the label.
    let close_label = (start + 1..n).find(|&j| ch[j] == ']')?;
    // Must be immediately followed by '('.
    if close_label + 1 >= n || ch[close_label + 1] != '(' {
        return None;
    }
    // Closing ')' of the URL.
    let close_url = (close_label + 2..n).find(|&j| ch[j] == ')')?;
    let label: String = ch[start + 1..close_label].iter().collect();
    let url: String = ch[close_label + 2..close_url].iter().collect();
    if url.trim().is_empty() {
        return None;
    }
    Some((label, url, close_url + 1))
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
        Style::default().fg(FG_DIM)
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
        Span::styled("kiwix✓", Style::default().fg(ACCENT_AI))
    } else {
        Span::styled("kiwix✗", Style::default().fg(ACCENT_ERR))
    };
    let status = if app.busy { "busy" } else { "ready" };
    let line = Line::from(vec![
        Span::styled(
            format!(" {} ", app.llm.model()),
            Style::default().fg(Color::Black).bg(ACCENT_USER),
        ),
        Span::raw(" "),
        kiwix_state,
        Span::raw(format!("  lang:{}  ", app.lang)),
        Span::styled(status, Style::default().add_modifier(Modifier::DIM)),
        Span::raw("  PgUp/PgDn scroll · Tab thinking · /quit"),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Concatenate the text of a parsed inline segment list.
    fn text_of(segs: &[Seg]) -> String {
        segs.iter().map(|(t, _)| t.as_str()).collect()
    }

    #[test]
    fn renders_link_as_label_and_visible_url() {
        let base = Style::default();
        let segs = parse_inline(
            "See [Ada Lovelace](http://localhost:8080/content/w/A/Ada)",
            base,
        );
        assert_eq!(
            text_of(&segs),
            "See Ada Lovelace (http://localhost:8080/content/w/A/Ada)"
        );
        // The label carries the underlined link style.
        assert!(segs
            .iter()
            .any(|(t, s)| t == "Ada Lovelace" && *s == link_label_style()));
        // The URL is present as its own styled segment.
        assert!(segs
            .iter()
            .any(|(t, s)| t.contains("localhost") && *s == link_url_style()));
    }

    #[test]
    fn leaves_non_link_brackets_as_text() {
        let base = Style::default();
        let segs = parse_inline("an array a[i] and (parens)", base);
        assert_eq!(text_of(&segs), "an array a[i] and (parens)");
    }

    #[test]
    fn normalizes_common_latex() {
        assert_eq!(
            normalize_math("Energy is \\(E = mc^2\\)."),
            "Energy is E = mc²."
        );
        assert_eq!(normalize_math("$$\\frac{a}{b}$$"), "(a)/(b)");
        assert_eq!(
            normalize_math("speed \\approx 3 \\times 10^8"),
            "speed ≈ 3 × 10⁸"
        );
        assert_eq!(normalize_math("$\\sqrt{2}$"), "√(2)");
    }

    #[test]
    fn leaves_plain_text_and_currency_untouched() {
        assert_eq!(normalize_math("no math here at all"), "no math here at all");
        assert_eq!(
            normalize_math("it costs $5 or $10 total"),
            "it costs $5 or $10 total"
        );
    }
}
