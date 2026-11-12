//! Remote paths: `/`-separated, below the endpoint's `remote_directory`.

use packie_config::SftpSettings;

/// `remote_directory` without a trailing `/` (empty for the root).
pub(super) fn remote_root(settings: &SftpSettings) -> &str {
    settings.remote_directory.trim_end_matches('/')
}

/// The absolute remote path of `relative` below `root`.
pub(super) fn absolute(root: &str, relative: &str) -> String {
    match (root.is_empty(), relative.is_empty()) {
        (true, true) => "/".to_owned(),
        (false, true) => root.to_owned(),
        _ => format!("{root}/{relative}"),
    }
}

/// `directory/name`, where `directory` may be empty.
pub(super) fn join_relative(directory: &str, name: &str) -> String {
    if directory.is_empty() {
        name.to_owned()
    } else {
        format!("{directory}/{name}")
    }
}

/// A relative path with `/` separators and no empty, `.` or `..` parts (validation rejects
/// `..`; dropping it here keeps the path inside the source directory regardless).
pub(super) fn normalized_relative(path: &str) -> String {
    path.split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != "." && *part != "..")
        .collect::<Vec<_>>()
        .join("/")
}

/// Splits `a/b/c.csv` into (`a/b`, `c.csv`).
pub(super) fn split_relative(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_join_without_doubled_slashes() {
        assert_eq!(absolute("", ""), "/");
        assert_eq!(absolute("/in", ""), "/in");
        assert_eq!(absolute("", "a.csv"), "/a.csv");
        assert_eq!(absolute("/in", "sub/a.csv"), "/in/sub/a.csv");
        assert_eq!(join_relative("", "a"), "a");
        assert_eq!(join_relative("p", "a"), "p/a");
        assert_eq!(normalized_relative("./processed//done/"), "processed/done");
        assert_eq!(split_relative("a/b/c.csv"), ("a/b", "c.csv"));
        assert_eq!(split_relative("c.csv"), ("", "c.csv"));
    }
}
