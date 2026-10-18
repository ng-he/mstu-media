//! Just enough base64 for RTSP: parameter sets out of SDP, credentials in.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let mut word = 0u32;

        for (at, byte) in chunk.iter().enumerate() {
            word |= (*byte as u32) << (16 - 8 * at);
        }

        for at in 0..4 {
            // Three bytes are four characters; a short group is padded out.
            if at <= chunk.len() {
                text.push(ALPHABET[(word >> (18 - 6 * at)) as usize & 0x3f] as char);
            } else {
                text.push('=');
            }
        }
    }

    text
}

/// None if `text` holds anything that is not base64.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(text.len() / 4 * 3);
    let mut word = 0u32;
    let mut bits = 0;

    for character in text.bytes() {
        // Padding only ever ends the text, and spaces are the wrapping of a
        // long SDP line.
        if character == b'=' {
            break;
        }

        if character.is_ascii_whitespace() {
            continue;
        }

        let six = ALPHABET.iter().position(|allowed| *allowed == character)? as u32;

        word = (word << 6) | six;
        bits += 6;

        if bits >= 8 {
            bits -= 8;
            bytes.push((word >> bits) as u8);
        }
    }

    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The examples from RFC 4648, which cover every padding case.
    #[test]
    fn the_examples_from_the_rfc_round_trip() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(plain.as_bytes()), encoded, "encoding {plain:?}");
            assert_eq!(
                decode(encoded).as_deref(),
                Some(plain.as_bytes()),
                "decoding {encoded:?}"
            );
        }
    }

    /// An SPS as a camera writes it into sprop-parameter-sets.
    #[test]
    fn a_parameter_set_decodes_to_its_bytes() {
        let sps = decode("Z0IAKeKQFsRYC3LQ").expect("base64");

        assert_eq!(sps[0] & 0x1f, 7, "the nal type of an sps");
        assert_eq!(encode(&sps), "Z0IAKeKQFsRYC3LQ");
    }

    #[test]
    fn what_is_not_base64_is_refused() {
        assert!(decode("not base64!").is_none());
        assert!(decode("Zm9v\n").is_some(), "a wrapped line is still base64");
    }
}
