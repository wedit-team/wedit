//! Hand-written, single-pass lexers for Rust and C#.
//!
//! The lexer works on the whole buffer at once so multi-line constructs
//! (block comments, raw / verbatim strings) highlight correctly. It never
//! fails: anything it does not recognise is emitted as `Tok::Plain`.

use std::ops::Range;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Lang {
    Rust,
    CSharp,
    Plain,
}

impl Lang {
    pub fn from_path(path: &Path) -> Lang {
        match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
            Some("rs") => Lang::Rust,
            Some("cs") | Some("csx") => Lang::CSharp,
            _ => Lang::Plain,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Lang::Rust => "Rust",
            Lang::CSharp => "C#",
            Lang::Plain => "Plain Text",
        }
    }

    pub fn line_comment(self) -> Option<&'static str> {
        match self {
            Lang::Rust | Lang::CSharp => Some("//"),
            Lang::Plain => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Plain,
    Keyword,
    Control,
    Type,
    Function,
    Macro,
    String,
    Escape,
    Number,
    Comment,
    DocComment,
    Attribute,
    Lifetime,
    Constant,
    Punct,
    Preprocessor,
}

pub type Spans = Vec<(Range<usize>, Tok)>;

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "const", "crate", "dyn", "enum", "extern", "false", "fn", "impl", "let", "mod", "move", "mut",
    "pub", "ref", "self", "static", "struct", "super", "trait", "true", "type", "union", "unsafe", "use", "where",
    "macro_rules",
];
const RUST_CONTROL: &[&str] = &["break", "continue", "else", "for", "if", "in", "loop", "match", "return", "while", "yield"];
const RUST_TYPES: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize", "f32", "f64", "bool",
    "char", "str", "Self",
];

const CS_KEYWORDS: &[&str] = &[
    "abstract", "as", "base", "checked", "class", "const", "delegate", "event", "explicit", "extern", "false",
    "fixed", "implicit", "in", "interface", "internal", "is", "lock", "namespace", "new", "null", "operator", "out",
    "override", "params", "private", "protected", "public", "readonly", "ref", "sealed", "sizeof", "stackalloc",
    "static", "struct", "this", "true", "typeof", "unchecked", "unsafe", "using", "virtual", "volatile", "async",
    "await", "get", "set", "init", "value", "var", "dynamic", "record", "where", "nameof", "partial", "required",
    "global", "add", "remove", "with", "not", "and", "or", "when", "file", "scoped", "managed", "unmanaged", "enum",
    "default", "let", "from", "select", "orderby", "group", "into", "join", "on", "equals", "by", "ascending",
    "descending",
];
const CS_CONTROL: &[&str] = &[
    "break", "case", "catch", "continue", "do", "else", "finally", "for", "foreach", "goto", "if", "return",
    "switch", "throw", "try", "while", "yield",
];
const CS_TYPES: &[&str] = &[
    "bool", "byte", "char", "decimal", "double", "float", "int", "long", "object", "sbyte", "short", "string",
    "uint", "ulong", "ushort", "void", "nint", "nuint",
];

pub fn tokenize(lang: Lang, src: &str) -> Spans {
    match lang {
        Lang::Plain => vec![(0..src.len(), Tok::Plain)],
        _ => Lexer::new(lang, src).run(),
    }
}

struct Lexer<'a> {
    lang: Lang,
    src: &'a str,
    b: &'a [u8],
    i: usize,
    out: Spans,
    plain_start: usize,
    /// True while only whitespace has been seen on the current line.
    line_start: bool,
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80
}
fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

impl<'a> Lexer<'a> {
    fn new(lang: Lang, src: &'a str) -> Self {
        Self { lang, src, b: src.as_bytes(), i: 0, out: Vec::new(), plain_start: 0, line_start: true }
    }

    fn peek(&self, off: usize) -> u8 {
        *self.b.get(self.i + off).unwrap_or(&0)
    }

    fn flush_plain(&mut self, upto: usize) {
        if upto > self.plain_start {
            self.out.push((self.plain_start..upto, Tok::Plain));
        }
    }

