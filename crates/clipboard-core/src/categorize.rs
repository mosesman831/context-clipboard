//! Deterministic clip categorization (SPEC §7).
//!
//! Rules, in priority order:
//! 1. `image` — MIME starts with `image/`.
//! 2. `file` — MIME is `text/uri-list`, or the text is only `file://` URLs.
//! 3. `url` — trimmed text is a single `http(s)` URL.
//! 4. `code` — shebang, markdown fences, code-ish keywords, brace/semicolon
//!    structure, or high symbol density.
//! 5. `text` — everything else.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Clip category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Url,
    Code,
    Text,
    Image,
    File,
}

impl Category {
    /// Lowercase string form used in the DB and IPC.
    pub fn as_str(&self) -> &'static str {
        match self {
            Category::Url => "url",
            Category::Code => "code",
            Category::Text => "text",
            Category::Image => "image",
            Category::File => "file",
        }
    }
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Category {
    type Err = crate::error::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "url" => Ok(Category::Url),
            "code" => Ok(Category::Code),
            "text" => Ok(Category::Text),
            "image" => Ok(Category::Image),
            "file" => Ok(Category::File),
            other => Err(crate::error::Error::InvalidInput(format!(
                "unknown category: {other}"
            ))),
        }
    }
}

/// Categorize a clip from its MIME type and optional text payload.
pub fn categorize(mime: &str, text: Option<&str>) -> Category {
    let mime_l = mime.trim().to_ascii_lowercase();

    if mime_l.starts_with("image/") {
        return Category::Image;
    }
    if mime_l == "text/uri-list" {
        return Category::File;
    }

    let text = match text {
        Some(t) => t,
        None => return Category::Text,
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Category::Text;
    }

    if looks_like_file_urls(trimmed) {
        return Category::File;
    }
    if is_single_http_url(trimmed) {
        return Category::Url;
    }
    if looks_like_code(trimmed) {
        return Category::Code;
    }

    Category::Text
}

/// True if every whitespace-separated token is a `file://` URL (and there is
/// at least one).
fn looks_like_file_urls(s: &str) -> bool {
    let mut any = false;
    for token in s.split_whitespace() {
        if token.is_empty() {
            continue;
        }
        if !token.starts_with("file://") {
            return false;
        }
        any = true;
    }
    any
}

/// True if the whole string is a single http(s) URL (no interior whitespace,
/// with a plausible host).
fn is_single_http_url(s: &str) -> bool {
    if s.chars().any(char::is_whitespace) {
        return false;
    }
    let lower = s.to_ascii_lowercase();
    let rest = if let Some(r) = lower.strip_prefix("https://") {
        r
    } else if let Some(r) = lower.strip_prefix("http://") {
        r
    } else {
        return false;
    };

    // Host portion ends at the first '/', '?', or '#'.
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches('.');
    if host.is_empty() {
        return false;
    }
    // Strip optional userinfo and port.
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    host == "localhost" || host.contains('.')
}

fn looks_like_code(s: &str) -> bool {
    if s.starts_with("#!") {
        return true; // shebang
    }
    if s.contains("```") {
        return true; // markdown fence
    }

    const INDICATORS: &[&str] = &[
        "def ",
        "class ",
        "function ",
        "fn ",
        "func ",
        "import ",
        "#include",
        "public ",
        "private ",
        "static ",
        "return ",
        "const ",
        "let ",
        "var ",
        "=>",
        "();",
        "{}",
        "</",
        "/>",
        "printf(",
        "println",
        "console.log",
        "system.out",
        "std::",
        "->",
        "::",
        "$(",
        "#define",
        "public static",
    ];
    let lower = s.to_ascii_lowercase();
    let hits = INDICATORS.iter().filter(|p| lower.contains(**p)).count();
    if hits >= 2 {
        return true;
    }

    let braces = s.matches('{').count() + s.matches('}').count();
    let semis = s.matches(';').count();
    if braces >= 2 || (semis >= 2 && s.contains('=')) {
        return true;
    }

    // High density of code-ish symbols.
    let symbols = s
        .chars()
        .filter(|c| "{}[]()<>;=+-*/%&|^~".contains(*c))
        .count();
    let non_ws = s.chars().filter(|c| !c.is_whitespace()).count().max(1);
    if symbols as f64 / non_ws as f64 > 0.18 {
        return true;
    }

    // A single strong indicator combined with multi-line structure.
    hits >= 1 && s.contains('\n')
}

/// One line of a categorization fixture file.
#[derive(Debug, Deserialize)]
struct Fixture {
    mime: String,
    #[serde(default)]
    text: Option<String>,
    category: String,
}

