//! Stdout JSON formatting — compact by default, pretty with `--pretty`.

use serde_json::Value;

use crate::error::Result;

#[derive(Debug, Clone, Copy)]
pub enum Format {
    Compact,
    Pretty,
}

impl Format {
    #[must_use]
    pub fn from_pretty_flag(pretty: bool) -> Self {
        if pretty {
            Self::Pretty
        } else {
            Self::Compact
        }
    }
}

/// Write JSON to any writer with the chosen format, terminating with a newline.
pub fn write_json<W: std::io::Write>(w: &mut W, value: &Value, fmt: Format) -> Result<()> {
    match fmt {
        Format::Compact => serde_json::to_writer(&mut *w, value)?,
        Format::Pretty => serde_json::to_writer_pretty(&mut *w, value)?,
    }
    writeln!(w)?;
    Ok(())
}

/// Convenience: write to stdout.
pub fn print_json(value: &Value, fmt: Format) -> Result<()> {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    write_json(&mut lock, value, fmt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compact_is_single_line() {
        let mut buf = Vec::new();
        write_json(&mut buf, &json!({"a": 1, "b": [1, 2]}), Format::Compact).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(s, "{\"a\":1,\"b\":[1,2]}\n");
    }

    #[test]
    fn pretty_has_indent_and_newlines() {
        let mut buf = Vec::new();
        write_json(&mut buf, &json!({"a": 1}), Format::Pretty).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("\n  \"a\""));
        assert!(s.ends_with("}\n"));
    }
}
