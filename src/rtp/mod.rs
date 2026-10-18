//! RTP: packets, the clock their timestamps are on, and access units out of
//! them.
//!
//! A frame arrives as one packet, as several packets to be joined, or several
//! to a packet, and which it is depends on the codec. What comes out is
//! Annex-B, which is the shape the rest of this crate works in.

use std::collections::VecDeque;

use crate::nalu::START_CODE;

pub mod h264;
pub mod h265;

mod clock;
mod packet;

pub use clock::{Clock, VIDEO_CLOCK};
pub use packet::{Packet, parse};

/// One access unit: a frame, with its parameter sets in front of it when the
/// camera sent them in band.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The RTP timestamp of the packets it came from, in the track's clock.
    pub timestamp: u32,

    pub bytes: Vec<u8>,

    /// Whether a decoder can start here.
    pub is_sync: bool,
}

pub trait Depacketizer: Send {
    /// Takes one packet of the track.
    fn push(&mut self, payload: &[u8], timestamp: u32, marker: bool);

    /// An access unit that is finished, if one is.
    fn pop(&mut self) -> Option<Frame>;

    /// The parameter sets, Annex-B, the once after they change.
    fn take_parameters(&mut self) -> Option<Vec<u8>>;

    /// Packets whose packetization this does not know.
    fn unsupported(&self) -> u64;
}

/// The depacketizer for what rtpmap named, with the parameter sets SDP gave,
/// and the codec's name as mstu spells it.
pub fn depacketizer(
    encoding: &str,
    sets: Vec<Vec<u8>>,
) -> Option<(&'static str, Box<dyn Depacketizer>)> {
    match encoding {
        "H264" => Some(("h264", Box::new(h264::H264::new(sets)))),
        "H265" | "HEVC" => Some(("h265", Box::new(h265::H265::new(sets)))),
        _ => None,
    }
}

/// The parameter sets a stream has shown so far, one per kind.
///
/// A camera repeats them every so often, and may change them mid-stream when
/// it is reconfigured; the latest of each kind is the one that counts.
pub struct Sets {
    sets: Vec<(u8, Vec<u8>)>,
    changed: bool,
}

impl Sets {
    /// `kind` tells a parameter set from a frame, and one kind from another.
    fn new(sets: Vec<Vec<u8>>, kind: fn(&[u8]) -> u8) -> Self {
        let mut held = Self {
            sets: Vec::new(),
            changed: false,
        };

        for set in sets {
            held.insert(kind(&set), set);
        }

        held
    }

    fn insert(&mut self, kind: u8, bytes: Vec<u8>) {
        match self.sets.iter_mut().find(|(seen, _)| *seen == kind) {
            // Nothing changed, so nothing downstream needs telling again.
            Some((_, held)) if *held == bytes => return,
            Some((_, held)) => *held = bytes,
            None => self.sets.push((kind, bytes)),
        }

        self.changed = true;
    }

    fn take(&mut self) -> Option<Vec<u8>> {
        if !self.changed || self.sets.is_empty() {
            return None;
        }

        self.changed = false;

        Some(crate::nalu::annexb_of(
            self.sets.iter().map(|(_, set)| set.as_slice()),
        ))
    }
}

/// Collects the NAL units of one access unit, and says when it ended.
#[derive(Default)]
struct Builder {
    timestamp: Option<u32>,
    bytes: Vec<u8>,
    is_sync: bool,

    /// Whether a fragment is being joined, so the pieces of one that started
    /// before this plugin was looking are not taken for a NAL unit.
    joining: bool,

    ready: VecDeque<Frame>,
}

impl Builder {
    /// A timestamp of its own is a frame of its own, which is how an access
    /// unit ends on a camera that sets no marker bit.
    fn begin(&mut self, timestamp: u32) {
        if self.timestamp == Some(timestamp) {
            return;
        }

        self.finish();
        self.timestamp = Some(timestamp);
    }

    fn finish(&mut self) {
        self.joining = false;

        if self.bytes.is_empty() {
            return;
        }

        self.ready.push_back(Frame {
            timestamp: self.timestamp.unwrap_or(0),
            bytes: std::mem::take(&mut self.bytes),
            is_sync: std::mem::take(&mut self.is_sync),
        });
    }

    fn nalu(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        self.bytes.extend_from_slice(&START_CODE);
        self.bytes.extend_from_slice(bytes);
    }

    /// The rest of a fragmented NAL unit, which carries no header of its own.
    fn more(&mut self, bytes: &[u8]) {
        if self.joining {
            self.bytes.extend_from_slice(bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(bytes: &[u8]) -> u8 {
        bytes[0]
    }

    #[test]
    fn the_sets_sdp_gave_are_the_first_news() {
        let mut sets = Sets::new(vec![vec![7, 1], vec![8, 2]], kind);
        let extras = sets.take().expect("the sets from sdp");

        assert_eq!(extras, [0, 0, 0, 1, 7, 1, 0, 0, 0, 1, 8, 2]);
        assert!(sets.take().is_none(), "and only the once");
    }

    #[test]
    fn a_set_repeated_in_band_is_no_news() {
        let mut sets = Sets::new(vec![vec![7, 1]], kind);

        sets.take().expect("the first news");
        sets.insert(7, vec![7, 1]);

        assert!(sets.take().is_none());
    }

    #[test]
    fn a_set_that_changed_is_news_again() {
        let mut sets = Sets::new(vec![vec![7, 1]], kind);

        sets.take().expect("the first news");
        sets.insert(7, vec![7, 9]);

        assert_eq!(
            sets.take().expect("the new set"),
            [0, 0, 0, 1, 7, 9],
            "the old one is gone, not kept beside it"
        );
    }

    #[test]
    fn nothing_from_a_camera_that_sent_no_sets() {
        let mut sets = Sets::new(vec![], kind);

        assert!(sets.take().is_none());
    }

    #[test]
    fn a_unit_ends_when_the_timestamp_moves() {
        let mut build = Builder::default();

        build.begin(100);
        build.nalu(&[5, 1, 2]);
        assert!(build.ready.is_empty(), "nothing says it ended yet");

        build.begin(200);

        let frame = build.ready.pop_front().expect("the first unit");

        assert_eq!(frame.timestamp, 100);
        assert_eq!(frame.bytes, [0, 0, 0, 1, 5, 1, 2]);
    }

    #[test]
    fn the_pieces_of_a_unit_are_one_unit() {
        let mut build = Builder::default();

        build.begin(100);
        build.nalu(&[7, 1]);
        build.nalu(&[5, 2]);
        build.finish();

        let frame = build.ready.pop_front().expect("one unit of two parts");

        assert_eq!(frame.bytes, [0, 0, 0, 1, 7, 1, 0, 0, 0, 1, 5, 2]);
    }

    /// A fragment that started before this plugin was looking is not a NAL
    /// unit, and must not be glued onto the one being built.
    #[test]
    fn the_tail_of_a_fragment_nobody_saw_start_is_dropped() {
        let mut build = Builder::default();

        build.begin(100);
        build.more(&[1, 2, 3]);

        assert!(build.bytes.is_empty());
    }
}
