/// A point in time with the UTC offset it was written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Timestamp {
    seconds: i64,
    nanos: u32,
    offset_seconds: i32,
}

impl Timestamp {
    const MAX_OFFSET_SECONDS: i32 = 24 * 60 * 60 - 1;

    /// Returns `None` when `nanos` is not below one second or the offset is a
    /// day or more.
    pub fn new(seconds: i64, nanos: u32, offset_seconds: i32) -> Option<Self> {
        (nanos < 1_000_000_000 && offset_seconds.abs() <= Self::MAX_OFFSET_SECONDS).then_some(
            Self {
                seconds,
                nanos,
                offset_seconds,
            },
        )
    }

    /// Seconds since the Unix epoch, in UTC.
    pub fn seconds(&self) -> i64 {
        self.seconds
    }

    pub fn nanos(&self) -> u32 {
        self.nanos
    }

    pub fn offset_seconds(&self) -> i32 {
        self.offset_seconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_out_of_range_parts() {
        assert!(Timestamp::new(0, 1_000_000_000, 0).is_none());
        assert!(Timestamp::new(0, 0, 24 * 60 * 60).is_none());
        assert!(Timestamp::new(0, 999_999_999, -(24 * 60 * 60 - 1)).is_some());
    }
}
