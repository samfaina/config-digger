//! The name rule Claude Code keeps its skill names to since 2.1.299, as its
//! 2.1.302 binary checks it: a name without hidden, direction-changing or
//! blank-like characters. Lengths count UTF-16 code units, as JavaScript's
//! `length` does. Inside an emoji sequence, a joiner or a variation
//! selector is part of a visible character: Claude Code finds those
//! sequences with `\p{RGI_Emoji}`, which config-digger reads as any well
//! formed emoji sequence, a list the definition marks unverified.

use regex::Regex;
use std::sync::LazyLock;

/// The most UTF-16 code units a name holds.
const MAX_LENGTH: usize = 256;

/// Whether Claude Code takes `name` as a skill folder's name: it shows the
/// skill under it.
pub(crate) fn takes_folder_name(name: &str) -> bool {
    visible(name) && !colon_spacing(name)
}

/// Whether Claude Code takes `name` as a skill's frontmatter `name`, an
/// alias it shows and runs the skill by: letters, marks, digits, `_`, `:`,
/// `.` and `-` alone, and visible.
pub(crate) fn takes_frontmatter_name(name: &str) -> bool {
    name.chars().all(|c| is(&WORD, c)) && visible(name)
}

static WORD: LazyLock<Regex> = LazyLock::new(|| class(r"[\p{L}\p{M}\p{N}_:.-]"));
static HIDDEN: LazyLock<Regex> =
    LazyLock::new(|| class(r"[\p{Cc}\p{Cf}\p{Co}\p{Cn}  \p{Default_Ignorable_Code_Point}⠀]"));
static LETTER_NUMBER_MARK: LazyLock<Regex> = LazyLock::new(|| class(r"[\p{L}\p{N}\p{M}]"));
/// A mark, Hangul jamo included, which a letter must carry.
static MARK_LIKE: LazyLock<Regex> = LazyLock::new(|| class(r"[\p{M}ᅠ-ᇿힰ-퟿]"));
static MARK: LazyLock<Regex> = LazyLock::new(|| class(r"\p{M}"));
static SPACE_SEPARATOR: LazyLock<Regex> = LazyLock::new(|| class(r"\p{Zs}"));
static SOFT_DOTTED: LazyLock<Regex> = LazyLock::new(|| class(r"\p{Soft_Dotted}"));
static LATIN_OR_COMMON: LazyLock<Regex> = LazyLock::new(|| class(r"[\p{sc=Latin}\p{sc=Common}]"));
static INHERITED: LazyLock<Regex> = LazyLock::new(|| class(r"\p{sc=Inherited}"));
static PICTOGRAPHIC: LazyLock<Regex> = LazyLock::new(|| class(r"\p{Extended_Pictographic}"));
static SKIN_TONE: LazyLock<Regex> = LazyLock::new(|| class(r"\p{Emoji_Modifier}"));
static REGIONAL: LazyLock<Regex> = LazyLock::new(|| class(r"\p{Regional_Indicator}"));
/// An Arabic mark a ZWNJ may follow between two joining letters.
static ARABIC_MARK: LazyLock<Regex> = LazyLock::new(|| {
    class(r"[[\p{sc=Arabic}\p{sc=Inherited}]&&\p{Mn}&&[^\p{Default_Ignorable_Code_Point}]]")
});
/// The Arabic letters that join the letter after them.
static JOINS_FORWARD: LazyLock<Regex> =
    LazyLock::new(|| class(r"[ؠئبت-خس-ؿف-هىيٮٯٸ-ڇښ-ڿہۂیێېۑۺ-ۼۿݐ-ݘݜ-ݪݭ-ݰݲݵ-ݷݺ-ݿࢆࢉ-ࢍ࢏ࢠ-ࢩࢯࢰࢳ-ࢸࢺ-ࣈ]"));
/// The Arabic letters that join the letter before them.
static JOINS_BACK: LazyLock<Regex> =
    LazyLock::new(|| class(r"[ؠآ-ؿف-يٮٯٱ-ٳٵ-ۓەۮۯۺ-ۼۿݐ-ݿࡰ-ࢂࢆࢉ-࢏ࢠ-ࢬࢮ-ࣈ]"));

fn class(pattern: &str) -> Regex {
    Regex::new(&format!("^{pattern}$")).expect("a valid class")
}

fn is(class: &Regex, c: char) -> bool {
    class.is_match(c.encode_utf8(&mut [0; 4]))
}

const ZWNJ: char = '\u{200C}';
const ZWJ: char = '\u{200D}';
const VS16: char = '\u{FE0F}';
const CANCEL_TAG: char = '\u{E007F}';
const KEYCAP: char = '\u{20E3}';
/// What an emoji sequence counts as once found.
const SMILEY: char = '\u{263A}';

