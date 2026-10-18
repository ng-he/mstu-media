//! H.264 over RTP, as RFC 6184 packs it.

use crate::codec::video::h264::{NaluType, starts_a_decoder};

use super::{Builder, Depacketizer, Frame, Sets};

/// How a unit is packed into a packet, where it is not one unit to a packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketizationType {
    Unknown(u8),
    StapA = 24,
    FuA = 28,
}

impl From<u8> for PacketizationType {
    fn from(value: u8) -> Self {
        match value {
            24 => Self::StapA,
            28 => Self::FuA,
            _ => Self::Unknown(value),
        }
    }
}

/// The two bytes in front of each unit in an aggregation packet.
const AGGREGATE_LENGTH: usize = 2;

/// The byte after the packet's own header in a fragment.
const FRAGMENT_HEADER: usize = 2;

pub struct H264 {
    build: Builder,
    sets: Sets,
    unsupported: u64,
}

impl H264 {
    pub fn new(sets: Vec<Vec<u8>>) -> Self {
        Self {
            build: Builder::default(),
            sets: Sets::new(sets, kind),
            unsupported: 0,
        }
    }

    /// One whole NAL unit, which may be a parameter set to keep rather than a
    /// piece of a frame.
    fn nalu(&mut self, bytes: &[u8]) {
        let Some(header) = bytes.first() else {
            return;
        };

        match NaluType::from(header & 0x1f) {
            NaluType::Sps | NaluType::Pps => self.sets.insert(kind(bytes), bytes.to_vec()),
            _ if starts_a_decoder(header & 0x1f) => self.build.is_sync = true,
            _ => {}
        }

        self.build.nalu(bytes);
    }

    /// `STAP-A`: several units in one packet, each with its length in front.
    fn aggregated(&mut self, payload: &[u8]) {
        let mut at = 1;

        while at + AGGREGATE_LENGTH <= payload.len() {
            let size = u16::from_be_bytes([payload[at], payload[at + 1]]) as usize;
            let from = at + AGGREGATE_LENGTH;
            let end = from + size;

            if size == 0 || end > payload.len() {
                self.unsupported += 1;
                return;
            }

            self.nalu(&payload[from..end]);
            at = end;
        }
    }

    /// `FU-A`: one unit over several packets, its header rebuilt from the
    /// packet's own.
    fn fragment(&mut self, payload: &[u8]) {
        if payload.len() <= FRAGMENT_HEADER {
            self.unsupported += 1;
            return;
        }

        let fragment = payload[1];
        let starts = fragment & 0x80 != 0;
        let nalu = fragment & 0x1f;

        if !starts {
            self.build.more(&payload[FRAGMENT_HEADER..]);
            return;
        }

        // The type comes from the fragment, the rest of the header from the
        // packet that carried it.
        let header = (payload[0] & 0xe0) | nalu;
        let mut unit = Vec::with_capacity(payload.len() - 1);

        unit.push(header);
        unit.extend_from_slice(&payload[FRAGMENT_HEADER..]);

        if starts_a_decoder(nalu) {
            self.build.is_sync = true;
        }

        self.build.nalu(&unit);
        self.build.joining = true;
    }
}

impl Depacketizer for H264 {
    fn push(&mut self, payload: &[u8], timestamp: u32, marker: bool) {
        let Some(header) = payload.first() else {
            return;
        };

        self.build.begin(timestamp);

        match PacketizationType::from(header & 0x1f) {
            PacketizationType::StapA => self.aggregated(payload),
            PacketizationType::FuA => self.fragment(payload),

            // A unit of its own, which is most of a low bitrate stream.
            PacketizationType::Unknown(1..=23) => self.nalu(payload),

            // STAP-B, MTAP and the interleaved modes, which no camera uses.
            PacketizationType::Unknown(_) => self.unsupported += 1,
        }

        if marker {
            self.build.finish();
        }
    }

    fn pop(&mut self) -> Option<Frame> {
        self.build.ready.pop_front()
    }

    fn take_parameters(&mut self) -> Option<Vec<u8>> {
        self.sets.take()
    }

    fn unsupported(&self) -> u64 {
        self.unsupported
    }
}

