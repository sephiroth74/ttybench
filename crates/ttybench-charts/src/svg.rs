//! The charts' common parts: their paints in the light and the dark
//! appearance, a chart's frame of its title, subtitle and description for
//! screen readers, the axes' round ticks, and numbers and texts.

use std::fmt::Write;

/// A chart's width, in points.
pub const WIDTH: f64 = 680.0;

/// Where a chart's content starts under a subtitle of one line.
pub const TOP: f64 = 62.0;

/// A subtitle's size and line.
const SUBTITLE_SIZE: f64 = 12.0;
const SUBTITLE_LINE: f64 = 16.0;

/// The inks and the chart's own paints, each class's in the light and the
/// dark appearance.
const PAINTS: &[(&str, &str, &str, &str)] = &[
    ("surface", "fill", "#fcfcfb", "#1a1a19"),
    ("primary", "fill", "#0b0b0b", "#f0efec"),
    ("secondary", "fill", "#52514e", "#c3c2b7"),
    ("muted", "fill", "#898781", "#898781"),
    ("grid", "stroke", "#e1e0d9", "#2c2c2a"),
    ("base", "stroke", "#c3c2b7", "#383835"),
    ("budget", "stroke", "#52514e", "#c3c2b7"),
    ("context", "fill", "#898781", "#6f6e69"),
    ("context-line", "stroke", "#898781", "#6f6e69"),
    ("ring", "stroke", "#fcfcfb", "#1a1a19"),
    ("halo", "stroke", "#fcfcfb", "#1a1a19"),
    ("better", "fill", "#0ca30c", "#0ca30c"),
    ("better-line", "stroke", "#0ca30c", "#0ca30c"),
    ("worse", "fill", "#d03b3b", "#d03b3b"),
    ("worse-line", "stroke", "#d03b3b", "#d03b3b"),
    ("plain", "fill", "#52514e", "#c3c2b7"),
    ("plain-line", "stroke", "#52514e", "#c3c2b7"),
];

/// The terminals' colors, in the session's order of its terminals, each
/// in the light and the dark appearance: a categorical palette whose
/// neighbors stay apart for the color blind too.
pub const SERIES: [(&str, &str); 8] = [
    ("#2a78d6", "#3987e5"),
    ("#eb6834", "#d95926"),
    ("#1baf7a", "#199e70"),
    ("#eda100", "#c98500"),
    ("#e87ba4", "#d55181"),
    ("#008300", "#0ca30c"),
    ("#6250d6", "#9085e9"),
    ("#e34948", "#e66767"),
];

/// The class of the terminal at `index` of the session's, its fill; and of
/// its line, `-line` after it. A terminal past the palette's goes gray.
pub fn series(index: usize) -> String {
    if index < SERIES.len() {
        format!("t{index}")
    } else {
        "context".to_owned()
    }
}

/// The rules of each class under `scope`, of the dark appearance or not.
fn rules(out: &mut String, scope: &str, dark: bool, indent: &str) {
    for (name, kind, light, darker) in PAINTS {
        let _ = writeln!(
            out,
            "{indent}{scope} .{name} {{ {kind}: {}; }}",
            if dark { darker } else { light }
        );
    }
    for (index, (light, darker)) in SERIES.iter().enumerate() {
        let color = if dark { darker } else { light };
        let _ = writeln!(out, "{indent}{scope} .t{index} {{ fill: {color}; }}");
        let _ = writeln!(out, "{indent}{scope} .t{index}-line {{ stroke: {color}; }}");
    }
}

/// The charts' style: the light appearance's paints, the dark one's as the
/// reader's appearance asks for it, unless a page names the chart `light`,
/// or as a page names it `dark`.
fn style() -> String {
    let mut out = String::from("<style>\n");
    rules(&mut out, ".chart", false, "  ");
    out.push_str(
        "  .chart text { font-family: system-ui, -apple-system, \"Segoe UI\", sans-serif; }\n",
    );
    out.push_str("  .chart .tick { font-variant-numeric: tabular-nums; }\n");
    out.push_str("  .chart .hit { fill: transparent; }\n");
    out.push_str(
        "  .chart .halo { paint-order: stroke; stroke-width: 4px; stroke-linejoin: round; }\n",
    );
    out.push_str("  @media (prefers-color-scheme: dark) {\n");
    rules(&mut out, ".chart:not(.light)", true, "    ");
    out.push_str("  }\n");
    rules(&mut out, ".chart.dark", true, "  ");
    out.push_str("</style>");
    out
}

