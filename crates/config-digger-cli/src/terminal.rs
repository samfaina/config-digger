//! Terminal conventions: when to colour, how wide a line may be, and fitting
//! styled text to that width without counting escape codes.
//!
//! Output is always UTF-8 without touching the console code page: the standard
//! library writes UTF-16 to a Windows console and UTF-8 bytes to pipes and files.
//! A shell that decodes captured output with another encoding, as PowerShell
//! does with `[Console]::OutputEncoding`, picks it before the process starts,
//! so changing the code page from here would not help it.

use anstyle::Style;
use clap::ValueEnum;
use std::ffi::OsString;
use unicode_width::UnicodeWidthChar;

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

    /// The same choice for clap's help and parse errors.
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

/// The width `list` fits its lines to. `COLUMNS` overrides it wherever stdout
/// goes, as it does for Python's `shutil.get_terminal_size`; otherwise it is
/// the width of the terminal stdout writes to. `None` means no limit: `--wide`,
/// or stdout is a pipe or a file and `COLUMNS` is not set.
pub fn output_width(wide: bool) -> Option<usize> {
    if wide {
        return None;
    }
    let override_width = std::env::var("COLUMNS")
        .ok()
        .and_then(|columns| columns.trim().parse::<usize>().ok())
        .filter(|&columns| columns > 0);
    override_width.or_else(|| {
        terminal_size::terminal_size_of(std::io::stdout())
            .map(|(terminal_size::Width(width), _)| usize::from(width))
    })
}

const ELLIPSIS: char = '…';

/// One output line made of styled pieces.
#[derive(Default)]
pub struct Line(Vec<(String, Style)>);

impl Line {
    /// Adds `text` in `style`.
    pub fn push(&mut self, text: impl Into<String>, style: Style) -> &mut Self {
        self.0.push((text.into(), style));
        self
    }

    /// Adds `text` padded with spaces to `width` columns.
    pub fn push_padded(&mut self, text: &str, width: usize, style: Style) -> &mut Self {
        self.push(text, style);
        let padding = width.saturating_sub(columns(text));
        self.push(" ".repeat(padding), Style::new())
    }

    /// How many columns the line takes, escape codes excluded.
    pub fn width(&self) -> usize {
        self.0.iter().map(|(text, _)| columns(text)).sum()
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
            room -= columns(&piece);
            let whole = piece.len() == text.len();
            kept.push((piece, style));
            if !whole {
                break;
            }
        }
        // Padding before the cut would leave a gap between the text and the `…`.
        while let Some((last, _)) = kept.last_mut() {
            last.truncate(last.trim_end().len());
            if !last.is_empty() {
                break;
            }
            kept.pop();
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

/// How many columns `text` takes, counted character by character so that
/// measuring and cutting always agree.
pub fn columns(text: &str) -> usize {
    text.chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// Shortens `text` to `width` columns by replacing its middle with `…`,
/// keeping more of the end, where a path is most specific.
pub fn elide_middle(text: &str, width: usize) -> String {
    if columns(text) <= width {
        return text.to_owned();
    }
    let Some(room) = width.checked_sub(1) else {
        return String::new();
    };
    let head = take_columns(text.chars(), room / 3);
    let tail: String = take_columns(text.chars().rev(), room - columns(&head))
        .chars()
        .rev()
        .collect();
    format!("{head}{ELLIPSIS}{tail}")
}

/// Splits `text` into lines of at most `width` columns, breaking between
/// words. A word wider than `width` gets a line of its own, to be cut later.
/// Without a width the text stays one line.
pub fn wrap(text: &str, width: Option<usize>) -> Vec<String> {
    let Some(width) = width else {
        return vec![text.to_owned()];
    };
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if columns(line) + 1 + columns(word) <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_owned()),
        }
    }
    lines
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

        assert!(columns(&elided) <= 12, "{elided}");
        assert!(elided.ends_with("\\x.json"), "{elided}");
    }

    #[test]
    fn wrap_breaks_between_words() {
        assert_eq!(
            wrap("Shadowed by the user Entry", Some(12)),
            ["Shadowed by", "the user", "Entry"]
        );
    }

    #[test]
    fn wrap_gives_a_long_word_its_own_line() {
        assert_eq!(
            wrap(r"by C:\a\very\long\path.json#x here", Some(10)),
            ["by", r"C:\a\very\long\path.json#x", "here"]
        );
    }

    #[test]
    fn wrap_without_a_width_keeps_one_line() {
        assert_eq!(wrap("a b  c", None), ["a b  c"]);
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
    fn fit_drops_trailing_padding_pieces_before_the_cut() {
        let mut line = Line::default();
        line.push_padded("name", 6, Style::new())
            .push("Effective", Style::new());

        assert_eq!(plain(&line.fit(Some(7))), "name…");
    }

    #[test]
    fn fit_measures_emoji_presentation_sequences_like_it_cuts_them() {
        let mut line = Line::default();
        line.push("☺\u{FE0F}☺\u{FE0F}☺\u{FE0F}", Style::new())
            .push("tail", Style::new());

        let fitted = line.fit(Some(3));

        assert!(fitted.width() <= 3, "{}", plain(&fitted));
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