    fn emit(&mut self, start: usize, end: usize, tok: Tok) {
        self.flush_plain(start);
        // Merge with previous span of the same kind to keep the job small.
        if let Some(last) = self.out.last_mut() {
            if last.1 == tok && last.0.end == start {
                last.0.end = end;
                self.plain_start = end;
                return;
            }
        }
        self.out.push((start..end, tok));
        self.plain_start = end;
    }

    fn run(mut self) -> Spans {
        while self.i < self.b.len() {
            let c = self.b[self.i];
            let start = self.i;
            let was_line_start = self.line_start;
            if c == b'\n' {
                self.line_start = true;
                self.i += 1;
                continue;
            }
            if c == b' ' || c == b'\t' || c == b'\r' {
                self.i += 1;
                continue;
            }
            self.line_start = false;

            // Comments
            if c == b'/' && self.peek(1) == b'/' {
                let end = self.src[start..].find('\n').map(|n| start + n).unwrap_or(self.b.len());
                let text = &self.src[start..end];
                let doc = (text.starts_with("///") && !text.starts_with("////")) || text.starts_with("//!");
                self.emit(start, end, if doc { Tok::DocComment } else { Tok::Comment });
                self.i = end;
                continue;
            }
            if c == b'/' && self.peek(1) == b'*' {
                self.block_comment();
                continue;
            }

            match self.lang {
                Lang::Rust => {
                    if self.rust_special(c, start) {
                        continue;
                    }
                }
                Lang::CSharp => {
                    if self.cs_special(c, start, was_line_start) {
                        continue;
                    }
                }
                Lang::Plain => {}
            }

            if c == b'"' {
                self.i += 1;
                self.quoted(start, b'"', true);
                continue;
            }
            if c.is_ascii_digit() || (c == b'.' && self.peek(1).is_ascii_digit()) {
                self.number();
                continue;
            }
            if is_ident_start(c) {
                self.ident();
                continue;
            }
            if b"{}()[];,.:<>=+-*/%!&|^~?".contains(&c) {
                self.i += 1;
                self.emit(start, self.i, Tok::Punct);
                continue;
            }
            // Advance by one UTF-8 char.
            self.i += self.src[self.i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        }
        let len = self.b.len();
        self.flush_plain(len);
        self.out
    }

    fn block_comment(&mut self) {
        let start = self.i;
        self.i += 2;
        let doc = self.peek(0) == b'*' && self.peek(1) != b'/' || self.peek(0) == b'!';
        let nested = self.lang == Lang::Rust;
        let mut depth = 1;
        while self.i < self.b.len() && depth > 0 {
            if self.b[self.i] == b'*' && self.peek(1) == b'/' {
                depth -= 1;
                self.i += 2;
            } else if nested && self.b[self.i] == b'/' && self.peek(1) == b'*' {
                depth += 1;
                self.i += 2;
            } else {
                self.i += 1;
            }
        }
        self.emit(start, self.i, if doc { Tok::DocComment } else { Tok::Comment });
    }

    /// Lex a quoted literal, the opening quote already consumed. Escapes are highlighted.
    fn quoted(&mut self, start: usize, quote: u8, escapes: bool) {
        let mut seg = start;
        while self.i < self.b.len() {
            let c = self.b[self.i];
            if escapes && c == b'\\' {
                self.emit(seg, self.i, Tok::String);
                let es = self.i;
                self.i += 1;
                if let Some(ch) = self.src[self.i..].chars().next() {
                    self.i += ch.len_utf8();
                }
                if self.b.get(es + 1) == Some(&b'u') && self.peek(0) == b'{' {
                    while self.i < self.b.len() && self.b[self.i] != b'}' && self.b[self.i] != quote {
                        self.i += 1;
                    }
                    self.i = (self.i + 1).min(self.b.len());
                }
                self.i = self.i.min(self.b.len());
                self.emit(es, self.i, Tok::Escape);
                seg = self.i;
                continue;
            }
            self.i += 1;
            if c == quote {
                break;
            }
            if c == b'\n' && quote == b'\'' {
                break;
            }
        }
        self.i = self.i.min(self.b.len());
        self.emit(seg, self.i, Tok::String);
    }

    fn number(&mut self) {
        let start = self.i;
        if self.peek(0) == b'0' && matches!(self.peek(1), b'x' | b'X' | b'b' | b'B' | b'o') {
            self.i += 2;
        }
        while self.i < self.b.len() {
            let c = self.b[self.i];
            if c.is_ascii_alphanumeric() || c == b'_' {
                self.i += 1;
            } else if c == b'.' && self.peek(1).is_ascii_digit() {
                self.i += 1;
            } else if (c == b'+' || c == b'-') && matches!(self.b[self.i - 1], b'e' | b'E') && !self.src[start..].starts_with("0x") {
                self.i += 1;
            } else {
                break;
            }
        }
        self.emit(start, self.i, Tok::Number);
    }

    fn ident(&mut self) {
        let start = self.i;
        while self.i < self.b.len() && is_ident(self.b[self.i]) {
            self.i += 1;
        }
        let word = &self.src[start..self.i];
        let (kw, ctl, ty) = match self.lang {
            Lang::Rust => (RUST_KEYWORDS, RUST_CONTROL, RUST_TYPES),
            _ => (CS_KEYWORDS, CS_CONTROL, CS_TYPES),
        };
        // Look past whitespace (and C# generic args) for a call paren.
        let after = self.skip_generics(self.i);
        let next_non_ws = self.src[after..].bytes().find(|c| !c.is_ascii_whitespace()).unwrap_or(0);
        let tok = if ctl.contains(&word) {
            Tok::Control
        } else if ty.contains(&word) {
            Tok::Type
        } else if kw.contains(&word) {
            Tok::Keyword
        } else if self.lang == Lang::Rust && self.peek(0) == b'!' && self.peek(1) != b'=' {
            self.i += 1;
            Tok::Macro
        } else if next_non_ws == b'(' {
            Tok::Function
        } else if self.lang == Lang::Rust && self.src[self.i..].starts_with("::<") && word.chars().next().is_some_and(|c| c.is_lowercase()) {
            Tok::Function
        } else if word.len() > 1 && word.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_') && word.bytes().any(|c| c.is_ascii_uppercase()) {
            Tok::Constant
        } else if word.chars().next().is_some_and(|c| c.is_uppercase()) {
            Tok::Type
        } else {
            Tok::Plain
        };
        self.emit(start, self.i, tok);
    }

