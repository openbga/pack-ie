use crate::Error;
use std::{iter::Peekable, str::Chars};

pub(crate) struct Row {
    pub line: usize,
    pub fields: Vec<String>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Start,
    Bare,
    Quoted,
    Closed,
}
struct Lexer {
    rows: Vec<Row>,
    fields: Vec<String>,
    field: String,
    state: State,
    line: usize,
    start: usize,
    active: bool,
}
impl Lexer {
    fn new(line: usize) -> Self {
        Self {
            rows: Vec::new(),
            fields: Vec::new(),
            field: String::new(),
            state: State::Start,
            line,
            start: line,
            active: false,
        }
    }
    fn finish_row(&mut self) {
        if self.active {
            self.fields.push(std::mem::take(&mut self.field));
            self.rows.push(Row {
                line: self.start,
                fields: std::mem::take(&mut self.fields),
            });
        }
        self.active = false;
        self.state = State::Start;
    }
    fn quoted(&mut self, ch: char, quote: Option<char>, chars: &mut Peekable<Chars<'_>>) {
        if Some(ch) == quote {
            if chars.next_if_eq(&ch).is_some() {
                self.field.push(ch);
            } else {
                self.state = State::Closed;
            }
            return;
        }
        self.field.push(ch);
        if ch == '\r' && chars.next_if_eq(&'\n').is_some() {
            self.field.push('\n');
        }
        if ch == '\r' || ch == '\n' {
            self.line += 1;
        }
    }
    fn outside(
        &mut self,
        ch: char,
        delimiter: char,
        quote: Option<char>,
        chars: &mut Peekable<Chars<'_>>,
    ) -> Result<(), Error> {
        if ch == '\r' || ch == '\n' {
            if ch == '\r' {
                chars.next_if_eq(&'\n');
            }
            self.finish_row();
            self.line += 1;
            self.start = self.line;
        } else if ch == delimiter {
            self.fields.push(std::mem::take(&mut self.field));
            self.state = State::Start;
            self.active = true;
        } else if Some(ch) == quote && self.state == State::Start {
            self.state = State::Quoted;
            self.active = true;
        } else if self.state == State::Closed || Some(ch) == quote {
            return Err(Error::Syntax {
                line: self.line,
                column: self.fields.len() + 1,
            });
        } else {
            self.field.push(ch);
            self.state = State::Bare;
            self.active = true;
        }
        Ok(())
    }
}
pub(crate) fn parse_rows(
    text: &str,
    delimiter: char,
    quote: Option<char>,
    line: usize,
) -> Result<Vec<Row>, Error> {
    let mut lexer = Lexer::new(line);
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if lexer.state == State::Quoted {
            lexer.quoted(ch, quote, &mut chars);
        } else {
            lexer.outside(ch, delimiter, quote, &mut chars)?;
        }
    }
    if lexer.state == State::Quoted {
        return Err(Error::Truncated {
            line: lexer.line,
            column: lexer.fields.len() + 1,
        });
    }
    lexer.finish_row();
    Ok(lexer.rows)
}
