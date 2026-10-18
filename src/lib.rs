pub mod codec;
pub mod fourcc;
pub mod nalu;
pub mod rtp;
pub mod sdp;

/// Just enough base64 for SDP's parameter sets, out in the open because
/// whatever reads them tends to have a login to encode as well.
pub mod base64;

#[cfg(any(test, feature = "fixture"))]
pub mod fixture;

pub use codec::Codec;
pub use codec::MediaType;
pub use fourcc::FourCC;
pub use nalu::Nalu;
