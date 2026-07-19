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
    let area = f.area();
    // The input box grows with its wrapped content, but never so tall that the
    // chat pane drops below its 3-line minimum.
    let rows = input_rows(&app.input, input_inner_width(area.width));
    let max_content = (area.height as usize).saturating_sub(1 + 3 + 2).max(1);
    let input_height = (rows.min(max_content) + 2) as u16;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),               // chat
            Constraint::Length(input_height), // input
            Constraint::Length(1),            // status
        ])
        .split(area);

    draw_chat(f, app, chunks[0]);
    draw_input(f, app, chunks[1]);
    draw_status(f, app, chunks[2]);
}

/// Usable text width inside the input box (total width minus the two borders).
fn input_inner_width(total_width: u16) -> usize {
    (total_width.saturating_sub(2)).max(1) as usize
}

/// Number of rows the prompt line `"> {input}"` occupies once hard-wrapped to
/// `width` columns. The trailing cursor position counts, so a line filled exactly
/// to `width` reports one extra row (the cursor sits on a fresh line).
fn input_rows(input: &str, width: usize) -> usize {
    let cols = 2 + input.chars().count(); // "> " prefix + text
    cols / width + 1
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
            DisplayKind::Subagent => render_subagent(&mut lines, msg, app.show_thinking, width),
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
        // Thinking and Subagent are rendered by their own functions; never here.
        DisplayKind::Thinking => ("thinking", thinking_style()),
        DisplayKind::Subagent => ("research", subagent_style()),
    }
}