/// Load JSONL fixtures from `path` and score the categorizer against them.
/// Returns `(correct, total)`. Missing files yield `(0, 0)`. Blank lines and
/// lines that fail to parse are skipped (they do not count toward the total).
pub fn load_fixtures_and_score(path: &Path) -> Result<(usize, usize)> {
    if !path.exists() {
        return Ok((0, 0));
    }
    let raw = std::fs::read_to_string(path)?;
    let mut correct = 0usize;
    let mut total = 0usize;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let fx: Fixture = match serde_json::from_str(line) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let expected = match fx.category.parse::<Category>() {
            Ok(c) => c,
            Err(_) => continue,
        };
        total += 1;
        if categorize(&fx.mime, fx.text.as_deref()) == expected {
            correct += 1;
        }
    }
    Ok((correct, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_by_mime() {
        assert_eq!(categorize("image/png", None), Category::Image);
        assert_eq!(categorize("image/jpeg", Some("ignored")), Category::Image);
    }

    #[test]
    fn file_by_mime_and_urls() {
        assert_eq!(categorize("text/uri-list", Some("x")), Category::File);
        assert_eq!(
            categorize("text/plain", Some("file:///home/u/a.txt")),
            Category::File
        );
        assert_eq!(
            categorize("text/plain", Some("file:///a/b.txt\nfile:///c/d.png")),
            Category::File
        );
        // Mixed file + non-file is not a pure file list.
        assert_ne!(
            categorize("text/plain", Some("file:///a http://b.com")),
            Category::File
        );
    }

    #[test]
    fn url_single() {
        assert_eq!(
            categorize("text/plain", Some("https://example.com/path?q=1")),
            Category::Url
        );
        assert_eq!(
            categorize("text/plain", Some("  http://localhost:8080  ")),
            Category::Url
        );
        // Two URLs -> not a single URL.
        assert_ne!(
            categorize("text/plain", Some("https://a.com https://b.com")),
            Category::Url
        );
        // Missing host.
        assert_ne!(categorize("text/plain", Some("https://")), Category::Url);
        // Non-http scheme.
        assert_ne!(categorize("text/plain", Some("ftp://a.com")), Category::Url);
    }

    #[test]
    fn code_heuristics() {
        assert_eq!(
            categorize("text/plain", Some("#!/bin/bash\necho hi")),
            Category::Code
        );
        assert_eq!(
            categorize("text/plain", Some("```rust\nfn main() {}\n```")),
            Category::Code
        );
        assert_eq!(
            categorize("text/plain", Some("fn main() {\n    println!(\"hi\");\n}")),
            Category::Code
        );
        assert_eq!(
            categorize(
                "text/plain",
                Some("const x = 1;\nconst y = 2;\nreturn x + y;")
            ),
            Category::Code
        );
        assert_eq!(
            categorize("text/plain", Some("import os\nimport sys")),
            Category::Code
        );
    }

    #[test]
    fn plain_text_default() {
        assert_eq!(
            categorize("text/plain", Some("just a normal sentence here.")),
            Category::Text
        );
        assert_eq!(categorize("text/plain", None), Category::Text);
        assert_eq!(categorize("text/plain", Some("   ")), Category::Text);
        assert_eq!(
            categorize("text/plain", Some("Meeting notes: discuss roadmap")),
            Category::Text
        );
    }

    #[test]
    fn category_str_roundtrip() {
        for c in [
            Category::Url,
            Category::Code,
            Category::Text,
            Category::Image,
            Category::File,
        ] {
            assert_eq!(c.as_str().parse::<Category>().expect("parse"), c);
        }
        assert!("nope".parse::<Category>().is_err());
    }

    #[test]
    fn fixtures_missing_is_zero() {
        let (c, t) =
            load_fixtures_and_score(Path::new("/nonexistent/fixtures.jsonl")).expect("score");
        assert_eq!((c, t), (0, 0));
    }

    #[test]
    fn fixtures_scored_from_temp_file() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("cat.jsonl");
        let body = concat!(
            "{\"mime\":\"text/plain\",\"text\":\"https://rust-lang.org\",\"category\":\"url\"}\n",
            "\n",
            "{\"mime\":\"image/png\",\"text\":null,\"category\":\"image\"}\n",
            "{\"mime\":\"text/plain\",\"text\":\"hello world\",\"category\":\"text\"}\n"
        );
        std::fs::write(&path, body).expect("write");
        let (correct, total) = load_fixtures_and_score(&path).expect("score");
        assert_eq!(total, 3);
        assert_eq!(correct, 3);
    }
}