/// What kind of parameter set a unit is, for keeping one of each.
fn kind(bytes: &[u8]) -> u8 {
    bytes.first().map(|header| header & 0x1f).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::{PPS, SPS, idr, slice};

    fn h264() -> H264 {
        H264::new(vec![SPS.to_vec(), PPS.to_vec()])
    }

    #[test]
    fn a_whole_unit_in_one_packet_is_a_frame_of_its_own() {
        let mut depacket = h264();

        depacket.push(&slice(&[1, 2, 3]), 9000, true);

        let frame = depacket.pop().expect("a frame");

        assert_eq!(frame.timestamp, 9000);
        assert_eq!(frame.bytes, [0, 0, 0, 1, 0x41, 1, 2, 3]);
        assert!(!frame.is_sync, "a plain slice is no place to start");
        assert!(depacket.pop().is_none());
    }

    #[test]
    fn a_key_frame_says_a_decoder_can_start_there() {
        let mut depacket = h264();

        depacket.push(&idr(&[9]), 9000, true);

        assert!(depacket.pop().expect("a frame").is_sync);
    }

    /// STAP-A, where a camera puts the parameter sets and the frame together.
    #[test]
    fn an_aggregation_packet_comes_apart_into_its_units() {
        let mut depacket = h264();
        let mut payload = vec![24u8];

        for unit in [SPS.to_vec(), PPS.to_vec(), idr(&[7])] {
            payload.extend_from_slice(&(unit.len() as u16).to_be_bytes());
            payload.extend_from_slice(&unit);
        }

        depacket.push(&payload, 9000, true);

        let frame = depacket.pop().expect("a frame");

        assert_eq!(
            frame
                .bytes
                .windows(4)
                .filter(|w| *w == [0, 0, 0, 1])
                .count(),
            3
        );
        assert!(frame.is_sync);
    }

    /// FU-A, where one slice is too big for a packet.
    #[test]
    fn a_fragmented_unit_is_joined_back_together() {
        let mut depacket = h264();

        // An IDR of nine bytes, cut into three.
        depacket.push(&[0x7c, 0x85, 1, 2, 3], 9000, false);
        depacket.push(&[0x7c, 0x05, 4, 5, 6], 9000, false);
        depacket.push(&[0x7c, 0x45, 7, 8, 9], 9000, true);

        let frame = depacket.pop().expect("a frame");

        assert_eq!(frame.bytes, [0, 0, 0, 1, 0x65, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert!(frame.is_sync, "the fragment said it was an idr");
    }

    /// Without a marker bit, which plenty of cameras leave clear, the next
    /// timestamp is what ends a frame.
    #[test]
    fn a_frame_ends_when_the_next_one_starts() {
        let mut depacket = h264();

        depacket.push(&slice(&[1]), 9000, false);
        assert!(depacket.pop().is_none(), "it may not have ended yet");

        depacket.push(&slice(&[2]), 12000, false);

        let frame = depacket.pop().expect("the frame before");

        assert_eq!(frame.timestamp, 9000);
        assert_eq!(frame.bytes, [0, 0, 0, 1, 0x41, 1]);
    }

    #[test]
    fn the_parameter_sets_are_news_once() {
        let mut depacket = h264();

        let extras = depacket.take_parameters().expect("the sets from sdp");

        assert_eq!(extras[4] & 0x1f, 7, "the sps");
        assert!(depacket.take_parameters().is_none());
    }

    /// A camera that sends no sprops in SDP sends them in band instead, and
    /// downstream still has to be told.
    #[test]
    fn sets_arriving_in_band_are_news_too() {
        let mut depacket = H264::new(vec![]);

        assert!(depacket.take_parameters().is_none());

        depacket.push(SPS, 9000, false);
        depacket.push(PPS, 9000, false);

        let extras = depacket.take_parameters().expect("the sets in band");

        assert_eq!(extras.len(), SPS.len() + PPS.len() + 8);
    }

    #[test]
    fn what_cannot_be_unpacked_is_counted_and_dropped() {
        let mut depacket = h264();

        // STAP-B, which names an interleaving order nothing here follows.
        depacket.push(&[25, 0, 0], 9000, true);

        assert_eq!(depacket.unsupported(), 1);
        assert!(depacket.pop().is_none());

        // An aggregation packet claiming more than it carries.
        depacket.push(&[24, 0xff, 0xff, 1], 9000, true);

        assert_eq!(depacket.unsupported(), 2);
    }

    #[test]
    fn an_empty_packet_is_no_packet() {
        let mut depacket = h264();

        depacket.push(&[], 9000, true);

        assert!(depacket.pop().is_none());
        assert_eq!(depacket.unsupported(), 0);
    }
}