    /// If a generic argument list `<...>` starts at `pos`, return the index just after it.
    fn skip_generics(&self, pos: usize) -> usize {
        if self.b.get(pos) != Some(&b'<') {
            return pos;
        }
        let mut depth = 0;
        for (k, &c) in self.b[pos..].iter().enumerate().take(200) {
            match c {
                b'<' => depth += 1,
                b'>' => {
                    depth -= 1;
                    if depth == 0 {
                        return pos + k + 1;
                    }
                }
                b' ' | b',' | b'\'' | b'&' | b':' | b'[' | b']' | b'?' | b'_' => {}
                c if c.is_ascii_alphanumeric() || c >= 0x80 => {}
                _ => return pos,
            }
        }
        pos
    }

    /// Rust-only tokens. Returns true if something was consumed.
    fn rust_special(&mut self, c: u8, start: usize) -> bool {
        // Attributes: #[...] and #![...]
        if c == b'#' && (self.peek(1) == b'[' || (self.peek(1) == b'!' && self.peek(2) == b'[')) {
            let mut depth = 0;
            while self.i < self.b.len() {
                match self.b[self.i] {
                    b'[' => depth += 1,
                    b']' => {
                        depth -= 1;
                        if depth == 0 {
                            self.i += 1;
                            break;
                        }
                    }
                    b'\n' if depth == 0 => break,
                    _ => {}
                }
                self.i += 1;
            }
            self.emit(start, self.i, Tok::Attribute);
            return true;
        }
        // Raw strings: r"..", r#".."#, br"..", and byte strings b"..".
        let (prefix_len, raw) = match (c, self.peek(1), self.peek(2)) {
            (b'r', b'"', _) | (b'r', b'#', _) => (1, true),
            (b'b', b'r', b'"') | (b'b', b'r', b'#') | (b'c', b'r', b'"') | (b'c', b'r', b'#') => (2, true),
            (b'b', b'"', _) | (b'c', b'"', _) => (1, false),
            (b'b', b'\'', _) => {
                self.i += 2;
                self.quoted(start, b'\'', true);
                return true;
            }
            _ => (0, false),
        };
        if prefix_len > 0 && (start == 0 || !is_ident(self.b[start - 1])) {
            if raw {
                let mut j = self.i + prefix_len;
                let mut hashes = 0;
                while self.b.get(j) == Some(&b'#') {
                    hashes += 1;
                    j += 1;
                }
                if self.b.get(j) == Some(&b'"') {
                    let close: String = std::iter::once('"').chain(std::iter::repeat_n('#', hashes)).collect();
                    let body = j + 1;
                    let end = self.src[body..].find(&close).map(|n| body + n + close.len()).unwrap_or(self.b.len());
                    self.i = end;
                    self.emit(start, end, Tok::String);
                    return true;
                }
            } else {
                self.i += prefix_len + 1;
                self.quoted(start, b'"', true);
                return true;
            }
        }
        // Char literal vs lifetime / label.
        if c == b'\'' {
            let n1 = self.peek(1);
            if n1 == b'\\' {
                self.i += 1;
                self.quoted(start, b'\'', true);
                return true;
            }
            // 'x' (any single char followed by a closing quote)
            if let Some(ch) = self.src[self.i + 1..].chars().next() {
                let after = self.i + 1 + ch.len_utf8();
                if self.b.get(after) == Some(&b'\'') {
                    self.i = after + 1;
                    self.emit(start, self.i, Tok::String);
                    return true;
                }
            }
            if is_ident_start(n1) {
                self.i += 1;
                while self.i < self.b.len() && is_ident(self.b[self.i]) {
                    self.i += 1;
                }
                self.emit(start, self.i, Tok::Lifetime);
                return true;
            }
        }
        false
    }

