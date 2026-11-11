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
        if pretty { Self::Pretty } else { Self::Compact }
    }
}

/// Write JSON to any writer with the chosen format, terminating with a newline.
/// # Errors
///
/// Fails on I/O errors writing to `w`, or on serialization errors.
pub fn write_json<W: std::io::Write>(w: &mut W, value: &Value, fmt: Format) -> Result<()> {
    match fmt {
        Format::Compact => serde_json::to_writer(&mut *w, value)?,
        Format::Pretty => serde_json::to_writer_pretty(&mut *w, value)?,
    }
    writeln!(w)?;
    Ok(())
}

/// Convenience: write to stdout.
/// # Errors
///
/// Fails on I/O errors writing to stdout, or on serialization errors.
pub fn print_json(value: &Value, fmt: Format) -> Result<()> {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    write_json(&mut lock, value, fmt)
}
