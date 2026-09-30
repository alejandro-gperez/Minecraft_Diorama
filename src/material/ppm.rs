use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use crate::color::Color;

use super::Texture;

#[derive(Debug)]
pub enum PpmLoadError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    InvalidFile {
        path: PathBuf,
        source: Box<PpmLoadError>,
    },
    InvalidMagic,
    TruncatedHeader(&'static str),
    MalformedHeader(&'static str),
    InvalidDimensions,
    UnsupportedMaxValue(u32),
    TruncatedPayload {
        expected: usize,
        actual: usize,
    },
    UnexpectedPayloadSize {
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for PpmLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "failed to read PPM {}: {source}", path.display())
            }
            Self::InvalidFile { path, source } => {
                write!(formatter, "invalid PPM {}: {source}", path.display())
            }
            Self::InvalidMagic => write!(formatter, "unsupported PPM magic; expected binary P6"),
            Self::TruncatedHeader(field) => write!(formatter, "truncated PPM header at {field}"),
            Self::MalformedHeader(field) => {
                write!(formatter, "malformed PPM header field: {field}")
            }
            Self::InvalidDimensions => write!(formatter, "invalid or overflowing PPM dimensions"),
            Self::UnsupportedMaxValue(value) => {
                write!(
                    formatter,
                    "unsupported PPM maximum channel value {value}; expected 255"
                )
            }
            Self::TruncatedPayload { expected, actual } => write!(
                formatter,
                "truncated PPM payload: expected {expected} bytes, found {actual}"
            ),
            Self::UnexpectedPayloadSize { expected, actual } => write!(
                formatter,
                "invalid PPM payload size: expected {expected} bytes, found {actual}"
            ),
        }
    }
}

impl Error for PpmLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::InvalidFile { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn load_ppm(path: &Path) -> Result<Texture, PpmLoadError> {
    let bytes = fs::read(path).map_err(|source| PpmLoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse_ppm(&bytes).map_err(|source| PpmLoadError::InvalidFile {
        path: path.to_path_buf(),
        source: Box::new(source),
    })
}

pub fn parse_ppm(bytes: &[u8]) -> Result<Texture, PpmLoadError> {
    let mut header = HeaderReader::new(bytes);
    let magic = header
        .next_token()
        .ok_or(PpmLoadError::TruncatedHeader("magic"))?;
    if magic != b"P6" {
        return Err(PpmLoadError::InvalidMagic);
    }

    let width = parse_usize(
        header
            .next_token()
            .ok_or(PpmLoadError::TruncatedHeader("width"))?,
        "width",
    )?;
    let height = parse_usize(
        header
            .next_token()
            .ok_or(PpmLoadError::TruncatedHeader("height"))?,
        "height",
    )?;
    if width == 0 || height == 0 {
        return Err(PpmLoadError::InvalidDimensions);
    }

    let maximum = parse_u32(
        header
            .next_token()
            .ok_or(PpmLoadError::TruncatedHeader("maximum channel value"))?,
        "maximum channel value",
    )?;
    if maximum != 255 {
        return Err(PpmLoadError::UnsupportedMaxValue(maximum));
    }

    let payload = header.payload()?;
    let expected = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or(PpmLoadError::InvalidDimensions)?;
    if payload.len() < expected {
        return Err(PpmLoadError::TruncatedPayload {
            expected,
            actual: payload.len(),
        });
    }
    if payload.len() > expected {
        return Err(PpmLoadError::UnexpectedPayloadSize {
            expected,
            actual: payload.len(),
        });
    }

    let texels = payload
        .chunks_exact(3)
        .map(|rgb| {
            Color::new(
                rgb[0] as f32 / 255.0,
                rgb[1] as f32 / 255.0,
                rgb[2] as f32 / 255.0,
            )
        })
        .collect();

    Texture::try_new(width, height, texels).ok_or(PpmLoadError::InvalidDimensions)
}

struct HeaderReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> HeaderReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn next_token(&mut self) -> Option<&'a [u8]> {
        self.skip_whitespace_and_comments();
        let start = self.position;
        while self.position < self.bytes.len()
            && !self.bytes[self.position].is_ascii_whitespace()
            && self.bytes[self.position] != b'#'
        {
            self.position += 1;
        }
        (start < self.position).then_some(&self.bytes[start..self.position])
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            while self.position < self.bytes.len()
                && self.bytes[self.position].is_ascii_whitespace()
            {
                self.position += 1;
            }
            if self.position >= self.bytes.len() || self.bytes[self.position] != b'#' {
                return;
            }
            while self.position < self.bytes.len() && self.bytes[self.position] != b'\n' {
                self.position += 1;
            }
        }
    }

    fn payload(&mut self) -> Result<&'a [u8], PpmLoadError> {
        let separator = *self
            .bytes
            .get(self.position)
            .ok_or(PpmLoadError::TruncatedHeader("pixel-data separator"))?;
        if !separator.is_ascii_whitespace() {
            return Err(PpmLoadError::MalformedHeader("pixel-data separator"));
        }
        self.position += 1;
        if separator == b'\r' && self.bytes.get(self.position) == Some(&b'\n') {
            self.position += 1;
        }
        Ok(&self.bytes[self.position..])
    }
}

