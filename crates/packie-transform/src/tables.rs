//! Code tables (configuration.md §8.2 `tables`): a name maps to entries, each entry a
//! code and its replacement. A table is part of the mapping that owns it, so it is
//! versioned with the channel configuration.

use std::collections::BTreeMap;

/// One code table. Lookup is by exact match, or with case folded on both sides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeTable {
    entries: BTreeMap<String, String>,
    folded: BTreeMap<String, String>,
    ambiguous: Vec<String>,
}

impl CodeTable {
    /// Builds a table from `(code, replacement)` pairs. A later pair with the same code
    /// replaces an earlier one.
    #[must_use]
    pub fn new(entries: impl IntoIterator<Item = (String, String)>) -> Self {
        let entries: BTreeMap<String, String> = entries.into_iter().collect();
        let mut folded: BTreeMap<String, String> = BTreeMap::new();
        let mut ambiguous = Vec::new();
        for (code, replacement) in &entries {
            let key = code.to_lowercase();
            match folded.get(&key) {
                Some(existing) if existing != replacement => ambiguous.push(key),
                Some(_) => {}
                None => {
                    folded.insert(key, replacement.clone());
                }
            }
        }
        ambiguous.sort();
        ambiguous.dedup();
        Self {
            entries,
            folded,
            ambiguous,
        }
    }

    /// The replacement for `code`. With `ignore_case` both sides are lowercased first;
    /// codes that differ only by case and map to different replacements are
    /// [ambiguous](Self::is_case_ambiguous) and never match.
    #[must_use]
    pub fn lookup(&self, code: &str, ignore_case: bool) -> Option<&str> {
        if !ignore_case {
            return self.entries.get(code).map(String::as_str);
        }
        let key = code.to_lowercase();
        if self.ambiguous.binary_search(&key).is_ok() {
            return None;
        }
        self.folded.get(&key).map(String::as_str)
    }

    /// Whether two codes differ only by case but map to different replacements, so a
    /// case-folded lookup cannot choose between them.
    #[must_use]
    pub fn is_case_ambiguous(&self) -> bool {
        !self.ambiguous.is_empty()
    }

    /// The entries in code order.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the table has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(pairs: &[(&str, &str)]) -> CodeTable {
        CodeTable::new(
            pairs
                .iter()
                .map(|(a, b)| ((*a).to_owned(), (*b).to_owned())),
        )
    }

    #[test]
    fn exact_lookup_is_case_sensitive() {
        let t = table(&[("H", "High"), ("l", "Low")]);
        assert_eq!(t.lookup("H", false), Some("High"));
        assert_eq!(t.lookup("h", false), None);
        assert_eq!(t.lookup("", false), None);
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn folded_lookup_matches_either_case() {
        let t = table(&[("H", "High"), ("l", "Low")]);
        assert_eq!(t.lookup("h", true), Some("High"));
        assert_eq!(t.lookup("L", true), Some("Low"));
        assert!(!t.is_case_ambiguous());
    }

    #[test]
    fn codes_differing_only_by_case_with_different_values_are_ambiguous() {
        let t = table(&[("a", "1"), ("A", "2"), ("b", "3"), ("B", "3")]);
        assert!(t.is_case_ambiguous());
        assert_eq!(t.lookup("a", true), None);
        assert_eq!(t.lookup("a", false), Some("1"));
        assert_eq!(t.lookup("b", true), Some("3"));
    }
}
