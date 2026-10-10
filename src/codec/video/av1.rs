use bytes::Bytes;

/// What an AV1 track declares up front.
///
/// `config_obus` is the sequence header as the file stores it; put it in front
/// of the samples and the result is a stream a decoder can start on.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Parameters {
    pub profile: u8,
    pub level: u8,
    pub config_obus: Bytes,
}
