use std::ops::Range;

use bytes::Bytes;

pub const START_CODE: [u8; 4] = [0, 0, 0, 1];

/// Rewrites a 4-byte-length-prefixed sample into Annex-B where it lies.
///
/// Same size either way, so this needs no buffer of its own. False on a
/// malformed sample.
pub fn avcc_to_annexb_in_place(data: &mut [u8]) -> bool {
    let mut offset = 0;

    while offset < data.len() {
        let end = offset + START_CODE.len();

        if end > data.len() {
            return false;
        }

        let Ok(prefix) = data[offset..end].try_into() else {
            return false;
        };

        let size = u32::from_be_bytes(prefix) as usize;

        if end + size > data.len() {
            return false;
        }

        data[offset..end].copy_from_slice(&START_CODE);
        offset = end + size;
    }

    true
}

/// Converts AVCC (length prefixed) to Annex-B (start code prefixed).
///
/// `length_size` 4 is rewritten in place, shorter prefixes need a copy.
/// Returns None on a malformed sample.
pub fn avcc_to_annexb(mut data: Vec<u8>, length_size: usize) -> Option<Vec<u8>> {
    if length_size == 0 || length_size > 4 {
        return None;
    }

    if length_size == START_CODE.len() {
        let mut offset = 0;

        while offset < data.len() {
            let end = offset + START_CODE.len();
            if end > data.len() {
                return None;
            }

            let size = u32::from_be_bytes(data[offset..end].try_into().ok()?) as usize;
            if end + size > data.len() {
                return None;
            }

            data[offset..end].copy_from_slice(&START_CODE);
            offset = end + size;
        }

        return Some(data);
    }

    let mut annexb = Vec::with_capacity(data.len());
    let mut offset = 0;

    while offset < data.len() {
        let end = offset + length_size;
        if end > data.len() {
            return None;
        }

        let size = data[offset..end]
            .iter()
            .fold(0usize, |size, byte| (size << 8) | *byte as usize);

        if end + size > data.len() {
            return None;
        }

        annexb.extend_from_slice(&START_CODE);
        annexb.extend_from_slice(&data[end..end + size]);

        offset = end + size;
    }

    Some(annexb)
}

/// Where each unit of an Annex-B stream lies, so whoever holds the bytes can
/// slice them without copying.
///
/// Start codes of three bytes and of four are both read, since a stream may
/// carry either.
pub fn annexb_ranges(data: &[u8]) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut at = 0;

    while at < data.len() {
        let start = match data[at..] {
            [0, 0, 0, 1, ..] => Some(4),
            [0, 0, 1, ..] => Some(3),
            _ => None,
        };

        let Some(code) = start else {
            at += 1;
            continue;
        };

        // The unit before this start code ends where this one begins.
        if let Some(last) = ranges.last_mut() {
            last.end = at;
        }

        at += code;
        ranges.push(at..data.len());
    }

    ranges.retain(|range| !range.is_empty());
    ranges
}

/// An Annex-B stream of `units`, each behind a start code.
pub fn annexb_of<'a>(units: impl IntoIterator<Item = &'a [u8]>) -> Vec<u8> {
    let mut annexb = Vec::new();

    for unit in units {
        if unit.is_empty() {
            continue;
        }

        annexb.extend_from_slice(&START_CODE);
        annexb.extend_from_slice(unit);
    }

    annexb
}

/// Raw Network Abstraction Layer Unit (NALU).
/// This type stores only the raw NAL payload itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nalu(Bytes);

impl From<Vec<u8>> for Nalu {
    fn from(bytes: Vec<u8>) -> Self {
        Self(Bytes::from(bytes))
    }
}

impl From<&[u8]> for Nalu {
    fn from(bytes: &[u8]) -> Self {
        Self(Bytes::copy_from_slice(bytes))
    }
}

impl From<Bytes> for Nalu {
    fn from(bytes: Bytes) -> Self {
        Self(bytes)
    }
}

