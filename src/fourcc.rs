use core::fmt;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid fourcc length")]
    InvalidLength,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FourCC([u8; 4]);

impl FourCC {
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("????")
    }

    pub const fn from(value: [u8; 4]) -> Self {
        Self([value[0], value[1], value[2], value[3]])
    }
}

impl TryFrom<&[u8]> for FourCC {
    type Error = Error;

    fn try_from(value: &[u8]) -> Result<Self> {
        if value.len() != 4 {
            return Err(Error::InvalidLength);
        }

        Ok(Self([value[0], value[1], value[2], value[3]]))
    }
}

impl fmt::Display for FourCC {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_as_the_four_characters_it_holds() {
        assert_eq!(FourCC::from(*b"avc1").as_str(), "avc1");
        assert_eq!(FourCC::from(*b"av01").to_string(), "av01");
    }

    #[test]
    fn takes_four_bytes_and_no_other_count() {
        assert_eq!(FourCC::try_from(&b"moov"[..]).unwrap(), FourCC::from(*b"moov"));
        assert!(FourCC::try_from(&b"moo"[..]).is_err());
        assert!(FourCC::try_from(&b"moovv"[..]).is_err());
    }

    #[test]
    fn says_so_rather_than_panicking_on_bytes_that_are_not_text() {
        assert_eq!(FourCC::from([0xff, 0xfe, 0xfd, 0xfc]).as_str(), "????");
    }
}