fn parse_usize(token: &[u8], field: &'static str) -> Result<usize, PpmLoadError> {
    parse_ascii(token, field)?
        .parse()
        .map_err(|_| PpmLoadError::InvalidDimensions)
}

fn parse_u32(token: &[u8], field: &'static str) -> Result<u32, PpmLoadError> {
    parse_ascii(token, field)?
        .parse()
        .map_err(|_| PpmLoadError::MalformedHeader(field))
}

fn parse_ascii<'a>(token: &'a [u8], field: &'static str) -> Result<&'a str, PpmLoadError> {
    std::str::from_utf8(token).map_err(|_| PpmLoadError::MalformedHeader(field))
}

#[cfg(test)]
mod tests {
    use std::{io::ErrorKind, path::Path};

    use super::{PpmLoadError, load_ppm, parse_ppm};
    use crate::color::Color;

    #[test]
    fn parses_valid_p6_with_exact_row_major_colors() {
        let texture = parse_ppm(b"P6\n2 2\n255\n\xff\0\0\0\xff\0\0\0\xff\xff\xff\xff").unwrap();

        assert_eq!(texture.width(), 2);
        assert_eq!(texture.height(), 2);
        assert_eq!(texture.texel(0, 0), Some(Color::new(1.0, 0.0, 0.0)));
        assert_eq!(texture.texel(1, 0), Some(Color::new(0.0, 1.0, 0.0)));
        assert_eq!(texture.texel(0, 1), Some(Color::new(0.0, 0.0, 1.0)));
        assert_eq!(texture.texel(1, 1), Some(Color::WHITE));
    }

    #[test]
    fn rejects_invalid_magic() {
        assert!(matches!(
            parse_ppm(b"P3\n1 1\n255\n\0\0\0"),
            Err(PpmLoadError::InvalidMagic)
        ));
    }

    #[test]
    fn rejects_invalid_or_overflowing_dimensions() {
        assert!(matches!(
            parse_ppm(b"P6\n0 1\n255\n"),
            Err(PpmLoadError::InvalidDimensions)
        ));
        assert!(matches!(
            parse_ppm(b"P6\n18446744073709551615 18446744073709551615\n255\n"),
            Err(PpmLoadError::InvalidDimensions)
        ));
    }

    #[test]
    fn rejects_unsupported_maximum_channel_value() {
        assert!(matches!(
            parse_ppm(b"P6\n1 1\n100\n\0\0\0"),
            Err(PpmLoadError::UnsupportedMaxValue(100))
        ));
    }

    #[test]
    fn rejects_truncated_or_malformed_header() {
        assert!(matches!(
            parse_ppm(b"P6\n2"),
            Err(PpmLoadError::TruncatedHeader("height"))
        ));
        assert!(matches!(
            parse_ppm(b"P6\nnope 1\n255\n"),
            Err(PpmLoadError::InvalidDimensions)
        ));
    }

    #[test]
    fn rejects_truncated_and_oversized_payloads() {
        assert!(matches!(
            parse_ppm(b"P6\n1 1\n255\n\0\0"),
            Err(PpmLoadError::TruncatedPayload {
                expected: 3,
                actual: 2
            })
        ));
        assert!(matches!(
            parse_ppm(b"P6\n1 1\n255\n\0\0\0\0"),
            Err(PpmLoadError::UnexpectedPayloadSize {
                expected: 3,
                actual: 4
            })
        ));
    }

    #[test]
    fn missing_file_error_retains_path_and_io_cause() {
        let path = Path::new("assets/textures/does_not_exist.ppm");
        let error = load_ppm(path).unwrap_err();

        match error {
            PpmLoadError::Io {
                path: actual,
                source,
            } => {
                assert_eq!(actual, path);
                assert_eq!(source.kind(), ErrorKind::NotFound);
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn generated_runtime_assets_are_all_16_by_16() {
        let texture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/textures");
        for filename in [
            "grass_top.ppm",
            "grass_side.ppm",
            "dirt.ppm",
            "cobblestone.ppm",
            "obsidian.ppm",
            "glass.ppm",
            "lava.ppm",
            "coal_ore.ppm",
            "iron_ore.ppm",
            "gold_ore.ppm",
            "diamond_ore.ppm",
        ] {
            let texture = load_ppm(&texture_dir.join(filename)).unwrap();
            assert_eq!((texture.width(), texture.height()), (16, 16), "{filename}");
        }
    }
}
