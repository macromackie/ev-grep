use core::ops::Range;

use serde::{Deserialize, Serialize};

use crate::{InputError, LineRange};

/// Zero-based Unicode character position. Newlines advance the line and reset the column.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the field order is the order of ast-grep's JSON keys"
)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Position {
    /// The position after `text`, counting a newline as the start of the next line.
    #[inline]
    pub fn advance(self, text: &str) -> Result<Self, InputError> {
        let mut position = self;
        for character in text.chars() {
            if character == '\n' {
                position.line = position
                    .line
                    .checked_add(1)
                    .ok_or(InputError::PositionOverflow)?;
                position.column = 0;
            } else {
                position.column = position
                    .column
                    .checked_add(1)
                    .ok_or(InputError::PositionOverflow)?;
            }
        }
        Ok(position)
    }

    fn find_offset(self, text: &str) -> Result<usize, InputError> {
        let mut position = Self::default();
        for (offset, character) in text.char_indices() {
            if position == self {
                return Ok(offset);
            }
            if character == '\n' {
                position.line = position
                    .line
                    .checked_add(1)
                    .ok_or(InputError::PositionOverflow)?;
                position.column = 0;
            } else {
                position.column = position
                    .column
                    .checked_add(1)
                    .ok_or(InputError::PositionOverflow)?;
            }
        }
        if position != self {
            return Err(InputError::PositionOutsideContext);
        }
        Ok(text.len())
    }
}

/// End-exclusive source range compatible with ast-grep's JSON output.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the field order is the order of ast-grep's JSON keys"
)]
pub struct Region {
    pub start: Position,
    pub end: Position,
    #[serde(rename = "byteOffset", skip_serializing_if = "Option::is_none")]
    pub byte_offset: Option<Range<usize>>,
}

impl Region {
    /// The region that `offsets`, a byte range, covers in `source`; an error when it is not on
    /// character boundaries.
    #[inline]
    pub fn from_offsets(source: &str, offsets: Range<usize>) -> Result<Self, InputError> {
        let prefix = source
            .get(..offsets.start)
            .ok_or(InputError::InvalidStartOffset)?;
        let text = source
            .get(offsets.clone())
            .ok_or(InputError::InvalidSourceRange)?;
        let start = Position::default().advance(prefix)?;
        Ok(Self {
            start,
            end: start.advance(text)?,
            byte_offset: Some(offsets),
        })
    }

    /// The one-based, end-inclusive lines the region covers.
    #[inline]
    #[must_use]
    pub const fn to_lines(&self) -> LineRange {
        let end = if self.end.column == 0 && self.end.line > self.start.line {
            self.end.line
        } else {
            self.end.line.saturating_add(1)
        };
        LineRange {
            start: self.start.line.saturating_add(1),
            end,
        }
    }

    /// Check that the region spans `text`, and that it sits in `context` when one is given.
    #[inline]
    pub fn validate(&self, text: &str, context: Option<&str>) -> Result<(), InputError> {
        if self.start.advance(text)? != self.end {
            return Err(InputError::RangeExtent);
        }
        if self.end.line == usize::MAX || self.end.column == usize::MAX {
            return Err(InputError::PositionTooLarge);
        }
        if let Some(bytes) = &self.byte_offset
            && bytes.end.checked_sub(bytes.start) != Some(text.len())
        {
            return Err(InputError::ByteOffsetLength);
        }
        if let Some(whole) = context {
            let offsets = self.start.find_offset(whole)?..self.end.find_offset(whole)?;
            if whole.get(offsets.clone()) != Some(text) {
                return Err(InputError::ContextMismatch);
            }
            if let Some(bytes) = &self.byte_offset
                && *bytes != offsets
            {
                return Err(InputError::ByteOffsetContext);
            }
        }
        Ok(())
    }
}
