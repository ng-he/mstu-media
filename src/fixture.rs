//! Media built byte by byte, for tests.
//!
//! Enabled by the `fixture` feature, so a plugin's tests can build the same
//! units and packets this crate's own tests do rather than each writing their
//! own parameter sets out by hand.

/// A real SPS and PPS, as a camera offers them in SDP.
pub const SPS: &[u8] = &[
    0x67, 0x42, 0x00, 0x29, 0xe2, 0x90, 0x16, 0xc4, 0x58, 0x0b, 0x72, 0xd0,
];

pub const PPS: &[u8] = &[0x68, 0xce, 0x3c, 0x80];

/// A non-IDR slice, nothing a decoder may start at.
pub fn slice(payload: &[u8]) -> Vec<u8> {
    let mut unit = vec![0x41];
    unit.extend_from_slice(payload);
    unit
}

/// An IDR slice, which is a place to start.
pub fn idr(payload: &[u8]) -> Vec<u8> {
    let mut unit = vec![0x65];
    unit.extend_from_slice(payload);
    unit
}

/// The same, for the two byte headers of H.265.
pub mod h265 {
    pub const VPS: &[u8] = &[0x40, 0x01, 0x0c, 0x01, 0xff, 0xff];
    pub const SPS: &[u8] = &[0x42, 0x01, 0x01, 0x01, 0x60];
    pub const PPS: &[u8] = &[0x44, 0x01, 0xc0];

    pub fn slice(payload: &[u8]) -> Vec<u8> {
        let mut unit = vec![0x02, 0x01];
        unit.extend_from_slice(payload);
        unit
    }

    /// IDR_W_RADL, type 19.
    pub fn idr(payload: &[u8]) -> Vec<u8> {
        let mut unit = vec![0x26, 0x01];
        unit.extend_from_slice(payload);
        unit
    }
}

/// An RTP packet around `payload`.
pub fn rtp(
    payload_type: u8,
    sequence: u16,
    timestamp: u32,
    marker: bool,
    payload: &[u8],
) -> Vec<u8> {
    let mut packet = vec![0x80, payload_type | if marker { 0x80 } else { 0 }];

    packet.extend_from_slice(&sequence.to_be_bytes());
    packet.extend_from_slice(&timestamp.to_be_bytes());
    packet.extend_from_slice(&0x0bad_c0deu32.to_be_bytes());
    packet.extend_from_slice(payload);

    packet
}

/// The description an H.264 sender offers: one video track, with its
/// parameter sets in `sprop-parameter-sets`.
pub fn description() -> String {
    let sets = format!(
        "{},{}",
        crate::base64::encode(SPS),
        crate::base64::encode(PPS)
    );

    format!(
        "v=0\r\n\
         o=- 0 0 IN IP4 127.0.0.1\r\n\
         s=Fixture\r\n\
         t=0 0\r\n\
         a=control:*\r\n\
         m=video 0 RTP/AVP 96\r\n\
         a=rtpmap:96 H264/90000\r\n\
         a=fmtp:96 packetization-mode=1;sprop-parameter-sets={sets}\r\n\
         a=control:trackID=1\r\n"
    )
}
