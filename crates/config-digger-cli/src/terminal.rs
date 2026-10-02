//! Terminal conventions: when to colour, how wide a line may be, and fitting
//! styled text to that width without counting escape codes.
//!
//! Output is always UTF-8 without touching the console code page: the standard
//! library writes UTF-16 to a Windows console and UTF-8 bytes to pipes and files.

use anstyle::Style;
use clap::ValueEnum;
use std::ffi::OsString;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The `--color` flag.
#[derive(Clone, Copy, Default, ValueEnum)]
pub enum ColorWhen {
    /// Colour a terminal unless `NO_COLOR` is set.
    #[default]
    Auto,
    Always,
    Never,
}

impl ColorWhen {
    /// Makes the choice global for `anstream`, which strips styles from any
    /// stream that should not get them and honours `NO_COLOR` in `auto`.
    pub fn apply(self) {
        let choice = match self {
            ColorWhen::Auto => anstream::ColorChoice::Auto,
            ColorWhen::Always => anstream::ColorChoice::Always,
            ColorWhen::Never => anstream::ColorChoice::Never,
        };
        choice.write_global();
    }

    pub fn for_clap(self) -> clap::ColorChoice {
        match self {
            ColorWhen::Auto => clap::ColorChoice::Auto,
            ColorWhen::Always => clap::ColorChoice::Always,
            ColorWhen::Never => clap::ColorChoice::Never,
        }
    }

    /// The `--color` value in raw arguments, read before parsing so that help
    /// and parse errors follow it too. The last one wins, as it does for clap.
    pub fn requested(args: &[OsString]) -> Self {
        let mut requested = ColorWhen::default();
        let mut args = args.iter().skip(1).map(|arg| arg.to_str());
        while let Some(arg) = args.next() {
            let value = match arg {
                Some("--") => break,
                Some("--color") => args.next().flatten(),
                Some(arg) => arg.strip_prefix("--color="),
                None => None,
            };
            if let Some(value) = value.and_then(|value| ColorWhen::from_str(value, false).ok()) {
                requested = value;
            }
        }
        requested
    }
}

/// Whether stderr gets colour, for writers that do not go through `anstream`.
pub fn stderr_is_colored() -> bool {
    anstream::AutoStream::choice(&std::io::stderr()) != anstream::ColorChoice::Never
}

/// The width `list` fits its lines to: `COLUMNS` when set, as most terminal
/// tools honour it, otherwise the width of the terminal stdout writes to.
/// `None` means no limit: `--wide`, or stdout is a pipe or a file.
pub fn output_width(wide: bool) -> Option<usize> {
    if wide {
        return None;
    }
    let columns = std::env::var("COLUMNS")
        .ok()
        .and_then(|columns| columns.trim().parse::<usize>().ok())
        .filter(|&columns| columns > 0);
    columns.or_else(|| {
        terminal_size::terminal_size_of(std::io::stdout())
            .map(|(terminal_size::Width(width), _)| usize::from(width))
    })
}

const ELLIPSIS: char = '…';

/// One output line made of styled pieces.
#[derive(Default)]
pub struct Line(Vec<(String, Style)>);

impl Line {
    pub fn push(&mut self, text: impl Into<String>, style: Style) -> &mut Self {
        self.0.push((text.into(), style));
        self
    }

    /// Adds `text` padded with spaces to `width` columns.
    pub fn push_padded(&mut self, text: &str, width: usize, style: Style) -> &mut Self {
        self.push(text, style);
        let padding = width.saturating_sub(text.width());
        self.push(" ".repeat(padding), Style::new())
    }

    pub fn width(&self) -> usize {
        self.0.iter().map(|(text, _)| text.width()).sum()
    }