    /// C#-only tokens. Returns true if something was consumed.
    fn cs_special(&mut self, c: u8, start: usize, line_start: bool) -> bool {
        // Preprocessor directives: #region, #if, #nullable ...
        if c == b'#' && line_start {
            let end = self.src[start..].find('\n').map(|n| start + n).unwrap_or(self.b.len());
            self.i = end;
            self.emit(start, end, Tok::Preprocessor);
            return true;
        }
        // Attributes: `[Name...]` at the start of a line.
        if c == b'[' && line_start {
            let rest = &self.src[start + 1..];
            let first = rest.trim_start().bytes().next().unwrap_or(0);
            if is_ident_start(first) {
                let mut depth = 0;
                let mut in_str = false;
                while self.i < self.b.len() {
                    let ch = self.b[self.i];
                    if in_str {
                        if ch == b'\\' {
                            self.i += 1;
                        } else if ch == b'"' {
                            in_str = false;
                        }
                    } else {
                        match ch {
                            b'"' => in_str = true,
                            b'[' => depth += 1,
                            b']' => {
                                depth -= 1;
                                if depth == 0 {
                                    self.i += 1;
                                    break;
                                }
                            }
                            b'\n' => break,
                            _ => {}
                        }
                    }
                    self.i += 1;
                }
                self.emit(start, self.i, Tok::Attribute);
                return true;
            }
        }
        // String prefixes: @"..", $"..", $@"..", @$"..", raw """..."""
        let mut j = self.i;
        let mut verbatim = false;
        let mut interp = 0;
        loop {
            match self.b.get(j) {
                Some(b'@') => verbatim = true,
                Some(b'$') => interp += 1,
                _ => break,
            }
            j += 1;
        }
        if self.b.get(j) == Some(&b'"') {
            // Raw string literal: three or more quotes.
            let mut q = 0;
            while self.b.get(j + q) == Some(&b'"') {
                q += 1;
            }
            if q >= 3 {
                let close = "\"".repeat(q);
                let body = j + q;
                let end = self.src[body..].find(&close).map(|n| body + n + q).unwrap_or(self.b.len());
                self.i = end;
                self.emit(start, end, Tok::String);
                return true;
            }
            if verbatim {
                // "" is an escaped quote inside verbatim strings.
                let mut k = j + 1;
                while k < self.b.len() {
                    if self.b[k] == b'"' {
                        if self.b.get(k + 1) == Some(&b'"') {
                            k += 2;
                            continue;
                        }
                        k += 1;
                        break;
                    }
                    k += 1;
                }
                self.i = k.min(self.b.len());
                self.emit(start, self.i, Tok::String);
                return true;
            }
            if interp > 0 {
                self.i = j + 1;
                self.quoted(start, b'"', true);
                return true;
            }
        }
        // Verbatim identifier @class
        if c == b'@' && is_ident_start(self.peek(1)) {
            self.i += 1;
            while self.i < self.b.len() && is_ident(self.b[self.i]) {
                self.i += 1;
            }
            self.emit(start, self.i, Tok::Plain);
            return true;
        }
        if c == b'\'' {
            self.i += 1;
            self.quoted(start, b'\'', true);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(lang: Lang, src: &str) -> Vec<(String, Tok)> {
        tokenize(lang, src)
            .into_iter()
            .filter(|(_, t)| *t != Tok::Plain)
            .map(|(r, t)| (src[r].to_string(), t))
            .collect()
    }

    #[test]
    fn spans_cover_text() {
        for (lang, src) in [
            (Lang::Rust, "fn main() { let s = r#\"hi\"#; /* /* nested */ */ 'a' }\n"),
            (Lang::CSharp, "#region X\n[Serializable]\npublic class A { string s = @\"a\"\"b\"; }\n"),
            (Lang::Rust, "let x = \"unterminated"),
            (Lang::CSharp, "var s = $\"{x} \\n é\";"),
        ] {
            let spans = tokenize(lang, src);
            let mut pos = 0;
            for (r, _) in &spans {
                assert_eq!(r.start, pos, "gap in {src:?}");
                pos = r.end;
            }
            assert_eq!(pos, src.len());
        }
    }

    #[test]
    fn rust_tokens() {
        let k = kinds(Lang::Rust, "fn foo<'a>(x: &'a str) -> Vec<u8> { println!(\"hi\\n\"); let c = 'x'; MAX }");
        assert!(k.contains(&("fn".into(), Tok::Keyword)));
        assert!(k.contains(&("foo".into(), Tok::Function)));
        assert!(k.contains(&("'a".into(), Tok::Lifetime)));
        assert!(k.contains(&("println!".into(), Tok::Macro)));
        assert!(k.contains(&("\\n".into(), Tok::Escape)));
        assert!(k.contains(&("'x'".into(), Tok::String)));
        assert!(k.contains(&("Vec".into(), Tok::Type)));
        assert!(k.contains(&("MAX".into(), Tok::Constant)));
    }

    #[test]
    fn csharp_tokens() {
        let k = kinds(Lang::CSharp, "#nullable enable\n[Obsolete(\"x\")]\npublic static void Main() { foreach (var x in xs) Console.WriteLine(@\"a\"\"b\"); }");
        assert!(k.contains(&("#nullable enable".into(), Tok::Preprocessor)));
        assert!(k.contains(&("[Obsolete(\"x\")]".into(), Tok::Attribute)));
        assert!(k.contains(&("void".into(), Tok::Type)));
        assert!(k.contains(&("foreach".into(), Tok::Control)));
        assert!(k.contains(&("WriteLine".into(), Tok::Function)));
        assert!(k.contains(&("@\"a\"\"b\"".into(), Tok::String)));
    }
}