impl Nalu {
    pub fn size(&self) -> usize {
        self.0.len()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_bytes(self) -> Bytes {
        self.0
    }

    /// The units of an Annex-B stream, each copied out of it.
    ///
    /// `annexb_ranges` says where they are; this is for a caller that wants
    /// them to outlive the bytes they came from.
    pub fn from_annexb(data: &[u8]) -> Vec<Self> {
        annexb_ranges(data)
            .into_iter()
            .map(|range| Self::from(&data[range]))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::vec;

    use super::*;

    #[test]
    fn the_units_of_a_stream_are_found_without_copying_it() {
        let stream = [
            0, 0, 0, 1, 0x67, 0x64, 0, 0, 1, 0x68, 0xee, 0, 0, 0, 1, 0x65, 9,
        ];
        let ranges = annexb_ranges(&stream);

        assert_eq!(ranges.len(), 3, "{ranges:?}");
        assert_eq!(&stream[ranges[0].clone()], &[0x67, 0x64]);
        assert_eq!(
            &stream[ranges[1].clone()],
            &[0x68, 0xee],
            "a three byte code"
        );
        assert_eq!(&stream[ranges[2].clone()], &[0x65, 9]);
    }

    #[test]
    fn a_stream_of_no_units_has_no_ranges() {
        assert!(annexb_ranges(&[]).is_empty());
        assert!(
            annexb_ranges(&[0, 0, 0, 1]).is_empty(),
            "a code and nothing after it"
        );
        assert!(annexb_ranges(&[9, 9, 9]).is_empty(), "no code at all");
    }

    #[test]
    fn units_go_back_into_a_stream_behind_their_start_codes() {
        let annexb = annexb_of([[0x67u8, 1].as_slice(), [0x68, 2].as_slice()]);

        assert_eq!(annexb, [0, 0, 0, 1, 0x67, 1, 0, 0, 0, 1, 0x68, 2]);

        let ranges = annexb_ranges(&annexb);

        assert_eq!(ranges.len(), 2, "and come back out the same way");
        assert_eq!(&annexb[ranges[0].clone()], &[0x67, 1]);
    }

    #[test]
    fn an_empty_unit_is_not_written() {
        assert!(annexb_of([[].as_slice()]).is_empty());
    }

    /// Two units of 2 and 3 bytes, each behind a 4-byte length.
    fn avcc() -> Vec<u8> {
        vec![0, 0, 0, 2, 0x65, 0x88, 0, 0, 0, 3, 0x41, 0x9a, 0x02]
    }

    #[test]
    fn rewrites_every_length_into_a_start_code() {
        let mut data = avcc();

        assert!(avcc_to_annexb_in_place(&mut data));
        assert_eq!(
            data,
            vec![0, 0, 0, 1, 0x65, 0x88, 0, 0, 0, 1, 0x41, 0x9a, 0x02]
        );
    }

    #[test]
    fn refuses_a_length_running_past_the_end() {
        let mut data = vec![0, 0, 0, 9, 0x65, 0x88];

        assert!(!avcc_to_annexb_in_place(&mut data));
    }

    #[test]
    fn refuses_a_sample_too_short_to_hold_a_length() {
        let mut data = vec![0, 0, 1];

        assert!(!avcc_to_annexb_in_place(&mut data));
    }

    #[test]
    fn an_empty_sample_is_nothing_to_rewrite() {
        assert!(avcc_to_annexb_in_place(&mut []));
    }

    #[test]
    fn copies_when_the_prefix_is_shorter_than_a_start_code() {
        // The same two units, behind 2-byte lengths.
        let data = vec![0, 2, 0x65, 0x88, 0, 3, 0x41, 0x9a, 0x02];

        let annexb = avcc_to_annexb(data, 2).expect("well formed");

        assert_eq!(
            annexb,
            vec![0, 0, 0, 1, 0x65, 0x88, 0, 0, 0, 1, 0x41, 0x9a, 0x02]
        );
    }

    #[test]
    fn refuses_a_prefix_length_no_sample_uses() {
        assert!(avcc_to_annexb(vec![0, 0, 0, 1], 0).is_none());
        assert!(avcc_to_annexb(vec![0, 0, 0, 1], 5).is_none());
    }

    #[test]
    fn finds_units_behind_either_start_code() {
        let data = [0, 0, 0, 1, 0x67, 0x42, 0, 0, 1, 0x68, 0xce, 0x3c];

        let nalus = Nalu::from_annexb(&data);

        assert_eq!(nalus.len(), 2);
        assert_eq!(nalus[0].as_bytes(), &[0x67, 0x42]);
        assert_eq!(nalus[1].as_bytes(), &[0x68, 0xce, 0x3c]);
    }

    #[test]
    fn finds_nothing_where_no_start_code_is() {
        assert!(Nalu::from_annexb(&[0x67, 0x42, 0x80]).is_empty());
    }
}