    /// Cuts the end of the line so that it fits `width` columns, marking the cut with `…`.
    pub fn fit(mut self, width: Option<usize>) -> Self {
        let Some(width) = width else { return self };
        if self.width() <= width {
            return self;
        }
        let mut room = width.saturating_sub(1);
        let mut kept = Vec::new();
        for (text, style) in self.0.drain(..) {
            let piece = take_columns(text.chars(), room);
            room -= piece.width();
            let whole = piece.len() == text.len();
            kept.push((piece, style));
            if !whole {
                break;
            }
        }
        if let Some((last, _)) = kept.last_mut() {
            last.truncate(last.trim_end().len());
        }
        if width > 0 {
            kept.push((ELLIPSIS.to_string(), Style::new()));
        }
        Line(kept)
    }

    /// The line with its styles as escape codes, which `anstream` strips when colour is off.
    pub fn render(&self) -> String {
        let mut out: String = self
            .0
            .iter()
            .map(|(text, style)| format!("{style}{text}{style:#}"))
            .collect();
        out.truncate(out.trim_end().len());
        out.push('\n');
        out
    }
}

/// Shortens `text` to `width` columns by replacing its middle with `…`,
/// keeping more of the end, where a path is most specific.
pub fn elide_middle(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let Some(room) = width.checked_sub(1) else {
        return String::new();
    };
    let head = take_columns(text.chars(), room / 3);
    let tail: String = take_columns(text.chars().rev(), room - head.width())
        .chars()
        .rev()
        .collect();
    format!("{head}{ELLIPSIS}{tail}")
}

/// The longest run of `chars` that fits in `columns`.
fn take_columns(chars: impl Iterator<Item = char>, columns: usize) -> String {
    let mut used = 0;
    chars
        .take_while(|c| {
            used += c.width().unwrap_or(0);
            used <= columns
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(line: &Line) -> String {
        line.0.iter().map(|(text, _)| text.as_str()).collect()
    }

    #[test]
    fn elide_middle_keeps_text_that_fits() {
        assert_eq!(elide_middle("C:\\a\\b.json", 11), "C:\\a\\b.json");
    }

    #[test]
    fn elide_middle_keeps_more_of_the_end() {
        assert_eq!(elide_middle("abcdefghijklmnop", 10), "abc…klmnop");
    }

    #[test]
    fn elide_middle_counts_wide_characters_as_two_columns() {
        let elided = elide_middle("日本語のフォルダ\\x.json", 12);

        assert!(elided.width() <= 12, "{elided}");
        assert!(elided.ends_with("\\x.json"), "{elided}");
    }

    #[test]
    fn elide_middle_to_nothing() {
        assert_eq!(elide_middle("abc", 0), "");
    }

    #[test]
    fn fit_cuts_across_styled_pieces() {
        let mut line = Line::default();
        line.push("name  ", Style::new())
            .push("Effective", Style::new().bold())
            .push("  C:\\x", Style::new());

        let fitted = line.fit(Some(10));

        assert_eq!(plain(&fitted), "name  Eff…");
        assert_eq!(fitted.width(), 10);
    }

    #[test]
    fn fit_drops_trailing_spaces_before_the_cut() {
        let mut line = Line::default();
        line.push("name      ", Style::new())
            .push("Effective", Style::new());

        assert_eq!(plain(&line.fit(Some(8))), "name…");
    }

    #[test]
    fn fit_without_a_width_keeps_the_line() {
        let mut line = Line::default();
        line.push("a long line", Style::new());

        assert_eq!(plain(&line.fit(None)), "a long line");
    }

    #[test]
    fn requested_color_reads_both_spellings_and_stops_at_double_dash() {
        let args = |list: &[&str]| -> Vec<OsString> {
            std::iter::once("config-digger")
                .chain(list.iter().copied())
                .map(OsString::from)
                .collect()
        };

        assert!(matches!(
            ColorWhen::requested(&args(&["--color", "never"])),
            ColorWhen::Never
        ));
        assert!(matches!(
            ColorWhen::requested(&args(&["list", "--color=always"])),
            ColorWhen::Always
        ));
        assert!(matches!(
            ColorWhen::requested(&args(&["--", "--color", "never"])),
            ColorWhen::Auto
        ));
    }
}
