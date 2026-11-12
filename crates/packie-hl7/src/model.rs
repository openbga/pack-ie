use crate::encoding::split;
use crate::path::valid_segment_name;
use crate::{EncodingCharacters, Error, Path, decode, escape};

/// A scalar value. Original wire escapes are retained until this value is edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subcomponent {
    encoded: Vec<u8>,
    decoded: Option<String>,
}

impl Default for Subcomponent {
    fn default() -> Self {
        Self {
            encoded: Vec::new(),
            decoded: Some(String::new()),
        }
    }
}

impl Subcomponent {
    fn parse(bytes: &[u8], encoding: EncodingCharacters) -> Result<Self, Error> {
        Ok(Self {
            encoded: bytes.to_vec(),
            decoded: String::from_utf8(decode(bytes, encoding)?).ok(),
        })
    }
    fn literal(value: &str, encoding: EncodingCharacters) -> Self {
        Self {
            encoded: escape(value, encoding),
            decoded: Some(value.to_owned()),
        }
    }
    fn verbatim(value: &[u8]) -> Self {
        Self {
            encoded: value.to_vec(),
            decoded: String::from_utf8(value.to_vec()).ok(),
        }
    }
    /// Decoded UTF-8; `None` means the wire value cannot be represented as UTF-8.
    #[must_use]
    pub fn value(&self) -> Option<&str> {
        self.decoded.as_deref()
    }
    /// Encoded bytes, with the partner's original escapes when unchanged.
    #[must_use]
    pub fn encoded(&self) -> &[u8] {
        &self.encoded
    }
    fn write(&self, out: &mut Vec<u8>, _encoding: EncodingCharacters) {
        out.extend_from_slice(&self.encoded);
    }
}

// Each aggregate caches decoded text so all getters can borrow a &str. Child
// vectors are read-only outside the crate; edits refresh the affected ancestors.
macro_rules! aggregate {
    ($(#[$doc:meta])* $name:ident, $child:ident, $children:ident, $delimiter:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name {
            $children: Vec<$child>,
            decoded: Option<String>,
        }
        impl Default for $name {
            fn default() -> Self {
                Self {
                    $children: Vec::new(),
                    decoded: Some(String::new()),
                }
            }
        }
        impl $name {
            fn parse(bytes: &[u8], encoding: EncodingCharacters) -> Result<Self, Error> {
                let children = split(bytes, encoding.$delimiter(), encoding.escape())
                    .into_iter()
                    .map(|bytes| $child::parse(bytes, encoding))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut node = Self {
                    $children: children,
                    decoded: None,
                };
                node.refresh(encoding);
                Ok(node)
            }
            fn literal(value: &str, encoding: EncodingCharacters) -> Self {
                Self {
                    $children: vec![$child::literal(value, encoding)],
                    decoded: Some(value.to_owned()),
                }
            }
            fn verbatim(value: &[u8]) -> Self {
                Self {
                    $children: vec![$child::verbatim(value)],
                    decoded: String::from_utf8(value.to_vec()).ok(),
                }
            }
            #[doc = concat!("The [`", stringify!($child), "`]s in order.")]
            #[must_use]
            pub fn $children(&self) -> &[$child] {
                &self.$children
            }
            /// Decoded text, with structural delimiters between children.
            #[must_use]
            pub fn value(&self) -> Option<&str> {
                self.decoded.as_deref()
            }
            fn refresh(&mut self, encoding: EncodingCharacters) {
                let mut value = String::new();
                for (index, child) in self.$children.iter().enumerate() {
                    let Some(text) = child.value() else {
                        self.decoded = None;
                        return;
                    };
                    if index != 0 {
                        value.push(char::from(encoding.$delimiter()));
                    }
                    value.push_str(text);
                }
                self.decoded = Some(value);
            }
            fn write(&self, out: &mut Vec<u8>, encoding: EncodingCharacters) {
                for (index, child) in self.$children.iter().enumerate() {
                    if index != 0 {
                        out.push(encoding.$delimiter());
                    }
                    child.write(out, encoding);
                }
            }
        }
    };
}

aggregate!(
    /// One component: subcomponents separated by the subcomponent delimiter.
    Component, Subcomponent, subcomponents, subcomponent);
aggregate!(
    /// One repetition: components separated by the component delimiter.
    Repetition,
    Component,
    components,
    component
);
aggregate!(
    /// One field: repetitions separated by the repetition delimiter.
    Field,
    Repetition,
    repetitions,
    repetition
);

/// A named segment, with one-based fields (including virtual MSH-1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    name: String,
    fields: Vec<Field>,
}

