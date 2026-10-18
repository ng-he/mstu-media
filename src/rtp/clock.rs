//! The clock a stream's timestamps are counted on.

/// Ninety kilohertz, which is what video rides at unless a description says
/// otherwise.
pub const VIDEO_CLOCK: u32 = 90_000;

/// Turns RTP timestamps into microseconds that start at zero and keep
/// counting past the point the 32 bit clock wraps.
pub struct Clock {
    rate: u64,
    previous: Option<u32>,
    ticks: u64,
    last: u64,
}

impl Clock {
    /// `rate` is the track's clock rate, as `a=rtpmap` gave it.
    pub fn new(rate: u32) -> Self {
        Self {
            rate: rate.max(1) as u64,
            previous: None,
            ticks: 0,
            last: 0,
        }
    }

    /// The frame's time, and how long since the one before it.
    pub fn at(&mut self, timestamp: u32) -> (u64, u32) {
        if let Some(previous) = self.previous {
            // As a signed difference, so the wrap is a step of one tick and a
            // frame out of order steps back instead of forward by four
            // billion.
            let step = timestamp.wrapping_sub(previous) as i32;

            self.ticks = self.ticks.saturating_add_signed(step as i64);
        }

        self.previous = Some(timestamp);

        let micros = self.ticks * 1_000_000 / self.rate;
        let gap = micros.saturating_sub(self.last);

        self.last = micros;

        (micros, gap.min(u32::MAX as u64) as u32)
    }

    /// The other way about: microseconds into ticks of this clock, for
    /// anything that writes timestamps rather than reads them.
    pub fn ticks(&self, micros: u64) -> u32 {
        (micros.saturating_mul(self.rate) / 1_000_000) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_is_timed_from_the_first_one_seen() {
        let mut clock = Clock::new(VIDEO_CLOCK);

        assert_eq!(
            clock.at(1_000_000),
            (0, 0),
            "the first frame starts the clock"
        );
        assert_eq!(clock.at(1_003_000), (33_333, 33_333), "a frame at thirty");
        assert_eq!(clock.at(1_006_000), (66_666, 33_333));
    }

    /// Thirteen hours in, the timestamp wraps; the frame after it is one step
    /// along, not thirteen hours back.
    #[test]
    fn the_clock_carries_on_past_the_wrap() {
        let mut clock = Clock::new(VIDEO_CLOCK);

        // A thousand ticks short of the wrap, then two thousand past it,
        // which is three thousand ticks apart: the wrap itself is one.
        clock.at(u32::MAX - 999);

        let (micros, gap) = clock.at(2_000);

        assert_eq!(gap, 3_000 * 1_000_000 / 90_000);
        assert_eq!(
            micros, gap as u64,
            "counted from the first frame, which was zero"
        );
    }

    #[test]
    fn a_time_can_be_turned_back_into_ticks() {
        let clock = Clock::new(VIDEO_CLOCK);

        assert_eq!(clock.ticks(1_000_000), 90_000);
        assert_eq!(clock.ticks(33_333), 2_999, "rounded down, as ticks are");
        assert_eq!(clock.ticks(0), 0);
    }

    /// A clock of one tick a second is as valid as any other, and a rate of
    /// nothing would divide by zero.
    #[test]
    fn a_clock_with_no_rate_still_counts() {
        let mut clock = Clock::new(0);

        clock.at(0);

        assert_eq!(clock.at(1), (1_000_000, 1_000_000));
    }
}