/// Whether `name` holds no hidden, direction-changing or blank-like
/// character, nor `<` or `>`: the binary's check while its gate is on.
fn visible(name: &str) -> bool {
    let length = name.encode_utf16().count();
    if name.is_empty() || length > MAX_LENGTH || name.chars().any(control_or_bracket) {
        return false;
    }
    if name.contains("  ") || blank_like(name) || name_skin_tone_after_vs16(name) {
        return false;
    }
    let seen = as_seen(name);
    !seen.iter().any(|&c| is(&HIDDEN, c))
        && !seen.contains(&KEYCAP)
        && !seen
            .windows(2)
            .any(|pair| pair[0] == SMILEY && is(&MARK_LIKE, pair[1]))
        && !misplaced_mark(&seen)
}

/// What the binary refuses whether its gate is on or not.
fn control_or_bracket(c: char) -> bool {
    matches!(c, '\0'..='\x1f' | '\x7f'..='\u{9f}' | '\u{2028}' | '\u{2029}' | '<' | '>')
}

/// A space separator other than the space, a character that shows as a
/// blank, or a space at either end.
fn blank_like(name: &str) -> bool {
    name.starts_with(' ')
        || name.ends_with(' ')
        || name.chars().any(|c| {
            (c != ' ' && is(&SPACE_SEPARATOR, c))
                || matches!(
                    c,
                    '\u{FFFC}' | '\u{1D159}' | '\u{13441}' | '\u{13442}' | '\u{16FE4}'
                )
        })
}

fn name_skin_tone_after_vs16(name: &str) -> bool {
    let chars: Vec<char> = name.chars().collect();
    chars
        .windows(2)
        .any(|pair| pair[0] == VS16 && ('\u{1F3FB}'..='\u{1F3FF}').contains(&pair[1]))
}

/// A space at either end or next to a `:`, or a `:` at either end.
fn colon_spacing(name: &str) -> bool {
    name.starts_with([' ', ':'])
        || name.ends_with([' ', ':'])
        || name.contains(": ")
        || name.contains(" :")
}

/// The characters of `name` as they show: each emoji sequence one
/// character, and without the joiners that join two letters of a script.
fn as_seen(name: &str) -> Vec<char> {
    let chars: Vec<char> = name.chars().collect();
    if !chars
        .iter()
        .any(|c| matches!(*c, ZWNJ | ZWJ | VS16 | CANCEL_TAG))
    {
        return chars;
    }
    let mut folded = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        match emoji_sequence(&chars, at) {
            Some(length) => {
                folded.push(SMILEY);
                at += length;
            }
            None => {
                folded.push(chars[at]);
                at += 1;
            }
        }
    }
    (0..folded.len())
        .filter(|&at| !joins_letters(&folded, at))
        .map(|at| folded[at])
        .collect()
}

/// The length of the emoji sequence starting at `at`: emoji joined by ZWJ.
fn emoji_sequence(chars: &[char], at: usize) -> Option<usize> {
    let mut length = emoji(chars, at)?;
    while chars.get(at + length) == Some(&ZWJ) {
        let Some(next) = emoji(chars, at + length + 1) else {
            break;
        };
        length += 1 + next;
    }
    Some(length)
}

/// The length of the emoji starting at `at`: a flag, a keycap, a tag
/// sequence, or a pictograph with its presentation selector and skin tone.
fn emoji(chars: &[char], at: usize) -> Option<usize> {
    let c = *chars.get(at)?;
    let next = |offset: usize| chars.get(at + offset).copied();
    if is(&REGIONAL, c) && next(1).is_some_and(|c| is(&REGIONAL, c)) {
        return Some(2);
    }
    if matches!(c, '0'..='9' | '#' | '*') {
        return (next(1) == Some(VS16) && next(2) == Some(KEYCAP)).then_some(3);
    }
    if c == '\u{1F3F4}' {
        let tags = (1..)
            .take_while(|&offset| {
                next(offset).is_some_and(|c| ('\u{E0020}'..='\u{E007E}').contains(&c))
            })
            .count();
        if tags > 0 && next(1 + tags) == Some(CANCEL_TAG) {
            return Some(tags + 2);
        }
    }
    if !is(&PICTOGRAPHIC, c) {
        return None;
    }
    let mut length = 1;
    if next(length) == Some(VS16) {
        length += 1;
    }
    if next(length).is_some_and(|c| is(&SKIN_TONE, c)) {
        length += 1;
    }
    Some(length)
}

/// The first code point of each Indic script whose consonants a joiner
/// after a virama may join: Devanagari, Bengali, Gujarati, Oriya, Telugu,
/// Kannada and Malayalam.
const INDIC: [u32; 7] = [0x900, 0x980, 0xA80, 0xB00, 0xC00, 0xC80, 0xD00];

