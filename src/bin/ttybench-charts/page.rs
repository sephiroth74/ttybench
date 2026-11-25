//! A session's page, `index.html`: static, of no script nor network, of
//! its report's Markdown as HTML, each chart inline after the table of
//! its group, the A/B's after the A/B's table; and the history's page of
//! its charts. Each follows the reader's light or dark appearance.

use std::fmt::Write;

use crate::kinds::Drawn;
use crate::svg::escape;

/// The pages' style.
const STYLE: &str = r#"<style>
  :root { color-scheme: light dark; }
  body { margin: 0 auto; max-width: 760px; padding: 24px 20px 48px; background: #f9f9f7; color: #0b0b0b;
         font: 15px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif; }
  h1 { font-size: 22px; margin: 8px 0 12px; }
  h2 { font-size: 18px; margin: 36px 0 8px; }
  h3 { font-size: 15px; margin: 24px 0 6px; }
  p, li { color: #52514e; }
  table { border-collapse: collapse; margin: 8px 0 16px; font-size: 13px; font-variant-numeric: tabular-nums;
          display: block; overflow-x: auto; max-width: 100%; }
  th, td { padding: 4px 10px 4px 0; border-bottom: 1px solid #e1e0d9; text-align: left; vertical-align: top; }
  th { font-weight: 600; color: #52514e; }
  code { font: 12px ui-monospace, SFMono-Regular, Menlo, monospace; }
  figure { margin: 12px 0 20px; }
  figure svg { display: block; max-width: 100%; height: auto; }
  @media (prefers-color-scheme: dark) {
    body { background: #0d0d0d; color: #f0efec; }
    p, li, th { color: #c3c2b7; }
    th, td { border-color: #2c2c2a; }
  }
</style>"#;

/// The page's start, of `title`.
fn head(title: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{}</title>\n{STYLE}\n</head>\n<body>\n",
        escape(title)
    )
}

/// The session's page of its report's Markdown, `markdown`, and its
/// charts, `drawn`.
pub fn session(title: &str, markdown: &str, drawn: &[Drawn]) -> String {
    let mut out = head(title);
    // The group whose section the lines are in: the `###` headings count
    // the report's groups in their order.
    let mut group: Option<usize> = None;
    let mut groups_seen = 0;
    let mut in_ab = false;
    let flush = |out: &mut String, group: Option<usize>, in_ab: bool| {
        for chart in drawn {
            let ours = if chart.kind == "ab" {
                in_ab
            } else {
                !in_ab && group.is_some() && chart.groups.first().copied() == group
            };
            if ours {
                let _ = writeln!(out, "<figure>\n{}</figure>", chart.svg);
            }
        }
    };
    let lines: Vec<&str> = markdown.lines().collect();
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        if let Some(text) = line.strip_prefix("### ") {
            flush(&mut out, group, in_ab);
            in_ab = false;
            group = Some(groups_seen);
            groups_seen += 1;
            let _ = writeln!(out, "<h3>{}</h3>", inline(text));
        } else if let Some(text) = line.strip_prefix("## ") {
            flush(&mut out, group, in_ab);
            group = None;
            in_ab = text.starts_with("A/B");
            let _ = writeln!(out, "<h2>{}</h2>", inline(text));
        } else if let Some(text) = line.strip_prefix("# ") {
            let _ = writeln!(out, "<h1>{}</h1>", inline(text));
        } else if line.starts_with('|') {
            let mut rows = Vec::new();
            while at < lines.len() && lines[at].starts_with('|') {
                rows.push(cells(lines[at]));
                at += 1;
            }
            table(&mut out, &rows);
            continue;
        } else if line.starts_with("- ") {
            out.push_str("<ul>\n");
            while at < lines.len() && lines[at].starts_with("- ") {
                let _ = writeln!(out, "<li>{}</li>", inline(&lines[at][2..]));
                at += 1;
            }
            out.push_str("</ul>\n");
            continue;
        } else if !line.trim().is_empty() {
            let mut text = line.to_owned();
            while at + 1 < lines.len() && is_prose(lines[at + 1]) {
                at += 1;
                text.push(' ');
                text.push_str(lines[at]);
            }
            let _ = writeln!(out, "<p>{}</p>", inline(&text));
        }
        at += 1;
    }
    flush(&mut out, group, in_ab);
    out.push_str("</body>\n</html>\n");
    out
}

/// The history's page of its charts, `drawn`, of `title`.
pub fn history(title: &str, drawn: &[Drawn]) -> String {
    let mut out = head(title);
    let _ = writeln!(out, "<h1>{}</h1>", escape(title));
    for chart in drawn {
        let _ = writeln!(out, "<figure>\n{}</figure>", chart.svg);
    }
    out.push_str("</body>\n</html>\n");
    out
}

/// Whether `line` goes on the paragraph before it.
fn is_prose(line: &str) -> bool {
    !line.trim().is_empty() && !line.starts_with(['#', '|']) && !line.starts_with("- ")
}

/// A table of `rows`, the first its header, the second its rule.
fn table(out: &mut String, rows: &[Vec<String>]) {
    out.push_str("<table>\n");
    for (index, row) in rows.iter().enumerate() {
        if index == 1
            && row
                .iter()
                .all(|cell| cell.chars().all(|c| matches!(c, '-' | ':')))
        {
            continue;
        }
        let tag = if index == 0 { "th" } else { "td" };
        out.push_str("<tr>");
        for cell in row {
            let _ = write!(out, "<{tag}>{}</{tag}>", inline(cell));
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</table>\n");
}

/// The cells of a table's row, `| a | b |`, split at its pipes that no
/// backslash escapes, the escapes kept for `inline`.
fn cells(line: &str) -> Vec<String> {
    let inner = line.trim().trim_start_matches('|');
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                cell.push(c);
                if let Some(next) = chars.next() {
                    cell.push(next);
                }
            }
            '|' => cells.push(std::mem::take(&mut cell).trim().to_owned()),
            _ => cell.push(c),
        }
    }
    cells.push(cell.trim().to_owned());
    cells
}

/// `text` of Markdown's spans of code and strong text, and its escaped
/// characters, as HTML, the rest escaped.
fn inline(text: &str) -> String {
    let mut out = String::new();
    let mut strong = false;
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if c == '\\'
            && let Some(next) = rest[1..].chars().next()
        {
            out.push_str(&escape(&next.to_string()));
            rest = &rest[1 + next.len_utf8()..];
        } else if c == '`'
            && let Some(end) = rest[1..].find('`')
        {
            let _ = write!(out, "<code>{}</code>", escape(&rest[1..=end]));
            rest = &rest[end + 2..];
        } else if rest.starts_with("**") {
            out.push_str(if strong { "</strong>" } else { "<strong>" });
            strong = !strong;
            rest = &rest[2..];
        } else {
            out.push_str(&escape(&c.to_string()));
            rest = &rest[c.len_utf8()..];
        }
    }
    if strong {
        out.push_str("</strong>");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chart(file: &str, kind: &'static str, groups: Vec<usize>) -> Drawn {
        Drawn {
            file: file.to_owned(),
            title: file.to_owned(),
            kind,
            groups,
            svg: format!("<svg data-file=\"{file}\"></svg>\n"),
        }
    }

    #[test]
    fn a_reports_markdown_with_each_chart_after_its_table() {
        let markdown = "# ttybench session s\n\nMeasured on Mac16,8.\n\n## A/B: b against a\n\nEach metric.\n\n| Metric | A |\n|---|---|\n| x `y` | **worse** |\n\n## startup\n\n### Ready (ms)\n\n`ready_ms`. Conditions: text plain\\|pipe.\n\n| Terminal | Median |\n|---|---|\n| a | 1.0 |\n\n### Window (ms)\n\n| Terminal | Median |\n|---|---|\n| a <b> | 2.0 |\n\n## Runs that measured nothing\n\n- `c1`, failed: it exited\n";
        let drawn = [
            chart("ab.svg", "ab", vec![0, 1]),
            chart("ready.svg", "bars", vec![0]),
            chart("window.svg", "bars", vec![1]),
        ];
        let page = session("s", markdown, &drawn);
        let order: Vec<usize> = [
            "<h2>A/B: b against a</h2>",
            "<td>x <code>y</code></td><td><strong>worse</strong></td>",
            "data-file=\"ab.svg\"",
            "<h3>Ready (ms)</h3>",
            "<p><code>ready_ms</code>. Conditions: text plain|pipe.</p>",
            "<td>a</td><td>1.0</td>",
            "data-file=\"ready.svg\"",
            "<h3>Window (ms)</h3>",
            "<td>a &lt;b&gt;</td>",
            "data-file=\"window.svg\"",
            "<li><code>c1</code>, failed: it exited</li>",
        ]
        .iter()
        .map(|part| {
            page.find(part)
                .unwrap_or_else(|| panic!("{part} not in:\n{page}"))
        })
        .collect();
        assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");
        assert_eq!(page.matches("data-file=\"ab.svg\"").count(), 1);
        assert!(!page.contains("<script") && !page.contains("http"));
        assert!(page.contains("<th>Terminal</th><th>Median</th>"));
        assert!(!page.contains("<td>---</td>"));
    }
}
