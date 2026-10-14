//! Pure text-editing operations (smart indent, comment toggling, line moves).
//!
//! Every function takes the buffer plus a selection expressed as char indices
//! `(start, end)` with `start <= end`, mutates the buffer, and returns the new
//! selection.

use crate::document::{byte_to_char, char_to_byte};

pub const INDENT: &str = "    ";

pub type Sel = (usize, usize);

/// Replace the selected chars with `insert`; caret ends after the insertion.
pub fn replace_selection(text: &mut String, sel: Sel, insert: &str) -> Sel {
    let (a, b) = (char_to_byte(text, sel.0), char_to_byte(text, sel.1));
    text.replace_range(a..b, insert);
    let caret = sel.0 + insert.chars().count();
    (caret, caret)
}

/// Byte range covering every full line touched by the selection (without the trailing `\n`).
fn line_block(text: &str, sel: Sel) -> (usize, usize) {
    let a = char_to_byte(text, sel.0);
    let mut b = char_to_byte(text, sel.1);
    // A selection ending at column 0 of a line does not include that line.
    if sel.1 > sel.0 && b > 0 && text.as_bytes()[b - 1] == b'\n' {
        b -= 1;
    }
    let start = text[..a].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = text[b..].find('\n').map(|i| b + i).unwrap_or(text.len());
    (start, end.max(start))
}

/// Apply `f` to every line in the selected block. Returns the new selection.
fn map_lines(text: &mut String, sel: Sel, f: impl Fn(&str) -> String) -> Sel {
    let (start, end) = line_block(text, sel);
    let old_block = text[start..end].to_string();
    let first_old_len = old_block.split('\n').next().unwrap_or("").chars().count() as isize;
    let new_block: Vec<String> = old_block.split('\n').map(f).collect();
    let new_block = new_block.join("\n");
    let first_new_len = new_block.split('\n').next().unwrap_or("").chars().count() as isize;
    text.replace_range(start..end, &new_block);

    let start_c = byte_to_char(text, start);
    if sel.0 == sel.1 {
        let delta = first_new_len - first_old_len;
        let caret = (sel.0 as isize + delta).max(start_c as isize) as usize;
        (caret, caret)
    } else {
        (start_c, start_c + new_block.chars().count())
    }
}

