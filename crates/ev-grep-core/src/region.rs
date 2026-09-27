use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

/// Zero-based Unicode character position. Newlines advance the line and reset the column.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Position {
    pub fn advance(self, text: &str) -> Result<Self> {
        let mut position = self;
        for character in text.chars() {
            if character == '\n' {
                position.line = position.line.checked_add(1).context("line overflow")?;
                position.column = 0;
            } else {
                position.column = position.column.checked_add(1).context("column overflow")?;
            }
        }
        Ok(position)
    }

    fn offset(self, text: &str) -> Result<usize> {
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
        ensure!(position == self, "position is outside context");
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
    pub fn from_offsets(source: &str, offsets: std::ops::Range<usize>) -> Result<Self> {
        let prefix = source
            .get(..offsets.start)
            .context("invalid start offset")?;
        let text = source
            .get(offsets.clone())
            .context("invalid source range")?;
        let start = Position::default().advance(prefix)?;
        Ok(Self {
            start,
            end: start.advance(text)?,
            byte_offset: Some(offsets),
        })
    }

    pub fn validate(&self, text: &str, context: Option<&str>) -> Result<()> {
        ensure!(
            self.start.advance(text)? == self.end,
            "range does not match text extent"
        );
        ensure!(
            self.end.line < usize::MAX && self.end.column < usize::MAX,
            "position is too large"
        );
        if let Some(bytes) = &self.byte_offset {
            ensure!(
                bytes.end.checked_sub(bytes.start) == Some(text.len()),
                "byte offsets do not match text length"
            );
        }
        if let Some(context) = context {
            let offsets = self.start.offset(context)?..self.end.offset(context)?;
            ensure!(
                context.get(offsets.clone()) == Some(text),
                "context slice does not match candidate text"
            );
            if let Some(bytes) = &self.byte_offset {
                ensure!(*bytes == offsets, "byte offsets disagree with context");
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
