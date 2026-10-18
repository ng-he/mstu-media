use std::matches;

use bytes::Bytes;

use crate::nalu;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NaluType {
    Unknown(u8),

    // The random access units, which a decoder may start at.
    BlaWLp = 16,
    BlaWRadl = 17,
    BlaNLp = 18,
    IdrWRadl = 19,
    IdrNLp = 20,
    CraNut = 21,

    Vps = 32,
    Sps = 33,
    Pps = 34,
    Aud = 35,

    PrefixSei = 39,
    SuffixSei = 40,
}

impl From<u8> for NaluType {
    fn from(v: u8) -> Self {
        match v {
            16 => Self::BlaWLp,
            17 => Self::BlaWRadl,
            18 => Self::BlaNLp,
            19 => Self::IdrWRadl,
            20 => Self::IdrNLp,
            21 => Self::CraNut,
            32 => Self::Vps,
            33 => Self::Sps,
            34 => Self::Pps,
            35 => Self::Aud,
            39 => Self::PrefixSei,
            40 => Self::SuffixSei,
            _ => Self::Unknown(v),
        }
    }
}

/// What a track declares up front: its parameter sets, as the Annex-B blob
/// that travels with the stream.
///
/// Kept as it arrived rather than taken apart, the same as H.264's; a decoder
/// wants the video, sequence and picture sets in the order the sender put
/// them in.
#[derive(Debug, PartialEq, Eq, Clone, Default)]
pub struct Parameters {
    annexb: Bytes,
}

impl Parameters {
    /// Takes an Annex-B blob as it stands, which is what `extras` carries.
    pub fn new(annexb: impl Into<Bytes>) -> Self {
        Self {
            annexb: annexb.into(),
        }
    }

    /// Builds the blob from loose units, as a file's sample description holds
    /// them.
    pub fn from_units<'a>(units: impl IntoIterator<Item = &'a [u8]>) -> Self {
        Self::new(nalu::annexb_of(units))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.annexb
    }

    pub fn is_empty(&self) -> bool {
        self.annexb.is_empty()
    }

    /// Every unit in it, sharing the bytes rather than copying them.
    pub fn units(&self) -> impl Iterator<Item = Bytes> + '_ {
        nalu::annexb_ranges(&self.annexb)
            .into_iter()
            .map(|range| self.annexb.slice(range))
    }

    /// The header is two bytes here, and the type is in the first of them.
    pub fn of_type(&self, kind: NaluType) -> impl Iterator<Item = Bytes> + '_ {
        self.units().filter(move |unit| {
            unit.first()
                .map(|header| NaluType::from((header >> 1) & 0x3f))
                == Some(kind)
        })
    }

    pub fn vps(&self) -> Option<Bytes> {
        self.of_type(NaluType::Vps).next()
    }

    pub fn sps(&self) -> Option<Bytes> {
        self.of_type(NaluType::Sps).next()
    }

    pub fn pps(&self) -> Option<Bytes> {
        self.of_type(NaluType::Pps).next()
    }
}

/// Whether a decoder can start at a unit of this type: the random access
/// units, from a broken link access up to a clean random access.
pub fn starts_a_decoder(nalu: u8) -> bool {
    matches!(
        NaluType::from(nalu),
        NaluType::BlaWLp
            | NaluType::BlaWRadl
            | NaluType::BlaNLp
            | NaluType::IdrWRadl
            | NaluType::IdrNLp
            | NaluType::CraNut
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::h265::{PPS, SPS, VPS};

    #[test]
    fn the_three_sets_are_held_in_the_order_they_came() {
        let params = Parameters::from_units([VPS, SPS, PPS]);

        assert_eq!(params.units().count(), 3);
        assert_eq!(params.vps().as_deref(), Some(VPS), "the video set first");
        assert_eq!(params.sps().as_deref(), Some(SPS));
        assert_eq!(params.pps().as_deref(), Some(PPS));

        assert_eq!(
            Parameters::new(params.as_bytes().to_vec()),
            params,
            "the blob is the whole of it"
        );
    }

    #[test]
    fn a_set_the_sender_left_out_is_not_invented() {
        let params = Parameters::from_units([SPS, PPS]);

        assert_eq!(params.vps(), None);
        assert_eq!(params.units().count(), 2);
    }

    #[test]
    fn nothing_declared_is_nothing_read() {
        let nothing = Parameters::default();

        assert!(nothing.is_empty());
        assert_eq!(nothing.sps(), None);
    }
}
