//! RTP packets, as they arrive inside the RTSP connection.

/// The fixed part of the header: version, payload type, sequence, timestamp
/// and source.
const HEADER: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet<'a> {
    pub payload_type: u8,
    pub sequence: u16,
    pub timestamp: u32,

    /// Set on the last packet of an access unit, where a camera bothers.
    pub marker: bool,

    pub payload: &'a [u8],
}

/// None if `bytes` is not an RTP packet, or claims more of itself than it has.
pub fn parse(bytes: &[u8]) -> Option<Packet<'_>> {
    if bytes.len() < HEADER || bytes[0] >> 6 != 2 {
        return None;
    }

    let has_padding = bytes[0] & 0x20 != 0;
    let has_extension = bytes[0] & 0x10 != 0;
    let sources = (bytes[0] & 0x0f) as usize;

    let mut at = HEADER + sources * 4;

    if has_extension {
        // A header extension is a word of its own, then its length in words.
        let length = at + 4;

        if length > bytes.len() {
            return None;
        }

        let words = u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]) as usize;

        at = length + words * 4;
    }

    let mut end = bytes.len();

    if has_padding {
        // The last byte says how much of the tail is padding, itself included.
        let padding = *bytes.last()? as usize;

        if padding == 0 || padding > end.saturating_sub(at) {
            return None;
        }

        end -= padding;
    }

    if at > end {
        return None;
    }

    Some(Packet {
        payload_type: bytes[1] & 0x7f,
        sequence: u16::from_be_bytes([bytes[2], bytes[3]]),
        timestamp: u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        marker: bytes[1] & 0x80 != 0,
        payload: &bytes[at..end],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::rtp;

    #[test]
    fn a_packet_is_read_down_to_its_payload() {
        let bytes = rtp(96, 7, 9000, true, &[1, 2, 3]);
        let packet = parse(&bytes).expect("a packet");

        assert_eq!(packet.payload_type, 96);
        assert_eq!(packet.sequence, 7);
        assert_eq!(packet.timestamp, 9000);
        assert!(packet.marker);
        assert_eq!(packet.payload, &[1, 2, 3]);
    }

    #[test]
    fn contributing_sources_are_stepped_over() {
        let mut bytes = rtp(96, 1, 0, false, &[9]);

        // Two sources named, so two words between the header and the payload.
        bytes[0] |= 2;
        bytes.splice(HEADER..HEADER, [0u8; 8]);

        assert_eq!(parse(&bytes).expect("a packet").payload, &[9]);
    }

    #[test]
    fn a_header_extension_is_stepped_over() {
        let mut bytes = rtp(96, 1, 0, false, &[9]);

        bytes[0] |= 0x10;
        bytes.splice(HEADER..HEADER, [0xbe, 0xde, 0, 1, 0, 0, 0, 0]);

        assert_eq!(parse(&bytes).expect("a packet").payload, &[9]);
    }

    #[test]
    fn padding_is_left_out_of_the_payload() {
        let mut bytes = rtp(96, 1, 0, false, &[9]);

        bytes[0] |= 0x20;
        bytes.extend_from_slice(&[0, 0, 3]);

        assert_eq!(parse(&bytes).expect("a packet").payload, &[9]);
    }

    #[test]
    fn what_is_not_a_packet_is_refused() {
        assert!(parse(&[]).is_none());
        assert!(parse(&[0x80, 96, 0, 1]).is_none(), "a header cut short");

        let mut version = rtp(96, 1, 0, false, &[9]);
        version[0] = 0x40;
        assert!(parse(&version).is_none(), "not version two");

        let mut padding = rtp(96, 1, 0, false, &[9]);
        padding[0] |= 0x20;
        padding.push(99);
        assert!(parse(&padding).is_none(), "more padding than packet");

        let mut extension = rtp(96, 1, 0, false, &[9]);
        extension[0] |= 0x10;
        assert!(
            parse(&extension).is_none(),
            "an extension that is not there"
        );
    }
}