/// A chart being drawn: its frame's texts and its body's marks.
pub struct Chart {
    /// Its id, unique among a session's charts, which its title's and
    /// description's ids take, so that a page holds many.
    pub id: String,
    /// Its title.
    pub title: String,
    /// Its subtitle.
    pub subtitle: String,
    /// Its description for screen readers: what it shows, in words.
    pub description: String,
    /// Its marks.
    pub body: String,
}

impl Chart {
    /// A chart of `id`, `title`, `subtitle` and `description`, of no marks
    /// yet.
    pub fn new(id: &str, title: &str, subtitle: &str, description: &str) -> Self {
        Self {
            id: id.to_owned(),
            title: title.to_owned(),
            subtitle: subtitle.to_owned(),
            description: description.to_owned(),
            body: String::new(),
        }
    }

    /// Where its content starts, under its title and subtitle.
    pub fn top(&self) -> f64 {
        TOP + SUBTITLE_LINE * (subtitle_lines(&self.subtitle).len() as f64 - 1.0)
    }

    /// Adds `mark` to its body.
    pub fn push(&mut self, mark: &str) {
        self.body.push_str(mark);
        self.body.push('\n');
    }

    /// The chart's SVG, of `height` points.
    pub fn finish(&self, height: f64) -> String {
        let id = escape(&self.id);
        let mut out = String::new();
        let _ = writeln!(
            out,
            r#"<svg xmlns="http://www.w3.org/2000/svg" class="chart" width="{WIDTH}" height="{height:.0}" viewBox="0 0 {WIDTH} {height:.0}" role="img" aria-labelledby="{id}-title {id}-desc">"#
        );
        let _ = writeln!(
            out,
            r#"<title id="{id}-title">{}</title>"#,
            escape(&self.title)
        );
        let _ = writeln!(
            out,
            r#"<desc id="{id}-desc">{}</desc>"#,
            escape(&self.description)
        );
        out.push_str(&style());
        out.push('\n');
        let _ = writeln!(
            out,
            r#"<rect class="surface" width="{WIDTH}" height="{height:.0}" rx="8"/>"#
        );
        let _ = writeln!(
            out,
            r#"<text class="primary" x="16" y="26" font-size="15" font-weight="600">{}</text>"#,
            escape(&self.title)
        );
        for (index, line) in subtitle_lines(&self.subtitle).iter().enumerate() {
            let _ = writeln!(
                out,
                r#"<text class="secondary" x="16" y="{:.0}" font-size="{SUBTITLE_SIZE}">{}</text>"#,
                45.0 + SUBTITLE_LINE * index as f64,
                escape(line)
            );
        }
        out.push_str(&self.body);
        out.push_str("</svg>\n");
        out
    }
}

/// `text` as SVG's text and attributes take it.
pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// About the width of `text` at `size` points in the system's sans serif,
/// of its characters' kinds: no font's metrics, but wide enough that a
/// line of it fits where this says it does.
pub fn text_width(text: &str, size: f64) -> f64 {
    let width: f64 = text
        .chars()
        .map(|c| match c {
            'i' | 'l' | 'j' | 'I' | '.' | ',' | ':' | ';' | '\'' | '|' | '!' | '(' | ')' | '['
            | ']' | ' ' => 0.29,
            'f' | 'r' | 't' => 0.36,
            'm' | 'w' | 'M' | 'W' => 0.82,
            c if c.is_uppercase() || "×%&@".contains(c) => 0.66,
            c if c.is_ascii_digit() => 0.57,
            _ => 0.53,
        })
        .sum();
    width * size
}

/// `subtitle` in lines that each fit the chart's width, broken at its
/// spaces, of a margin for other systems' wider fonts.
fn subtitle_lines(subtitle: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in subtitle.split(' ') {
        let wider = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && text_width(&wider, SUBTITLE_SIZE) * 1.1 > WIDTH - 40.0 {
            lines.push(std::mem::take(&mut line));
            line = word.to_owned();
        } else {
            line = wider;
        }
    }
    lines.push(line);
    lines
}

/// Round ticks from 0 past `top`, about `count` of them.
pub fn ticks(top: f64, count: usize) -> Vec<f64> {
    if !top.is_finite() || top <= 0.0 {
        return vec![0.0, 1.0];
    }
    let raw = top / count as f64;
    let power = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .into_iter()
        .map(|step| step * power)
        .find(|step| *step >= raw)
        .unwrap_or(10.0 * power);
    let mut ticks = vec![0.0];
    while *ticks.last().unwrap_or(&0.0) < top {
        let next = (ticks.len() as f64 * step * 1e9).round() / 1e9;
        ticks.push(next);
    }
    ticks
}