fn leading_ws(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// Tab: insert spaces to the next tab stop, or indent every selected line.
pub fn tab(text: &mut String, sel: Sel) -> Sel {
    let a = char_to_byte(text, sel.0);
    let b = char_to_byte(text, sel.1);
    if sel.0 != sel.1 && text[a..b].contains('\n') {
        return map_lines(text, sel, |l| if l.is_empty() { String::new() } else { format!("{INDENT}{l}") });
    }
    let line_start = text[..a].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let col = text[line_start..a].chars().count();
    let n = INDENT.len() - col % INDENT.len();
    replace_selection(text, sel, &" ".repeat(n))
}

/// Shift+Tab: remove one indent level from every selected line.
pub fn outdent(text: &mut String, sel: Sel) -> Sel {
    map_lines(text, sel, |l| {
        if let Some(rest) = l.strip_prefix('\t') {
            return rest.to_string();
        }
        let spaces = l.len() - l.trim_start_matches(' ').len();
        l[spaces.min(INDENT.len())..].to_string()
    })
}

/// Enter: newline that keeps indentation and opens a block after `{`, `(`, `[`.
pub fn newline(text: &mut String, sel: Sel) -> Sel {
    let a = char_to_byte(text, sel.0);
    let b = char_to_byte(text, sel.1);
    let line_start = text[..a].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let indent = leading_ws(&text[line_start..a]).to_string();
    let prev = text[..a].trim_end_matches([' ', '\t']).chars().last();
    let next = text[b..].chars().next();
    let opener = matches!(prev, Some('{' | '(' | '['));
    let closes = matches!((prev, next), (Some('{'), Some('}')) | (Some('('), Some(')')) | (Some('['), Some(']')));
    if opener {
        let inner = format!("\n{indent}{INDENT}");
        if closes {
            let insert = format!("{inner}\n{indent}");
            replace_selection(text, sel, &insert);
            let caret = sel.0 + inner.chars().count();
            (caret, caret)
        } else {
            replace_selection(text, sel, &inner)
        }
    } else {
        replace_selection(text, sel, &format!("\n{indent}"))
    }
}

/// Ctrl+/: toggle `//` comments on the selected lines.
pub fn toggle_comment(text: &mut String, sel: Sel, token: &str) -> Sel {
    let (start, end) = line_block(text, sel);
    let lines: Vec<&str> = text[start..end].split('\n').collect();
    let non_empty: Vec<&&str> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    if non_empty.is_empty() {
        return sel;
    }
    let all_commented = non_empty.iter().all(|l| l.trim_start().starts_with(token));
    let min_indent = non_empty.iter().map(|l| leading_ws(l).len()).min().unwrap_or(0);
    let token = token.to_string();
    map_lines(text, sel, move |l| {
        if l.trim().is_empty() {
            return l.to_string();
        }
        if all_commented {
            let ws = leading_ws(l);
            let rest = &l[ws.len() + token.len()..];
            format!("{ws}{}", rest.strip_prefix(' ').unwrap_or(rest))
        } else {
            format!("{}{token} {}", &l[..min_indent], &l[min_indent..])
        }
    })
}

/// Ctrl+Shift+D: duplicate the selected lines below.
pub fn duplicate_lines(text: &mut String, sel: Sel) -> Sel {
    let (start, end) = line_block(text, sel);
    let block = text[start..end].to_string();
    text.insert_str(end, &format!("\n{block}"));
    let shift = block.chars().count() + 1;
    (sel.0 + shift, sel.1 + shift)
}

/// Alt+Up / Alt+Down: move the selected lines.
pub fn move_lines(text: &mut String, sel: Sel, up: bool) -> Sel {
    let (start, end) = line_block(text, sel);
    if up {
        if start == 0 {
            return sel;
        }
        let prev_start = text[..start - 1].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let prev = text[prev_start..start - 1].to_string();
        let block = text[start..end].to_string();
        text.replace_range(prev_start..end, &format!("{block}\n{prev}"));
        let shift = prev.chars().count() + 1;
        (sel.0 - shift, sel.1 - shift)
    } else {
        if end >= text.len() {
            return sel;
        }
        let next_end = text[end + 1..].find('\n').map(|i| end + 1 + i).unwrap_or(text.len());
        let next = text[end + 1..next_end].to_string();
        let block = text[start..end].to_string();
        text.replace_range(start..next_end, &format!("{next}\n{block}"));
        let shift = next.chars().count() + 1;
        (sel.0 + shift, sel.1 + shift)
    }
}

/// Auto-closing brackets/quotes. Returns `None` if the typed char needs no special handling.
pub fn type_char(text: &mut String, sel: Sel, c: char) -> Option<Sel> {
    let close = match c {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '"' => '"',
        _ => {
            // Typing a closer right before the same closer just steps over it.
            if matches!(c, ')' | ']' | '}') && sel.0 == sel.1 {
                let b = char_to_byte(text, sel.1);
                if text[b..].starts_with(c) {
                    return Some((sel.0 + 1, sel.0 + 1));
                }
            }
            return None;
        }
    };
    let a = char_to_byte(text, sel.0);
    let b = char_to_byte(text, sel.1);
    if sel.0 != sel.1 {
        let inner = text[a..b].to_string();
        text.replace_range(a..b, &format!("{c}{inner}{close}"));
        return Some((sel.0 + 1, sel.1 + 1));
    }
    let next = text[b..].chars().next();
    let prev = text[..a].chars().last();
    if c == '"' {
        if next == Some('"') {
            return Some((sel.0 + 1, sel.0 + 1));
        }
        if prev.is_some_and(|p| p.is_alphanumeric() || p == '\\' || p == '"') {
            return None;
        }
    }
    // Only auto-close when the next char is whitespace, a closer, or end of text.
    if next.is_some_and(|n| !n.is_whitespace() && !matches!(n, ')' | ']' | '}' | ';' | ',')) {
        return None;
    }
    text.insert_str(a, &format!("{c}{close}"));
    Some((sel.0 + 1, sel.0 + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newline_opens_block() {
        let mut t = String::from("fn x() {}");
        let s = newline(&mut t, (8, 8));
        assert_eq!(t, "fn x() {\n    \n}");
        assert_eq!(s, (13, 13));
    }

    #[test]
    fn comment_roundtrip() {
        let mut t = String::from("    a\n    b\nc");
        let s = toggle_comment(&mut t, (0, 10), "//");
        assert_eq!(t, "    // a\n    // b\nc");
        toggle_comment(&mut t, s, "//");
        assert_eq!(t, "    a\n    b\nc");
    }

    #[test]
    fn move_and_duplicate() {
        let mut t = String::from("one\ntwo\nthree");
        let s = move_lines(&mut t, (5, 5), true);
        assert_eq!(t, "two\none\nthree");
        assert_eq!(s, (1, 1));
        let s = move_lines(&mut t, s, false);
        assert_eq!(t, "one\ntwo\nthree");
        assert_eq!(s, (5, 5));
        duplicate_lines(&mut t, (0, 0));
        assert_eq!(t, "one\none\ntwo\nthree");
    }

    #[test]
    fn tab_and_outdent() {
        let mut t = String::from("a\nb");
        let s = tab(&mut t, (0, 3));
        assert_eq!(t, "    a\n    b");
        outdent(&mut t, s);
        assert_eq!(t, "a\nb");
        let mut t = String::from("ab");
        assert_eq!(tab(&mut t, (1, 1)), (4, 4));
    }

    #[test]
    fn autoclose() {
        let mut t = String::from("x");
        assert_eq!(type_char(&mut t, (1, 1), '('), Some((2, 2)));
        assert_eq!(t, "x()");
        assert_eq!(type_char(&mut t, (2, 2), ')'), Some((3, 3)));
        assert_eq!(t, "x()");
    }
}
