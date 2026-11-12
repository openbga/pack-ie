//! Reading the command line the SCM keeps for the installed service.

use std::path::PathBuf;

use crate::service::bootstrap_argument;

/// Accumulates the words of a command line.
#[derive(Default)]
struct Splitter {
    words: Vec<String>,
    current: String,
    in_word: bool,
    quoted: bool,
    backslashes: usize,
}

impl Splitter {
    /// Pending backslashes were literal.
    fn literal_backslashes(&mut self) {
        let count = std::mem::take(&mut self.backslashes);
        self.current.extend(std::iter::repeat_n('\\', count));
    }

    /// 2n backslashes before a quote are n; an odd one makes the quote literal.
    fn quote(&mut self) {
        let count = std::mem::take(&mut self.backslashes);
        self.current.extend(std::iter::repeat_n('\\', count / 2));
        if count % 2 == 1 {
            self.current.push('"');
        } else {
            self.quoted = !self.quoted;
        }
        self.in_word = true;
    }

    fn feed(&mut self, c: char) {
        match c {
            '\\' => self.backslashes += 1,
            '"' => self.quote(),
            c if c.is_whitespace() && !self.quoted => {
                self.literal_backslashes();
                self.end_word();
            }
            c => {
                self.literal_backslashes();
                self.current.push(c);
                self.in_word = true;
            }
        }
    }

    fn end_word(&mut self) {
        if self.in_word {
            self.words.push(std::mem::take(&mut self.current));
            self.in_word = false;
        }
    }

    fn finish(mut self) -> Vec<String> {
        self.literal_backslashes();
        self.in_word |= !self.current.is_empty();
        self.end_word();
        self.words
    }
}

/// The words of a Windows command line, split by the rules of `CommandLineToArgvW`
/// (double quotes group, backslashes escape only before a quote).
fn split_command_line(line: &str) -> Vec<String> {
    let mut splitter = Splitter::default();
    for c in line.chars() {
        splitter.feed(c);
    }
    splitter.finish()
}

/// The bootstrap path in the command line the service is configured with.
pub(super) fn bootstrap_from_command_line(line: &str) -> Option<PathBuf> {
    let words = split_command_line(line);
    bootstrap_argument(words.iter().map(String::as_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_lines_split_like_windows() {
        assert_eq!(
            split_command_line(
                r#""C:\Program Files\packie\packie.exe" run --bootstrap "C:\ProgramData\Positive ACK\b.json" --service"#
            ),
            [
                r"C:\Program Files\packie\packie.exe",
                "run",
                "--bootstrap",
                r"C:\ProgramData\Positive ACK\b.json",
                "--service"
            ]
        );
        assert_eq!(
            split_command_line(r#"a  "b \"c\"" "d\\" e"#),
            ["a", r#"b "c""#, r"d\", "e"]
        );
        assert_eq!(split_command_line(r#"x "" y"#), ["x", "", "y"]);
        assert!(split_command_line("   ").is_empty());
    }

    #[test]
    fn bootstrap_comes_from_the_configured_command_line() {
        assert_eq!(
            bootstrap_from_command_line(
                r"C:\packie\packie.exe run --bootstrap C:\packie\bootstrap.json --service"
            ),
            Some(PathBuf::from(r"C:\packie\bootstrap.json"))
        );
        assert_eq!(
            bootstrap_from_command_line(r"C:\packie\packie.exe run"),
            None
        );
    }
}
