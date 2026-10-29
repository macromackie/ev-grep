use serde::{Deserialize, Serialize};

use crate::Error;

/// Zero-based Unicode character position. Newlines advance the line and reset the column.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Position {
    pub fn advance(self, text: &str) -> Result<Self, Error> {
        let mut position = self;
        for character in text.chars() {
            if character == '\n' {
                position.line = position
                    .line
                    .checked_add(1)
                    .ok_or(Error::PositionOverflow)?;
                position.column = 0;
            } else {
                position.column = position
                    .column
                    .checked_add(1)
                    .ok_or(Error::PositionOverflow)?;
            }
        }
        Ok(position)
    }

    fn offset(self, text: &str) -> Result<usize, Error> {
        let mut position = Self::default();
        for (offset, character) in text.char_indices() {
            if position == self {
                return Ok(offset);
            }
            if character == '\n' {
                position.line += 1;
                position.column = 0;
            } else {
                position.column += 1;
            }
        }
        if position != self {
            return Err(Error::PositionOutsideContext);
        }
        Ok(text.len())
    }
}

/// End-exclusive source range compatible with ast-grep's JSON output.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Region {
    pub start: Position,
    pub end: Position,
    #[serde(rename = "byteOffset", skip_serializing_if = "Option::is_none")]
    pub byte_offset: Option<std::ops::Range<usize>>,
}

impl Region {
    pub fn from_offsets(source: &str, offsets: std::ops::Range<usize>) -> Result<Self, Error> {
        let prefix = source
            .get(..offsets.start)
            .ok_or(Error::InvalidStartOffset)?;
        let text = source
            .get(offsets.clone())
            .ok_or(Error::InvalidSourceRange)?;
        let start = Position::default().advance(prefix)?;
        Ok(Self {
            start,
            end: start.advance(text)?,
            byte_offset: Some(offsets),
        })
    }

    pub fn validate(&self, text: &str, context: Option<&str>) -> Result<(), Error> {
        if self.start.advance(text)? != self.end {
            return Err(Error::RangeExtent);
        }
        if self.end.line == usize::MAX || self.end.column == usize::MAX {
            return Err(Error::PositionTooLarge);
        }
        if let Some(bytes) = &self.byte_offset
            && bytes.end.checked_sub(bytes.start) != Some(text.len())
        {
            return Err(Error::ByteOffsetLength);
        }
        if let Some(context) = context {
            let offsets = self.start.offset(context)?..self.end.offset(context)?;
            if context.get(offsets.clone()) != Some(text) {
                return Err(Error::ContextMismatch);
            }
            if let Some(bytes) = &self.byte_offset
                && *bytes != offsets
            {
                return Err(Error::ByteOffsetContext);
            }
        }
        Ok(())
    }

    pub fn lines(&self) -> crate::LineRange {
        let end = if self.end.column == 0 && self.end.line > self.start.line {
            self.end.line
        } else {
            self.end.line + 1
        };
        crate::LineRange {
            start: self.start.line + 1,
            end,
        }
    }
}
