use crate::syntax::Lang;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
}

impl LineEnding {
    pub fn label(self) -> &'static str {
        match self {
            LineEnding::Lf => "LF",
            LineEnding::CrLf => "CRLF",
        }
    }
}

pub struct Document {
    pub id: u64,
    pub path: Option<PathBuf>,
    pub untitled_index: usize,
    /// Buffer contents, always normalised to `\n` line endings.
    pub text: String,
    saved_text: String,
    pub lang: Lang,
    pub line_ending: LineEnding,
    /// Cursor char index, updated every frame from the editor.
    pub cursor: usize,
    pub selection_len: usize,
    /// Set when the file had invalid UTF-8 and was lossily decoded.
    pub lossy: bool,
}

impl Document {
    pub fn new_untitled(id: u64, untitled_index: usize) -> Self {
        Self {
            id,
            path: None,
            untitled_index,
            text: String::new(),
            saved_text: String::new(),
            lang: Lang::Plain,
            line_ending: if cfg!(windows) { LineEnding::CrLf } else { LineEnding::Lf },
            cursor: 0,
            selection_len: 0,
            lossy: false,
        }
    }

    pub fn open(id: u64, path: &Path) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
        let (raw, lossy) = match std::str::from_utf8(bytes) {
            Ok(s) => (s.to_owned(), false),
            Err(_) => (String::from_utf8_lossy(bytes).into_owned(), true),
        };
        let line_ending = if raw.contains("\r\n") { LineEnding::CrLf } else { LineEnding::Lf };
        let text = raw.replace("\r\n", "\n");
        Ok(Self {
            id,
            path: Some(path.to_path_buf()),
            untitled_index: 0,
            saved_text: text.clone(),
            text,
            lang: Lang::from_path(path),
            line_ending,
            cursor: 0,
            selection_len: 0,
            lossy,
        })
    }

    pub fn save_to(&mut self, path: &Path) -> std::io::Result<()> {
        let out = match self.line_ending {
            LineEnding::Lf => self.text.clone(),
            LineEnding::CrLf => self.text.replace('\n', "\r\n"),
        };
        std::fs::write(path, out)?;
        self.saved_text = self.text.clone();
        if self.path.as_deref() != Some(path) {
            self.path = Some(path.to_path_buf());
            self.lang = Lang::from_path(path);
        }
        self.lossy = false;
        Ok(())
    }

    pub fn is_dirty(&self) -> bool {
        self.text != self.saved_text
    }

    /// File name with extension, e.g. `main.rs`, or `Untitled-1`.
    pub fn title(&self) -> String {
        match &self.path {
            Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string()),
            None => format!("Untitled-{}", self.untitled_index),
        }
    }

    /// 1-based (line, column) of the cursor.
    pub fn line_col(&self) -> (usize, usize) {
        let mut line = 1;
        let mut col = 1;
        for (i, c) in self.text.chars().enumerate() {
            if i >= self.cursor {
                break;
            }
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        (line, col)
    }

    pub fn line_count(&self) -> usize {
        self.text.bytes().filter(|&b| b == b'\n').count() + 1
    }
}

/// Convert a char index into a byte index for `s`.
pub fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(s.len())
}

/// Convert a byte index into a char index for `s`.
pub fn byte_to_char(s: &str, byte_idx: usize) -> usize {
    s[..byte_idx.min(s.len())].chars().count()
}

/// Char index of the first character of 1-based line `line`.
pub fn line_start_char(s: &str, line: usize) -> usize {
    if line <= 1 {
        return 0;
    }
    let mut cur = 1;
    for (i, c) in s.chars().enumerate() {
        if c == '\n' {
            cur += 1;
            if cur == line {
                return i + 1;
            }
        }
    }
    s.chars().count()
}