/// Whether the character at `at` is a joiner between two letters of a
/// script that joins them with it.
fn joins_letters(chars: &[char], at: usize) -> bool {
    let c = chars[at];
    if !matches!(c, ZWNJ | ZWJ) || at == 0 {
        return false;
    }
    let before = |back: usize| at.checked_sub(back).map(|index| chars[index]);
    let after = chars.get(at + 1).copied();
    let indic = INDIC.iter().any(|&base| {
        let consonant = |c: Option<char>| {
            c.is_some_and(|c| (base + 0x15..=base + 0x39).contains(&u32::from(c)))
        };
        let code = |offset: u32| char::from_u32(base + offset);
        let nukta = base != 0xD00 && before(2) == code(0x3C) && consonant(before(3));
        before(1) == code(0x4D) && (consonant(before(2)) || nukta) && consonant(after)
    });
    let sinhala_letter =
        |c: Option<char>| c.is_some_and(|c| ('\u{0D9A}'..='\u{0DC6}').contains(&c));
    let sinhala = c == ZWJ
        && before(1) == Some('\u{0DCA}')
        && sinhala_letter(before(2))
        && sinhala_letter(after);
    let arabic = c == ZWNJ && after.is_some_and(|c| is(&JOINS_BACK, c)) && {
        let marks = (1..=8)
            .take_while(|&back| before(back).is_some_and(|c| is(&ARABIC_MARK, c)))
            .count();
        before(marks + 1).is_some_and(|c| is(&JOINS_FORWARD, c))
    };
    indic || sinhala || arabic
}

/// Whether a mark of `chars` sits where no letter carries it: first, after
/// no letter, digit or mark, ninth in a row, as a dot on a dotted letter,
/// on a dotless `ı` or `ȷ`, or of another script on a Latin or common
/// character.
fn misplaced_mark(chars: &[char]) -> bool {
    let mut run = 0;
    for (at, &c) in chars.iter().enumerate() {
        let before = || chars[..at].iter().rev();
        if is(&MARK, c) && at > 0 && matches!(chars[at - 1], '\u{0131}' | '\u{0237}') {
            return true;
        }
        if c == '\u{0307}' {
            let dotted = before()
                .find(|&&c| !is(&MARK, c) || is(&SOFT_DOTTED, c))
                .is_some_and(|&c| is(&SOFT_DOTTED, c));
            if dotted {
                return true;
            }
        }
        if !is(&MARK_LIKE, c) {
            run = 0;
            continue;
        }
        run += 1;
        let carried = at > 0 && is(&LETTER_NUMBER_MARK, chars[at - 1]);
        let on_latin = !is(&INHERITED, c)
            && before()
                .find(|&&c| !is(&MARK_LIKE, c) || is(&LATIN_OR_COMMON, c))
                .is_some_and(|&c| is(&LATIN_OR_COMMON, c));
        if !carried || run >= 9 || on_latin {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_name_keeps_inner_spaces_dots_capitals_and_letters_outside_ascii() {
        for name in [
            "a b",
            "a.b",
            "ABC",
            "ñandú",
            "日本語",
            "a\u{0301}",
            "1\u{FE0F}\u{20E3}x",
        ] {
            assert!(takes_folder_name(name), "{name:?}");
        }
    }

    #[test]
    fn a_folder_name_is_refused_with_a_hidden_or_blank_like_character() {
        for name in [
            "",
            " a",
            "a ",
            "a  b",
            "a\u{200B}b",
            "a\u{202E}b",
            "a\u{00A0}b",
            "a\u{3000}b",
            "a\tb",
            "a<b",
            "a>b",
            "a: b",
            ":a",
            "a\u{200D}b",
            "\u{0301}a",
            "a\u{093F}",
            "i\u{0307}",
            "\u{0131}\u{0301}",
            "a\u{20E3}",
        ] {
            assert!(!takes_folder_name(name), "{name:?}");
        }
        assert!(takes_folder_name(&"x".repeat(256)));
        assert!(!takes_folder_name(&"x".repeat(257)));
        assert!(!takes_folder_name(&"😀".repeat(129)));
    }

    #[test]
    fn a_joiner_inside_an_emoji_or_between_two_letters_of_a_script_is_visible() {
        for name in [
            "a👨\u{200D}👩\u{200D}👧",
            "❤\u{FE0F}",
            "👍🏽",
            "क\u{094D}\u{200D}ष",
            "ب\u{200C}ب",
            "ක\u{0DCA}\u{200D}ය",
        ] {
            assert!(takes_folder_name(name), "{name:?}");
        }
        assert!(!takes_folder_name("a\u{200C}b"));
    }

    #[test]
    fn a_frontmatter_name_holds_letters_marks_digits_and_four_signs_alone() {
        for name in ["a:b", ":a", "a:", "A_b.c-d", "ñandú", &"x".repeat(256)] {
            assert!(takes_frontmatter_name(name), "{name:?}");
        }
        for name in [
            "a b",
            "a\tb",
            "a/b",
            "a<b",
            "a>b",
            "a\u{200B}b",
            "a+b",
            &"x".repeat(257),
        ] {
            assert!(!takes_frontmatter_name(name), "{name:?}");
        }
    }
}
