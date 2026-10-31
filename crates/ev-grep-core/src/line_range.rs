use core::fmt;

/// Lines counted from 1, inclusive at both ends.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the derived order compares the start line first"
)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

impl fmt::Display for LineRange {
    #[inline]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.start == self.end {
            write!(formatter, "{}", self.start)
        } else {
            write!(formatter, "{}-{}", self.start, self.end)
        }
    }
}