impl Segment {
    /// Three-character segment name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// All fields in order; for MSH the first two are the delimiters (MSH-1, MSH-2).
    #[must_use]
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }
    /// One-based field access, using the standard MSH numbering exception.
    #[must_use]
    pub fn field(&self, number: usize) -> Option<&Field> {
        self.fields.get(number.checked_sub(1)?)
    }
    fn parse(bytes: &[u8], number: usize, encoding: EncodingCharacters) -> Result<Self, Error> {
        let Some(name) = bytes.get(..3) else {
            return Err(Error::InvalidSegment(number));
        };
        if !valid_segment_name(name) {
            return Err(Error::InvalidSegment(number));
        }
        if name == b"MSH" && number != 1 {
            return Err(Error::UnexpectedMsh(number));
        }
        let name = name.iter().map(|byte| char::from(*byte)).collect();
        let mut fields = Vec::new();
        let rest = if number == 1 {
            fields.push(Field::verbatim(&[encoding.field()]));
            fields.push(Field::verbatim(&encoding.msh2()));
            &bytes[9..]
        } else if bytes.len() == 3 {
            return Ok(Self { name, fields });
        } else if bytes[3] == encoding.field() {
            &bytes[4..]
        } else {
            return Err(Error::InvalidSegment(number));
        };
        for field in split(rest, encoding.field(), encoding.escape()) {
            fields.push(Field::parse(field, encoding)?);
        }
        Ok(Self { name, fields })
    }
    fn write(&self, out: &mut Vec<u8>, encoding: EncodingCharacters) {
        out.extend_from_slice(self.name.as_bytes());
        if self.name == "MSH" {
            out.push(encoding.field());
            out.extend_from_slice(&encoding.msh2());
            for field in self.fields.iter().skip(2) {
                out.push(encoding.field());
                field.write(out, encoding);
            }
        } else {
            for field in &self.fields {
                out.push(encoding.field());
                field.write(out, encoding);
            }
        }
        out.push(b'\r');
    }
}

/// A generic HL7 v2 message with retained source bytes and decoded field access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    original: Vec<u8>,
    encoding: EncodingCharacters,
    segments: Vec<Segment>,
    dirty: bool,
}