fn subagent_style() -> Style {
    Style::default().fg(ACCENT_TOOL).add_modifier(Modifier::DIM)
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

/// Render a collapsible `research` sub-agent block. Collapsed it is a single
/// header line showing the question; expanded (still streaming has already
/// finished for these, so this is the global Tab toggle or an un-collapsed block)
/// the cited answer follows, dim and markdown-formatted.
fn render_subagent(lines: &mut Vec<Line<'static>>, msg: &DisplayMessage, show: bool, width: usize) {
    let style = subagent_style();
    let expanded = show || !msg.collapsed;
    let arrow = if expanded { "▾" } else { "▸" };
    let question = msg.title.as_deref().unwrap_or("(question)");
    let header = if expanded {
        format!("{arrow} research: \"{question}\"")
    } else {
        let n = msg.text.lines().count().max(1);
        format!("{arrow} research: \"{question}\" ({n} lines) — Tab to expand")
    };
    lines.push(Line::from(Span::styled(header, style)));

    if expanded {
        let indent = "  ";
        let content_width = width.saturating_sub(indent.len()).max(1);
        // `first` starts false so the answer body uses the indent (the header above
        // already stands in for the prefix line).
        let mut first = false;
        render_body(
            lines,
            &msg.text,
            content_width,
            true,
            "",
            style,
            indent,
            style,
            &mut first,
        );
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
    render_body(
        lines,
        &msg.text,
        content_width,
        markdown,
        &prefix,
        style,
        &indent,
        body,
        &mut first,
    );
    // Guarantee at least the label line for an empty message.
    if first {
        lines.push(Line::from(Span::styled(prefix, style)));
    }
}

/// Render message body text into wrapped, styled lines. The first physical line
/// gets `prefix` (styled with `prefix_style`); wrapped continuations get `indent`.
/// `body` is the base style for the text. When `markdown` is true, fenced code
/// blocks, GFM tables, and inline/block markdown are interpreted.
///
/// `first` tracks whether the prefix has been emitted yet, so callers can render
/// a header line ahead of the body and still have the body indent correctly.
#[allow(clippy::too_many_arguments)]
fn render_body(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    content_width: usize,
    markdown: bool,
    prefix: &str,
    prefix_style: Style,
    indent: &str,
    body: Style,
    first: &mut bool,
) {
    let mut in_code = false;
    let raw_lines: Vec<&str> = text.split('\n').collect();
    let mut li = 0;
    while li < raw_lines.len() {
        let raw = raw_lines[li];
        // Fenced code block delimiters toggle verbatim mode and aren't rendered.
        if markdown && raw.trim_start().starts_with("```") {
            in_code = !in_code;
            li += 1;
            continue;
        }

        // GFM table: a header row followed by a delimiter row. Rendered as a whole
        // block so column widths can be computed across every row at once.
        if markdown && !in_code && is_table_start(&raw_lines, li) {
            let start = li;
            li += 1; // header
            li += 1; // delimiter
            while li < raw_lines.len() && raw_lines[li].contains('|') {
                li += 1;
            }
            let table_lines = render_table(&raw_lines[start..li], content_width);
            emit_lines(lines, table_lines, prefix, prefix_style, indent, first);
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
            if *first {
                spans.push(Span::styled(prefix.to_string(), prefix_style));
                *first = false;
            } else {
                spans.push(Span::raw(indent.to_string()));
            }
            if i == 0 && !marker.is_empty() {
                spans.push(Span::styled(marker.clone(), marker_style));
            } else if hang > 0 {
                spans.push(Span::raw(" ".repeat(hang)));
            }
            spans.extend(wl.into_iter().map(|(t, s)| Span::styled(t, s)));
            lines.push(Line::from(spans));
        }
        li += 1;
    }
}

/// Push a block of already-styled segment lines into the transcript, giving the
/// very first physical line the message prefix and the rest the hanging indent.
fn emit_lines(
    lines: &mut Vec<Line<'static>>,
    block: Vec<Vec<Seg>>,
    prefix: &str,
    prefix_style: Style,
    indent: &str,
    first: &mut bool,
) {
    for seg_line in block {
        let mut spans: Vec<Span<'static>> = Vec::new();
        if *first {
            spans.push(Span::styled(prefix.to_string(), prefix_style));
            *first = false;
        } else {
            spans.push(Span::raw(indent.to_string()));
        }
        spans.extend(seg_line.into_iter().map(|(t, s)| Span::styled(t, s)));
        lines.push(Line::from(spans));
    }
}

fn table_border_style() -> Style {
    Style::default().fg(FG_DIM)
}

/// Cell text alignment, read from the `:---`, `:--:`, `---:` markers in the
/// delimiter row.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Align {
    Left,
    Center,
    Right,
}

/// True when `lines[i]` begins a GFM table: a row containing `|` immediately
/// followed by a delimiter row (`| --- | :--: |` and friends).
fn is_table_start(lines: &[&str], i: usize) -> bool {
    if !lines[i].contains('|') {
        return false;
    }
    match lines.get(i + 1) {
        // The delimiter must carry a `|` so a `text | x` line above a `---`
        // thematic break isn't mistaken for a one-column table.
        Some(next) => next.contains('|') && is_delimiter_row(next),
        None => false,
    }
}

/// A GFM delimiter row: every cell is `:?-+:?` (optional surrounding spaces) and
/// contains at least one `-`.
fn is_delimiter_row(line: &str) -> bool {
    let cells = split_table_row(line);
    !cells.is_empty()
        && cells.iter().all(|c| {
            let t = c.trim();
            !t.is_empty() && t.contains('-') && t.chars().all(|ch| ch == '-' || ch == ':')
        })
}

/// Split one table row into trimmed cell strings, dropping the empty cells that
/// bracket a `|a|b|`-style row.
fn split_table_row(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(|c| c.trim().to_string()).collect()
}

/// Alignment encoded by a single delimiter cell.
fn parse_align(cell: &str) -> Align {
    let t = cell.trim();
    match (t.starts_with(':'), t.ends_with(':')) {
        (true, true) => Align::Center,
        (false, true) => Align::Right,
        _ => Align::Left,
    }
}

/// Pad or truncate a row's cells to exactly `ncols` entries.
fn fit_cols(cells: &[String], ncols: usize) -> Vec<String> {
    let mut v = cells.to_vec();
    v.truncate(ncols);
    v.resize(ncols, String::new());
    v
}

/// Longest natural cell width per column (capped), then shrunk proportionally so
/// the whole table — borders and padding included — fits within `avail` columns.
fn column_widths(grid: &[Vec<String>], ncols: usize, avail: usize) -> Vec<usize> {
    const MAX_COL: usize = 40;
    let mut nat = vec![1usize; ncols];
    for row in grid {
        for (i, cell) in row.iter().enumerate() {
            let w = cell.chars().count().clamp(1, MAX_COL);
            nat[i] = nat[i].max(w);
        }
    }
    // Overhead: `ncols + 1` vertical bars plus one padding space on each side.
    let overhead = (ncols + 1) + ncols * 2;
    let budget = avail.saturating_sub(overhead);
    let total: usize = nat.iter().sum();
    if total <= budget {
        return nat;
    }
    if budget < ncols {
        // Too narrow to shrink sensibly; give every column a single column.
        return vec![1; ncols];
    }
    // Distribute the budget proportionally to natural widths, min 1 each.
    let extra = budget - ncols;
    let shrinkable: usize = nat.iter().map(|w| w - 1).sum::<usize>().max(1);
    let mut out = vec![1usize; ncols];
    for i in 0..ncols {
        out[i] += (nat[i] - 1) * extra / shrinkable;
    }
    // Hand any rounding leftover to the widest columns first.
    let assigned: usize = out.iter().sum();
    let mut leftover = budget.saturating_sub(assigned);
    let mut idx: Vec<usize> = (0..ncols).collect();
    idx.sort_by_key(|&i| std::cmp::Reverse(nat[i]));
    let mut k = 0;
    while leftover > 0 {
        out[idx[k % ncols]] += 1;
        leftover -= 1;
        k += 1;
    }
    out
}

/// A horizontal border rule (`┌─┬─┐`, `├─┼─┤`, or `└─┴─┘`) spanning `widths`.
fn table_rule(widths: &[usize], left: &str, mid: &str, right: &str) -> Vec<Seg> {
    let mut s = String::from(left);
    for (i, w) in widths.iter().enumerate() {
        if i > 0 {
            s.push_str(mid);
        }
        s.push_str(&"─".repeat(w + 2)); // +2 for the padding spaces around a cell
    }
    s.push_str(right);
    vec![(s, table_border_style())]
}

/// Render a GFM table (`rows[0]` header, `rows[1]` delimiter, remaining rows the
/// body) into styled segment lines that fit within `avail` columns, using a full
/// box-drawing grid. Cell contents keep inline markdown and are wrapped to fit.
fn render_table(rows: &[&str], avail: usize) -> Vec<Vec<Seg>> {
    let header = split_table_row(rows[0]);
    let ncols = header.len().max(1);
    let delim = split_table_row(rows.get(1).copied().unwrap_or(""));
    let aligns: Vec<Align> = (0..ncols)
        .map(|i| delim.get(i).map(|c| parse_align(c)).unwrap_or(Align::Left))
        .collect();

    let mut grid: Vec<Vec<String>> = vec![fit_cols(&header, ncols)];
    for r in &rows[2.min(rows.len())..] {
        grid.push(fit_cols(&split_table_row(r), ncols));
    }

    let widths = column_widths(&grid, ncols, avail);
    let border = table_border_style();

    let mut out: Vec<Vec<Seg>> = Vec::new();
    out.push(table_rule(&widths, "┌", "┬", "┐"));
    for (ri, row) in grid.iter().enumerate() {
        let is_header = ri == 0;
        let base = if is_header {
            Style::default().fg(FG_TEXT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(FG_TEXT)
        };
        // Wrap each cell independently; the row is as tall as its tallest cell.
        let cells: Vec<Vec<Vec<Seg>>> = (0..ncols)
            .map(|ci| wrap_segments(&parse_inline(&row[ci], base), widths[ci]))
            .collect();
        let height = cells.iter().map(|c| c.len()).max().unwrap_or(1).max(1);

        for h in 0..height {
            let mut segs: Vec<Seg> = Vec::new();
            for ci in 0..ncols {
                segs.push(("│ ".to_string(), border));
                let content = cells[ci].get(h).cloned().unwrap_or_default();
                let cw: usize = content.iter().map(|(t, _)| t.chars().count()).sum();
                let pad = widths[ci].saturating_sub(cw);
                let (lp, rp) = match aligns[ci] {
                    Align::Left => (0, pad),
                    Align::Right => (pad, 0),
                    Align::Center => (pad / 2, pad - pad / 2),
                };
                if lp > 0 {
                    segs.push((" ".repeat(lp), base));
                }
                segs.extend(content);
                segs.push((" ".repeat(rp + 1), border)); // trailing pad + gap before bar
            }
            segs.push(("│".to_string(), border));
            out.push(segs);
        }

        if is_header {
            out.push(table_rule(&widths, "├", "┼", "┤"));
        }
    }
    out.push(table_rule(&widths, "└", "┴", "┘"));
    out
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
        " thinking… (Esc to interrupt) "
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

    let width = input_inner_width(area.width);
    let rows = wrap_input(&app.input, width);
    // Visible text rows (box height minus the two borders).
    let visible = area.height.saturating_sub(2) as usize;
    // Keep the last (cursor) row in view when the content is taller than the box.
    let scroll = rows.len().saturating_sub(visible);

    let lines: Vec<Line> = rows.iter().map(|r| Line::from(r.clone())).collect();
    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((scroll as u16, 0));
    f.render_widget(paragraph, area);

    if !app.busy {
        // Cursor sits just past the last character; project that column/row onto
        // the wrapped layout, accounting for any vertical scroll.
        let cols = 2 + app.input.chars().count();
        let cursor_row = cols / width;
        let cursor_col = cols % width;
        let x = area.x + 1 + cursor_col as u16;
        let y = area.y + 1 + (cursor_row.saturating_sub(scroll)) as u16;
        f.set_cursor_position((x, y));
    }
}

/// Hard-wrap `"> {input}"` into rows of at most `width` columns (character-based,
/// matching the single-column-per-char cursor model used elsewhere).
fn wrap_input(input: &str, width: usize) -> Vec<String> {
    let text: Vec<char> = format!("> {input}").chars().collect();
    if text.is_empty() {
        return vec![String::new()];
    }
    text.chunks(width).map(|c| c.iter().collect()).collect()
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let kiwix_state = if app.kiwix_reachable {
        Span::styled("kiwix✓", Style::default().fg(ACCENT_AI))
    } else {
        Span::styled("kiwix✗", Style::default().fg(ACCENT_ERR))
    };
    let status = if app.busy { "busy" } else { "ready" };
    let ctx = match app.context_tokens {
        Some(n) => format!("ctx:{}", format_tokens(n)),
        None => "ctx:—".to_string(),
    };
    let line = Line::from(vec![
        Span::styled(
            format!(" {} ", app.llm.model()),
            Style::default().fg(Color::Black).bg(ACCENT_USER),
        ),
        Span::raw(" "),
        kiwix_state,
        Span::raw(format!("  lang:{}  ", app.lang)),
        Span::styled(status, Style::default().add_modifier(Modifier::DIM)),
        Span::raw(format!("  {ctx}")),
        Span::raw("  PgUp/PgDn scroll · Tab thinking · /quit"),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

/// Format a token count compactly: raw below 1000, else one decimal + `k`.
fn format_tokens(n: u32) -> String {
    if n < 1000 {
        n.to_string()
    } else {
        format!("{:.1}k", n as f64 / 1000.0)
    }
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

    #[test]
    fn detects_table_header_and_delimiter() {
        let lines = vec!["| A | B |", "| --- | :--: |", "| 1 | 2 |"];
        assert!(is_table_start(&lines, 0));
        // A pipe line with no delimiter row below is not a table.
        let prose = vec!["value | other", "more prose here"];
        assert!(!is_table_start(&prose, 0));
        // A `---` thematic break after a pipe line must not trigger a table.
        let thematic = vec!["a | b", "---"];
        assert!(!is_table_start(&thematic, 0));
    }

    #[test]
    fn parses_cells_and_alignment() {
        assert_eq!(split_table_row("| A | B | C |"), vec!["A", "B", "C"]);
        assert_eq!(split_table_row("A | B"), vec!["A", "B"]);
        assert_eq!(parse_align(":---"), Align::Left);
        assert_eq!(parse_align(":--:"), Align::Center);
        assert_eq!(parse_align("---:"), Align::Right);
        assert!(is_delimiter_row("| --- | :---: |"));
        assert!(!is_delimiter_row("| not | a delim |"));
    }

    /// Visible column count of a rendered segment line.
    fn line_width(segs: &[Seg]) -> usize {
        segs.iter().map(|(t, _)| t.chars().count()).sum()
    }

    #[test]
    fn table_fits_within_available_width() {
        let rows = vec![
            "| Name | Description |",
            "| --- | --- |",
            "| alpha | a rather long description that will not fit a narrow pane |",
            "| beta | short |",
        ];
        let avail = 30;
        let out = render_table(&rows, avail);
        assert!(!out.is_empty());
        for segs in &out {
            assert!(
                line_width(segs) <= avail,
                "line exceeds width: {}",
                line_width(segs)
            );
        }
        // Border rows top and bottom.
        assert!(out[0].iter().any(|(t, _)| t.starts_with('┌')));
        assert!(out.last().unwrap().iter().any(|(t, _)| t.starts_with('└')));
    }

    #[test]
    fn table_renders_without_panicking_when_very_narrow() {
        let rows = vec!["| A | B | C |", "| --- | --- | --- |", "| 1 | 2 | 3 |"];
        // Narrower than the per-column overhead — must still produce output.
        let out = render_table(&rows, 4);
        assert!(!out.is_empty());
    }
}
