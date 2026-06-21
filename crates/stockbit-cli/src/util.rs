//! Small crate-private helpers shared across clients.

use url::Url;

use crate::error::Result;

/// Parse `s` as a URL, guaranteeing a trailing slash so relative `Url::join`
/// appends under (rather than replaces) the final path segment.
///
/// # Errors
///
/// Returns [`crate::error::Error::Url`] if `s` is not a valid URL.
pub(crate) fn with_trailing_slash(s: &str) -> Result<Url> {
    if s.ends_with('/') {
        Ok(Url::parse(s)?)
    } else {
        Ok(Url::parse(&format!("{s}/"))?)
    }
}

/// Truncate a string to at most `max` characters (not bytes), appending `…` when
/// cut. Multibyte-safe: never splits a UTF-8 scalar. Used to bound error bodies.
#[must_use]
pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let cut = s.char_indices().nth(max).map_or(s.len(), |(i, _)| i);
        format!("{}…", &s[..cut])
    }
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn returns_unchanged_when_under_limit() {
        assert_eq!(truncate("hi", 10), "hi");
    }

    #[test]
    fn uses_ellipsis_when_over_limit() {
        let out = truncate("0123456789abcd", 5);
        assert!(out.ends_with('…'));
        assert!(out.starts_with("01234"));
    }

    #[test]
    fn handles_multibyte_boundary_safely() {
        let s = "héllo wörld";
        let _ = truncate(s, 4);
        let _ = truncate(s, 6);
    }
}
