use std::fmt;

/// Wraps a value so that `{:?}` and `{}` print `[redacted]` instead of the value.
///
/// Message bodies, wire frames and fixed-width fields travel in this wrapper so that
/// logging a decoded structure can never leak content. Reading the value takes an explicit
/// call to [`Redacted::expose`].
#[derive(Clone, PartialEq, Eq)]
pub struct Redacted<T>(T);

impl<T> Redacted<T> {
    /// Wraps a value.
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// Borrows the wrapped value.
    pub fn expose(&self) -> &T {
        &self.0
    }

    /// Mutably borrows the wrapped value for in-crate buffers.
    pub(crate) fn expose_mut(&mut self) -> &mut T {
        &mut self.0
    }

    /// Unwraps the value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

impl<T> fmt::Display for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_and_display_redact_the_value() {
        let value = Redacted::new(String::from("SYNTHETIC-CANARY"));
        assert_eq!(format!("{value:?}"), "[redacted]");
        assert_eq!(format!("{value}"), "[redacted]");
        assert_eq!(format!("{:?}", Some(&value)), "Some([redacted])");
        assert_eq!(value.expose(), "SYNTHETIC-CANARY");
        assert_eq!(value.into_inner(), "SYNTHETIC-CANARY");
    }
}
