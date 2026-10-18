use bytes::Bytes;

use crate::nalu;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Profile {
    Unknown(u8, bool),
    ConstrainedBaseline,
    Baseline,
    Main,
    Extended,
    High,
}

impl From<(u8, u8)> for Profile {
    fn from(value: (u8, u8)) -> Profile {
        let profile: u8 = value.0;
        let constrained = value.1 & 0x40 != 0;
        match (profile, constrained) {
            (66, true) => Profile::ConstrainedBaseline,
            (66, false) => Profile::Baseline,
            (77, _) => Profile::Main,
            (88, _) => Profile::Extended,
            (100, _) => Profile::High,
            _ => Profile::Unknown(profile, constrained),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NaluType {
    Unknown(u8),
    Slice = 1,
    Idr = 5,
    Sei = 6,
    Sps = 7,
    Pps = 8,
    Aud = 9,
}

impl From<u8> for NaluType {
    fn from(v: u8) -> Self {
        match v {
            1 => Self::Slice,
            5 => Self::Idr,
            6 => Self::Sei,
            7 => Self::Sps,
            8 => Self::Pps,
            9 => Self::Aud,
            _ => Self::Unknown(v),
        }
    }
}

/// What a track declares up front: its parameter sets, as the Annex-B blob
/// that travels with the stream.
///
/// Kept as it arrived rather than taken apart, so what goes downstream is
/// byte for byte what came in -- the order the sender chose, and a second set
/// of a kind where there is one. The parts are read back out of it on ask.
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

    /// The blob, for handing straight on.
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

    pub fn of_type(&self, kind: NaluType) -> impl Iterator<Item = Bytes> + '_ {
        self.units().filter(move |unit| {
            unit.first().map(|header| NaluType::from(header & 0x1f)) == Some(kind)
        })
    }

    /// The first sequence parameter set, which is the one a decoder reads.
    pub fn sps(&self) -> Option<Bytes> {
        self.of_type(NaluType::Sps).next()
    }

    pub fn pps(&self) -> Option<Bytes> {
        self.of_type(NaluType::Pps).next()
    }

    /// The profile the sequence parameter set declares, which is where it is
    /// written whether the stream came from a file or off a wire.
    pub fn profile(&self) -> Option<Profile> {
        let sps = self.sps()?;

        Some(Profile::from((*sps.get(1)?, *sps.get(2)?)))
    }
}

/// Whether a decoder can start at a unit of this type, which in this codec
/// means an IDR and nothing else.
pub fn starts_a_decoder(nalu: u8) -> bool {
    NaluType::from(nalu) == NaluType::Idr
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parameters() -> Parameters {
        Parameters::from_units([crate::fixture::SPS, crate::fixture::PPS])
    }

    #[test]
    fn the_sets_are_held_as_the_blob_that_travels() {
        let params = parameters();
        let annexb = params.as_bytes();

        assert_eq!(annexb[..4], [0, 0, 0, 1]);
        assert_eq!(
            annexb.len(),
            crate::fixture::SPS.len() + crate::fixture::PPS.len() + 8
        );
        assert_eq!(
            Parameters::new(annexb.to_vec()),
            params,
            "the blob is the whole of it, so it rebuilds from itself"
        );
    }

    #[test]
    fn the_parts_are_read_back_out_of_it() {
        let params = parameters();

        assert_eq!(params.units().count(), 2);
        assert_eq!(params.sps().as_deref(), Some(crate::fixture::SPS));
        assert_eq!(params.pps().as_deref(), Some(crate::fixture::PPS));
    }

    /// A camera may send two of a kind, and the one that arrived first is the
    /// one a decoder reads; neither is thrown away.
    #[test]
    fn a_second_set_of_a_kind_is_kept_beside_the_first() {
        let other = [0x67u8, 0x4d, 0x00, 0x1f];
        let params = Parameters::from_units([crate::fixture::SPS, &other, crate::fixture::PPS]);

        assert_eq!(params.of_type(NaluType::Sps).count(), 2);
        assert_eq!(params.sps().as_deref(), Some(crate::fixture::SPS));
    }

    #[test]
    fn the_profile_is_read_out_of_the_sequence_parameter_set() {
        assert_eq!(parameters().profile(), Some(Profile::Baseline));

        // High profile, as a camera at any size announces itself.
        let high = [0x67u8, 100, 0x00, 0x1f];

        assert_eq!(
            Parameters::from_units([high.as_slice()]).profile(),
            Some(Profile::High)
        );
    }

    #[test]
    fn nothing_declared_is_nothing_read() {
        let nothing = Parameters::default();

        assert!(nothing.is_empty());
        assert_eq!(nothing.units().count(), 0);
        assert_eq!(nothing.sps(), None);
        assert_eq!(nothing.profile(), None);
    }
}
