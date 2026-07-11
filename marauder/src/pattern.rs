use std::str::FromStr;

use crate::error::Error;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Pattern {
    bytes: Vec<PatternByte>,
}

impl Pattern {
    /// Creates an exact byte pattern.
    ///
    /// # Errors
    /// Returns an error when `bytes` is empty.
    pub fn exact(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.is_empty() {
            return Err(Error::Pattern("pattern cannot be empty".into()));
        }

        Ok(Self {
            bytes: bytes.iter().copied().map(PatternByte::Exact).collect(),
        })
    }

    #[must_use]
    pub const fn len(&self) -> usize { self.bytes.len() }

    #[must_use]
    pub const fn is_empty(&self) -> bool { self.bytes.is_empty() }

    #[must_use]
    pub fn matches_at(&self, haystack: &[u8], offset: usize) -> bool {
        haystack
            .get(offset..offset.saturating_add(self.len()))
            .is_some_and(|window| self.matches(window))
    }

    #[must_use]
    pub fn find_in(&self, haystack: &[u8]) -> Option<usize> {
        if self.is_empty() || self.len() > haystack.len() {
            return None;
        }

        haystack.windows(self.len()).position(|window| self.matches(window))
    }

    #[must_use]
    pub fn find_all_in(&self, haystack: &[u8]) -> Vec<usize> {
        if self.is_empty() || self.len() > haystack.len() {
            return Vec::new();
        }

        haystack
            .windows(self.len())
            .enumerate()
            .filter_map(|(offset, window)| self.matches(window).then_some(offset))
            .collect()
    }

    fn matches(&self, bytes: &[u8]) -> bool {
        self.bytes
            .iter()
            .zip(bytes)
            .all(|(pattern_byte, byte)| pattern_byte.matches(*byte))
    }
}

impl FromStr for Pattern {
    type Err = Error;

    fn from_str(pattern: &str) -> Result<Self, Self::Err> {
        let bytes = pattern
            .split_ascii_whitespace()
            .map(PatternByte::from_token)
            .collect::<Result<Vec<_>, _>>()?;

        if bytes.is_empty() {
            return Err(Error::Pattern("pattern cannot be empty".into()));
        }

        Ok(Self { bytes })
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum PatternByte {
    Exact(u8),
    Wildcard,
}

impl PatternByte {
    fn from_token(token: &str) -> Result<Self, Error> {
        match token {
            "?" | "??" => Ok(Self::Wildcard),
            _ if token.len() == 2 => u8::from_str_radix(token, 16)
                .map(Self::Exact)
                .map_err(|_| Error::Pattern(format!("invalid pattern byte '{token}'"))),
            _ => Err(Error::Pattern(format!("invalid pattern token '{token}'"))),
        }
    }

    const fn matches(self, byte: u8) -> bool {
        match self {
            Self::Exact(expected) => expected == byte,
            Self::Wildcard => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_pattern_finds_bytes() {
        let pattern = Pattern::exact(b"scan").expect("exact pattern");

        assert_eq!(pattern.find_in(b"marauder-scan-marker"), Some(9));
    }

    #[test]
    fn ida_pattern_supports_wildcards() {
        let pattern = Pattern::from_str("48 8B ?? 90").expect("parse pattern");

        assert!(pattern.matches_at(&[0x48, 0x8b, 0xff, 0x90], 0));
        assert!(pattern.matches_at(&[0x00, 0x48, 0x8b, 0x12, 0x90], 1));
        assert!(!pattern.matches_at(&[0x48, 0x8b, 0xff, 0xcc], 0));
    }

    #[test]
    fn find_all_reports_overlapping_matches() {
        let pattern = Pattern::from_str("AA AA").expect("parse pattern");

        assert_eq!(pattern.find_all_in(&[0xaa, 0xaa, 0xaa]), vec![0, 1]);
    }

    #[test]
    fn rejects_invalid_patterns() {
        assert!(Pattern::from_str("").is_err());
        assert!(Pattern::from_str("GG").is_err());
        assert!(Pattern::from_str("123").is_err());
    }
}