impl Message {
    /// Start a message with MSH-1/MSH-2. Populate its remaining header before
    /// sending. No timestamp, identifiers, or partner defaults are invented.
    #[must_use]
    pub fn new(encoding: EncodingCharacters) -> Self {
        Self {
            original: Vec::new(),
            encoding,
            segments: vec![Segment {
                name: "MSH".to_owned(),
                fields: vec![
                    Field::verbatim(&[encoding.field()]),
                    Field::verbatim(&encoding.msh2()),
                ],
            }],
            dirty: true,
        }
    }
    /// Parse structural encoding. Requires MSH-9/10/11/12, validates segment
    /// headers, delimiters, escape syntax and raw control bytes. Does not validate
    /// message profiles, field datatypes, or clinical values. A final segment
    /// terminator may be absent; CR, LF and CRLF are accepted and preserved.
    ///
    /// # Errors
    ///
    /// The structural [`Error`] variants: [`Error::MissingMsh`],
    /// [`Error::InvalidEncodingCharacters`], [`Error::MissingHeaderField`],
    /// [`Error::InvalidSegment`], [`Error::UnexpectedMsh`],
    /// [`Error::ControlCharacter`], [`Error::TruncatedEscape`] and
    /// [`Error::InvalidHexEscape`].
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let encoding = parse_encoding(bytes)?;
        for (offset, byte) in bytes.iter().enumerate() {
            if byte.is_ascii_control() && !matches!(byte, b'\r' | b'\n') {
                return Err(Error::ControlCharacter(offset));
            }
        }
        let mut segments = Vec::new();
        let mut start = 0;
        let mut index = 0;
        while index < bytes.len() {
            if !matches!(bytes[index], b'\r' | b'\n') {
                index += 1;
                continue;
            }
            segments.push(Segment::parse(
                &bytes[start..index],
                segments.len() + 1,
                encoding,
            )?);
            if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
                index += 1;
            }
            start = index + 1;
            index += 1;
        }
        if start < bytes.len() {
            segments.push(Segment::parse(
                &bytes[start..],
                segments.len() + 1,
                encoding,
            )?);
        }
        let message = Self {
            original: bytes.to_vec(),
            encoding,
            segments,
            dirty: false,
        };
        for number in [9, 10, 11, 12] {
            if message
                .msh_field(number)
                .and_then(Field::value)
                .is_none_or(str::is_empty)
            {
                return Err(Error::MissingHeaderField(number));
            }
        }
        Ok(message)
    }
    /// Delimiters in use.
    #[must_use]
    pub fn encoding(&self) -> EncodingCharacters {
        self.encoding
    }
    /// Received bytes, unchanged even after editing. Empty for constructed messages.
    #[must_use]
    pub fn original_bytes(&self) -> &[u8] {
        &self.original
    }
    /// All segments in order.
    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }
    /// Select a one-based occurrence of a named segment.
    #[must_use]
    pub fn segment(&self, name: &str, occurrence: usize) -> Option<&Segment> {
        self.segments
            .iter()
            .filter(|segment| segment.name == name)
            .nth(occurrence.checked_sub(1)?)
    }
    /// One-based MSH field, `None` when the header is shorter.
    pub(crate) fn msh_field(&self, number: usize) -> Option<&Field> {
        self.segment("MSH", 1)?.field(number)
    }
    /// Original bytes when unchanged; canonical CR-terminated segments after edits.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        if !self.dirty {
            return self.original.clone();
        }
        let mut out = Vec::with_capacity(self.original.len());
        for segment in &self.segments {
            segment.write(&mut out, self.encoding);
        }
        out
    }
    /// Invalid paths, absent values, and values that are not UTF-8 return `None`.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&str> {
        self.get_path(&Path::parse(path).ok()?)
    }
    /// [`Message::get`] with a pre-parsed [`Path`].
    #[must_use]
    pub fn get_path(&self, path: &Path) -> Option<&str> {
        let field = self
            .segment(&path.segment, path.occurrence)?
            .field(path.field)?;
        if path.repetition.is_none() && path.component.is_none() {
            return field.value();
        }
        let repetition = field.repetitions.get(path.repetition.unwrap_or(0))?;
        let Some(component) = path.component else {
            return repetition.value();
        };
        let component = repetition.components.get(component - 1)?;
        let Some(subcomponent) = path.subcomponent else {
            return component.value();
        };
        component.subcomponents.get(subcomponent - 1)?.value()
    }
    /// Set a decoded literal. An aggregate assignment replaces its children;
    /// setting a component/subcomponent preserves all unrelated siblings.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidPath`] or [`Error::PathIndexTooLarge`] for a bad path, and
    /// [`Error::ImmutableEncodingCharacters`] for MSH-1 and MSH-2.
    pub fn set(&mut self, path: &str, value: &str) -> Result<(), Error> {
        self.set_path(&Path::parse(path)?, value)
    }
    /// [`Message::set`] with a pre-parsed [`Path`].
    ///
    /// # Errors
    ///
    /// [`Error::ImmutableEncodingCharacters`] for MSH-1 and MSH-2.
    pub fn set_path(&mut self, path: &Path, value: &str) -> Result<(), Error> {
        if path.segment == "MSH" && path.field <= 2 {
            return Err(Error::ImmutableEncodingCharacters);
        }
        let segment_index = self.ensure_segment(path);
        let segment = &mut self.segments[segment_index];
        segment
            .fields
            .resize_with(segment.fields.len().max(path.field), Field::default);
        let field = &mut segment.fields[path.field - 1];
        set_field(field, path, value, self.encoding);
        self.dirty = true;
        Ok(())
    }
    fn ensure_segment(&mut self, path: &Path) -> usize {
        let mut seen = 0;
        for (index, segment) in self.segments.iter().enumerate() {
            if segment.name != path.segment {
                continue;
            }
            seen += 1;
            if seen == path.occurrence {
                return index;
            }
        }
        for _ in seen..path.occurrence {
            self.segments.push(Segment {
                name: path.segment.clone(),
                fields: Vec::new(),
            });
        }
        self.segments.len() - 1
    }
}

fn set_field(field: &mut Field, path: &Path, value: &str, encoding: EncodingCharacters) {
    if path.repetition.is_none() && path.component.is_none() {
        *field = Field::literal(value, encoding);
        return;
    }
    let index = path.repetition.unwrap_or(0);
    field
        .repetitions
        .resize_with(field.repetitions.len().max(index + 1), Repetition::default);
    let repetition = &mut field.repetitions[index];
    if let Some(component) = path.component {
        repetition.components.resize_with(
            repetition.components.len().max(component),
            Component::default,
        );
        let component = &mut repetition.components[component - 1];
        if let Some(subcomponent) = path.subcomponent {
            component.subcomponents.resize_with(
                component.subcomponents.len().max(subcomponent),
                Subcomponent::default,
            );
            component.subcomponents[subcomponent - 1] = Subcomponent::literal(value, encoding);
            component.refresh(encoding);
        } else {
            *component = Component::literal(value, encoding);
        }
        repetition.refresh(encoding);
    } else {
        *repetition = Repetition::literal(value, encoding);
    }
    field.refresh(encoding);
}

fn parse_encoding(bytes: &[u8]) -> Result<EncodingCharacters, Error> {
    if !bytes.starts_with(b"MSH") {
        return Err(Error::MissingMsh);
    }
    if bytes.len() < 9 {
        return Err(Error::InvalidEncodingCharacters);
    }
    let encoding = EncodingCharacters::new(bytes[3], bytes[4], bytes[5], bytes[6], bytes[7])?;
    if bytes[8] != encoding.field() {
        return Err(Error::InvalidEncodingCharacters);
    }
    Ok(encoding)
}
