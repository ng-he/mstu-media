//! The session description a camera answers DESCRIBE with.
//!
//! Only what is needed to set a video track up: which payload type carries
//! it, at what clock, where to address it, and the parameter sets a decoder
//! needs before the first frame.

use crate::base64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Media {
    /// `video`, `audio`, or whatever else the camera offers.
    pub kind: String,

    pub payload_type: u8,

    /// The codec as rtpmap names it, upper case: `H264`, `H265`, `JPEG`.
    pub encoding: String,

    /// Ticks per second of this track's timestamps, 90000 for video.
    pub clock_rate: u32,

    /// Where SETUP addresses this track, as `a=control` gave it.
    pub control: String,

    /// `a=fmtp` parameters, name to value, lower case names.
    pub format: Vec<(String, String)>,
}

impl Media {
    pub fn parameter(&self, name: &str) -> Option<&str> {
        self.format
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// The parameter sets a decoder needs, in the order the camera listed
    /// them: `sprop-parameter-sets` for H.264, the three sprops for H.265.
    pub fn parameter_sets(&self) -> Vec<Vec<u8>> {
        let mut sets = Vec::new();

        for name in [
            "sprop-vps",
            "sprop-sps",
            "sprop-pps",
            "sprop-parameter-sets",
        ] {
            let Some(value) = self.parameter(name) else {
                continue;
            };

            // One parameter may hold several sets, comma separated.
            for encoded in value.split(',') {
                match base64::decode(encoded) {
                    Some(bytes) if !bytes.is_empty() => sets.push(bytes),
                    _ => continue,
                }
            }
        }

        sets
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Description {
    /// `a=control` of the session, before any media line.
    pub control: String,

    pub media: Vec<Media>,
}

impl Description {
    /// The first video track the camera offers, which is the one this plugin
    /// carries.
    pub fn video(&self) -> Option<&Media> {
        self.media.iter().find(|media| media.kind == "video")
    }
}

pub fn parse(text: &str) -> Description {
    let mut description = Description::default();

    for line in text.lines() {
        let line = line.trim_end();

        let Some((kind, value)) = line.split_once('=') else {
            continue;
        };

        match kind {
            "m" => {
                if let Some(media) = media_line(value) {
                    description.media.push(media);
                }
            }

            "a" => attribute(&mut description, value),

            _ => continue,
        }
    }

    description
}

/// `m=video 0 RTP/AVP 96`, whose last field is the payload type.
fn media_line(value: &str) -> Option<Media> {
    let mut fields = value.split_whitespace();

    let kind = fields.next()?.to_ascii_lowercase();
    let _port = fields.next()?;
    let _protocol = fields.next()?;

    // Several formats may be offered; the first is the camera's preference.
    let payload_type = fields.next()?.parse().ok()?;

    Some(Media {
        kind,
        payload_type,
        encoding: String::new(),
        clock_rate: 0,
        control: String::new(),
        format: Vec::new(),
    })
}

fn attribute(description: &mut Description, value: &str) {
    let (name, rest) = match value.split_once(':') {
        Some((name, rest)) => (name, rest.trim()),
        None => (value, ""),
    };

    // An attribute before the first media line belongs to the session.
    let Some(media) = description.media.last_mut() else {
        if name == "control" {
            description.control = rest.to_string();
        }

        return;
    };

    match name {
        "control" => media.control = rest.to_string(),

        // rtpmap:96 H264/90000
        "rtpmap" => {
            let Some((_, codec)) = rest.split_once(' ') else {
                return;
            };

            let mut parts = codec.split('/');

            if let Some(encoding) = parts.next() {
                media.encoding = encoding.trim().to_ascii_uppercase();
            }

            if let Some(rate) = parts.next() {
                media.clock_rate = rate.trim().parse().unwrap_or(0);
            }
        }

        // fmtp:96 packetization-mode=1;sprop-parameter-sets=Z0I...,aM4...
        "fmtp" => {
            let Some((_, parameters)) = rest.split_once(' ') else {
                return;
            };

            for parameter in parameters.split(';') {
                let Some((key, value)) = parameter.split_once('=') else {
                    continue;
                };

                media
                    .format
                    .push((key.trim().to_ascii_lowercase(), value.trim().to_string()));
            }
        }

        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What an H.264 camera answers DESCRIBE with, down to the line breaks.
    const H264: &str = "v=0\r\n\
         o=- 1234 1 IN IP4 10.0.0.7\r\n\
         s=Media Presentation\r\n\
         a=control:*\r\n\
         t=0 0\r\n\
         m=video 0 RTP/AVP 96\r\n\
         a=rtpmap:96 H264/90000\r\n\
         a=fmtp:96 packetization-mode=1;profile-level-id=420029;\
         sprop-parameter-sets=Z0IAKeKQFsRYC3LQ,aM48gA==\r\n\
         a=control:trackID=1\r\n\
         m=audio 0 RTP/AVP 97\r\n\
         a=rtpmap:97 MPEG4-GENERIC/16000/1\r\n\
         a=control:trackID=2\r\n";

    #[test]
    fn the_video_track_is_found_among_the_others() {
        let description = parse(H264);

        assert_eq!(description.media.len(), 2);
        assert_eq!(description.control, "*");

        let video = description.video().expect("a video track");

        assert_eq!(video.payload_type, 96);
        assert_eq!(video.encoding, "H264");
        assert_eq!(video.clock_rate, 90000);
        assert_eq!(video.control, "trackID=1");
    }

    #[test]
    fn the_format_parameters_are_read_one_by_one() {
        let video = parse(H264).video().cloned().expect("a video track");

        assert_eq!(video.parameter("packetization-mode"), Some("1"));
        assert_eq!(video.parameter("profile-level-id"), Some("420029"));
        assert_eq!(video.parameter("nothing-like-this"), None);
    }

    #[test]
    fn the_parameter_sets_come_out_as_nal_units() {
        let video = parse(H264).video().cloned().expect("a video track");
        let sets = video.parameter_sets();

        assert_eq!(sets.len(), 2, "an sps and a pps");
        assert_eq!(sets[0][0] & 0x1f, 7, "the sps first");
        assert_eq!(sets[1][0] & 0x1f, 8, "then the pps");
    }

    /// H.265 names its three parameter sets one by one, and a decoder wants
    /// them in that order.
    #[test]
    fn h265_lists_its_three_parameter_sets() {
        let description = parse(
            "m=video 0 RTP/AVP 98\r\n\
             a=rtpmap:98 H265/90000\r\n\
             a=fmtp:98 sprop-vps=QAEMAf//;sprop-sps=QgEB;sprop-pps=RAHA\r\n\
             a=control:track1\r\n",
        );

        let video = description.video().expect("a video track");

        assert_eq!(video.encoding, "H265");

        let sets = video.parameter_sets();

        assert_eq!(sets.len(), 3);
        assert_eq!((sets[0][0] >> 1) & 0x3f, 32, "the vps first");
        assert_eq!((sets[1][0] >> 1) & 0x3f, 33, "then the sps");
        assert_eq!((sets[2][0] >> 1) & 0x3f, 34, "then the pps");
    }

    #[test]
    fn a_description_of_nothing_describes_no_video() {
        assert!(parse("").video().is_none());
        assert!(parse("v=0\r\nm=nonsense\r\n").video().is_none());
    }

    /// Line endings are the camera's business, not the parser's.
    #[test]
    fn plain_newlines_read_the_same_as_the_pair() {
        let plain = parse("m=video 0 RTP/AVP 96\na=rtpmap:96 H264/90000\n");

        assert_eq!(plain.video().expect("a track").encoding, "H264");
    }
}