/// A tick's text: in full under 10,000, as `12K` past it.
pub fn tick_text(value: f64) -> String {
    let magnitude = value.abs();
    if magnitude >= 10_000.0 {
        format!("{:.0}K", value / 1000.0)
    } else if magnitude >= 1000.0 {
        let text = format!("{:.1}K", value / 1000.0);
        text.replace(".0K", "K")
    } else {
        let text = format!("{value:.6}");
        let text = text.trim_end_matches('0').trim_end_matches('.');
        if text == "-0" {
            "0".to_owned()
        } else {
            text.to_owned()
        }
    }
}

/// How many decimals a metric's values take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Digits {
    /// Whole numbers, as counts: a number between two of a decimal.
    Whole,
    /// This many decimals.
    Fixed(usize),
}

impl Digits {
    /// Those of `samples`, each terminal's of its median `medians`: whole of
    /// whole samples, else of the greatest median's magnitude, four
    /// figures or so.
    pub fn of(samples: &[f64], medians: &[f64]) -> Self {
        if !samples.is_empty() && samples.iter().all(|sample| sample.fract() == 0.0) {
            return Self::Whole;
        }
        let greatest = medians.iter().map(|m| m.abs()).fold(0.0, f64::max);
        Self::Fixed(if greatest >= 100.0 {
            1
        } else if greatest >= 1.0 {
            2
        } else if greatest >= 0.1 {
            3
        } else {
            4
        })
    }

    /// `value` of these decimals.
    pub fn format(self, value: f64) -> String {
        match self {
            Self::Whole if value.fract() == 0.0 => format!("{value:.0}"),
            Self::Whole => format!("{value:.1}"),
            Self::Fixed(digits) => format!("{value:.digits$}"),
        }
    }
}

/// A horizontal bar from `x` at the baseline, `length` long and `thick`,
/// at `y`, rounded at its data end alone.
pub fn bar_path(x: f64, y: f64, length: f64, thick: f64) -> String {
    let radius = 4f64.min(length / 2.0).min(thick / 2.0);
    let straight = (length - radius).max(0.0);
    format!(
        "M{x:.1},{y:.1} h{straight:.1} a{radius:.1},{radius:.1} 0 0 1 {radius:.1},{radius:.1} v{:.1} a{radius:.1},{radius:.1} 0 0 1 {:.1},{radius:.1} h{:.1} z",
        thick - 2.0 * radius,
        -radius,
        -straight
    )
}

/// A file's name of `parts`: their letters and digits, lowercase, the rest
/// `-`, of no `-` twice nor at an end.
pub fn slug(parts: &[&str]) -> String {
    let mut slug = String::new();
    for c in parts.join("-").chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    slug.trim_end_matches('-').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_ticks_past_the_top() {
        assert_eq!(ticks(430.6, 5), [0.0, 100.0, 200.0, 300.0, 400.0, 500.0]);
        assert_eq!(ticks(0.33, 4), [0.0, 0.1, 0.2, 0.3, 0.4]);
        assert_eq!(ticks(9.0, 4), [0.0, 2.5, 5.0, 7.5, 10.0]);
        assert_eq!(ticks(0.0, 5), [0.0, 1.0]);
        assert_eq!(tick_text(0.1), "0.1");
        assert_eq!(tick_text(2.5), "2.5");
        assert_eq!(tick_text(1500.0), "1.5K");
        assert_eq!(tick_text(20_000.0), "20K");
        assert_eq!(tick_text(-0.0), "0");
    }

    #[test]
    fn names_and_numbers() {
        assert_eq!(
            slug(&["throughput", "written_mib_s", "text plain"]),
            "throughput-written-mib-s-text-plain"
        );
        assert_eq!(Digits::of(&[1.0, 2.0], &[1.5]), Digits::Whole);
        assert_eq!(Digits::of(&[1.5], &[268.1]), Digits::Fixed(1));
        assert_eq!(Digits::Fixed(3).format(0.2961), "0.296");
        assert_eq!(escape("a<b & \"c\""), "a&lt;b &amp; &quot;c&quot;");
        assert_eq!(series(1), "t1");
        assert_eq!(series(8), "context");
        assert!(subtitle_lines(&"word ".repeat(60)).len() > 1);
    }
}
