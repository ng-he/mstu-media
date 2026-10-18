//! H.265 over RTP, as RFC 7798 packs it.
//!
//! The same three shapes as H.264, over a two byte NAL header: a unit of its
//! own, an aggregation packet, a fragment.

use crate::codec::video::h265::{NaluType, starts_a_decoder};

use super::{Builder, Depacketizer, Frame, Sets};

/// How a unit is packed into a packet, where it is not one unit to a packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketizationType {
    Unknown(u8),
    Ap = 48,
    Fu = 49,
}

impl From<u8> for PacketizationType {
    fn from(value: u8) -> Self {
        match value {
            48 => Self::Ap,
            49 => Self::Fu,
            _ => Self::Unknown(value),
        }
    }
}

/// The NAL header, which is two bytes here rather than one.
const HEADER: usize = 2;

/// The two bytes in front of each unit in an aggregation packet.
const AGGREGATE_LENGTH: usize = 2;

/// A fragment carries the packet's header and then its own byte.
const FRAGMENT_HEADER: usize = 3;

pub struct H265 {
    build: Builder,
    sets: Sets,
    unsupported: u64,
}

impl H265 {
    pub fn new(sets: Vec<Vec<u8>>) -> Self {
        Self {
            build: Builder::default(),
            sets: Sets::new(sets, kind),
            unsupported: 0,
        }
    }

    fn nalu(&mut self, bytes: &[u8]) {
        if bytes.len() < HEADER {
            self.unsupported += 1;
            return;
        }

        let nalu = kind(bytes);

        match NaluType::from(nalu) {
            NaluType::Vps | NaluType::Sps | NaluType::Pps => self.sets.insert(nalu, bytes.to_vec()),
            _ if starts_a_decoder(nalu) => self.build.is_sync = true,
            _ => {}
        }

        self.build.nalu(bytes);
    }

    /// `AP`: several units in one packet, each with its length in front.
    fn aggregated(&mut self, payload: &[u8]) {
        let mut at = HEADER;

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

    /// `FU`: one unit over several packets, its header rebuilt from the
    /// packet's own.
    fn fragment(&mut self, payload: &[u8]) {
        if payload.len() <= FRAGMENT_HEADER {
            self.unsupported += 1;
            return;
        }

        let fragment = payload[HEADER];
        let starts = fragment & 0x80 != 0;
        let nalu = fragment & 0x3f;

        if !starts {
            self.build.more(&payload[FRAGMENT_HEADER..]);
            return;
        }

        // The type goes back into the header the packet carried, layer and
        // temporal id and all.
        let mut unit = Vec::with_capacity(payload.len() - 1);

        unit.push((payload[0] & 0x81) | (nalu << 1));
        unit.push(payload[1]);
        unit.extend_from_slice(&payload[FRAGMENT_HEADER..]);

        if starts_a_decoder(nalu) {
            self.build.is_sync = true;
        }

        self.build.nalu(&unit);
        self.build.joining = true;
    }
}

impl Depacketizer for H265 {
    fn push(&mut self, payload: &[u8], timestamp: u32, marker: bool) {
        if payload.len() < HEADER {
            return;
        }

        self.build.begin(timestamp);

        match PacketizationType::from(kind(payload)) {
            PacketizationType::Ap => self.aggregated(payload),
            PacketizationType::Fu => self.fragment(payload),

            // 50 is PACI, which carries another packet inside it.
            PacketizationType::Unknown(0..=47) => self.nalu(payload),
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

/// The NAL unit type, out of the first of the two header bytes.
fn kind(bytes: &[u8]) -> u8 {
    bytes
        .first()
        .map(|header| (header >> 1) & 0x3f)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::h265::{PPS, SPS, VPS, idr, slice};

    fn h265() -> H265 {
        H265::new(vec![VPS.to_vec(), SPS.to_vec(), PPS.to_vec()])
    }

    #[test]
    fn a_whole_unit_in_one_packet_is_a_frame_of_its_own() {
        let mut depacket = h265();

        depacket.push(&slice(&[1, 2]), 9000, true);

        let frame = depacket.pop().expect("a frame");

        assert_eq!(frame.timestamp, 9000);
        assert_eq!(frame.bytes, [0, 0, 0, 1, 0x02, 0x01, 1, 2]);
        assert!(!frame.is_sync);
    }

    #[test]
    fn a_random_access_unit_says_a_decoder_can_start_there() {
        let mut depacket = h265();

        depacket.push(&idr(&[9]), 9000, true);

        assert!(depacket.pop().expect("a frame").is_sync);
    }

    #[test]
    fn an_aggregation_packet_comes_apart_into_its_units() {
        let mut depacket = h265();
        let mut payload = vec![96u8, 1];

        for unit in [VPS.to_vec(), SPS.to_vec(), PPS.to_vec(), idr(&[7])] {
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
            4
        );
        assert!(frame.is_sync);
    }

    #[test]
    fn a_fragmented_unit_is_joined_back_together() {
        let mut depacket = h265();

        // An IDR of six bytes, cut in two: type 19 with the start bit, then
        // the end bit.
        depacket.push(&[98, 1, 0x93, 1, 2, 3], 9000, false);
        depacket.push(&[98, 1, 0x53, 4, 5, 6], 9000, true);

        let frame = depacket.pop().expect("a frame");

        assert_eq!(frame.bytes, [0, 0, 0, 1, 0x26, 1, 1, 2, 3, 4, 5, 6]);
        assert!(frame.is_sync);
    }

    #[test]
    fn the_three_parameter_sets_are_news_once() {
        let mut depacket = h265();
        let extras = depacket.take_parameters().expect("the sets from sdp");

        assert_eq!((extras[4] >> 1) & 0x3f, 32, "the vps first");
        assert!(depacket.take_parameters().is_none());
    }

    #[test]
    fn what_cannot_be_unpacked_is_counted_and_dropped() {
        let mut depacket = h265();

        // PACI, which wraps a packet inside a packet.
        depacket.push(&[100, 1, 0, 0], 9000, true);

        assert_eq!(depacket.unsupported(), 1);
        assert!(depacket.pop().is_none());
    }

    #[test]
    fn a_packet_too_short_to_hold_a_header_is_no_packet() {
        let mut depacket = h265();

        depacket.push(&[0x26], 9000, true);

        assert!(depacket.pop().is_none());
        assert_eq!(depacket.unsupported(), 0);
    }
}
