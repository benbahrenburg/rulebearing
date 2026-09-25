//! A bounds-checked little-endian byte reader: every read returns an error naming the structure
//! it was reading, never a panic.
//!
//! - Architecture: [Security posture](../../../docs/architecture.md#security-posture) (untrusted
//!   input produces exit 2 with a named reason)
//! - Plan: [Wave 0, Step 9](../../../docs/plans/pending/0000-wave-0-spike.md#step-9-spike-b-rb-extract-dotnet-0d)
//! - Specification: ECMA-335 II.23.2 (compressed integers)

use std::fmt;

/// A structure that could not be read, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadError {
    /// What was being read, for example `CLI header`.
    pub what: &'static str,
    /// The byte offset in the file or stream where reading failed.
    pub offset: usize,
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "malformed {} at offset {:#x}", self.what, self.offset)
    }
}

impl std::error::Error for ReadError {}

/// The result of a read.
pub type Read<T> = Result<T, ReadError>;

/// Builds the error for `what` at `offset`.
///
/// # Errors
/// Always: it is the error constructor, shaped as a result so a reader can `return` it.
pub fn malformed<T>(what: &'static str, offset: usize) -> Read<T> {
    Err(ReadError { what, offset })
}

/// A cursor over a byte slice.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    position: usize,
    what: &'static str,
}

impl<'a> Reader<'a> {
    /// A reader at `position`, reporting failures as `what`.
    pub fn new(data: &'a [u8], position: usize, what: &'static str) -> Self {
        Self {
            data,
            position,
            what,
        }
    }

    /// The current offset.
    pub fn position(&self) -> usize {
        self.position
    }

    /// Moves to `position`.
    pub fn seek(&mut self, position: usize) {
        self.position = position;
    }

    fn fail<T>(&self) -> Read<T> {
        malformed(self.what, self.position)
    }

    /// The next `length` bytes.
    ///
    /// # Errors
    /// When fewer than `length` bytes remain.
    pub fn bytes(&mut self, length: usize) -> Read<&'a [u8]> {
        let end = self.position.checked_add(length);
        match end.and_then(|end| self.data.get(self.position..end)) {
            Some(slice) => {
                self.position += length;
                Ok(slice)
            }
            None => self.fail(),
        }
    }

    /// Skips `length` bytes.
    ///
    /// # Errors
    /// When fewer than `length` bytes remain.
    pub fn skip(&mut self, length: usize) -> Read<()> {
        self.bytes(length).map(|_| ())
    }

    /// One byte.
    ///
    /// # Errors
    /// At the end of the data.
    pub fn u8(&mut self) -> Read<u8> {
        Ok(self.bytes(1)?[0])
    }

    /// A little-endian `u16`.
    ///
    /// # Errors
    /// When fewer than two bytes remain.
    pub fn u16(&mut self) -> Read<u16> {
        let b = self.bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    /// A little-endian `u32`.
    ///
    /// # Errors
    /// When fewer than four bytes remain.
    pub fn u32(&mut self) -> Read<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A little-endian `u64`.
    ///
    /// # Errors
    /// When fewer than eight bytes remain.
    pub fn u64(&mut self) -> Read<u64> {
        let b = self.bytes(8)?;
        let mut array = [0u8; 8];
        array.copy_from_slice(b);
        Ok(u64::from_le_bytes(array))
    }

    /// A table or heap index that is two or four bytes wide.
    ///
    /// # Errors
    /// When too few bytes remain.
    pub fn index(&mut self, wide: bool) -> Read<u32> {
        if wide {
            self.u32()
        } else {
            self.u16().map(u32::from)
        }
    }

    /// An ECMA-335 compressed unsigned integer (II.23.2): one, two or four bytes.
    ///
    /// # Errors
    /// When the data ends first or the lead byte is invalid.
    pub fn compressed_u32(&mut self) -> Read<u32> {
        let start = self.position;
        let first = self.u8()?;
        if first & 0x80 == 0 {
            return Ok(u32::from(first));
        }
        if first & 0xC0 == 0x80 {
            let second = self.u8()?;
            return Ok((u32::from(first & 0x3F) << 8) | u32::from(second));
        }
        if first & 0xE0 == 0xC0 {
            let rest = self.bytes(3)?;
            return Ok((u32::from(first & 0x1F) << 24)
                | (u32::from(rest[0]) << 16)
                | (u32::from(rest[1]) << 8)
                | u32::from(rest[2]));
        }
        malformed(self.what, start)
    }

    /// An ECMA-335 compressed signed integer (II.23.2): the unsigned encoding with the sign in the
    /// lowest bit, rotated.
    ///
    /// # Errors
    /// When the data ends first or the lead byte is invalid.
    pub fn compressed_i32(&mut self) -> Read<i32> {
        let start = self.position;
        let width = match self.data.get(start) {
            Some(b) if b & 0x80 == 0 => 7,
            Some(b) if b & 0xC0 == 0x80 => 14,
            Some(_) => 29,
            None => return self.fail(),
        };
        let raw = self.compressed_u32()?;
        let magnitude = raw >> 1;
        #[allow(clippy::cast_possible_wrap)] // magnitude < 2^28, fits
        let value = if raw & 1 == 0 {
            magnitude as i32
        } else {
            (magnitude as i32) - (1i32 << (width - 1))
        };
        Ok(value)
    }

    /// Bytes up to a NUL, as UTF-8, consuming the NUL.
    ///
    /// # Errors
    /// When no NUL follows or the bytes are not UTF-8.
    pub fn c_string(&mut self) -> Read<&'a str> {
        let rest = self.data.get(self.position..).unwrap_or_default();
        let Some(length) = rest.iter().position(|&b| b == 0) else {
            return self.fail();
        };
        let text = std::str::from_utf8(&rest[..length]).or_else(|_| self.fail())?;
        self.position += length + 1;
        Ok(text)
    }
}

/// Rounds `value` up to a multiple of four.
pub fn align4(value: usize) -> Option<usize> {
    value.checked_add(3).map(|v| v & !3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn reads_little_endian_integers() {
        let data = [1, 0, 2, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0];
        let mut r = Reader::new(&data, 0, "test");
        assert_eq!(r.u16(), Ok(1));
        assert_eq!(r.u32(), Ok(2));
        assert_eq!(r.u64(), Ok(3));
        assert_eq!(
            r.u8(),
            Err(ReadError {
                what: "test",
                offset: 14
            })
        );
    }

    #[test]
    fn compressed_unsigned_matches_the_spec_examples() {
        // ECMA-335 II.23.2 table of examples.
        for (bytes, value) in [
            (&[0x03][..], 0x03),
            (&[0x7F][..], 0x7F),
            (&[0x80, 0x80][..], 0x80),
            (&[0xAE, 0x57][..], 0x2E57),
            (&[0xBF, 0xFF][..], 0x3FFF),
            (&[0xC0, 0x00, 0x40, 0x00][..], 0x4000),
            (&[0xDF, 0xFF, 0xFF, 0xFF][..], 0x1FFF_FFFF),
        ] {
            assert_eq!(
                Reader::new(bytes, 0, "c").compressed_u32(),
                Ok(value),
                "{bytes:x?}"
            );
        }
        assert!(Reader::new(&[0xE0], 0, "c").compressed_u32().is_err());
        assert!(Reader::new(&[0x80], 0, "c").compressed_u32().is_err());
    }

    #[test]
    fn compressed_signed_matches_the_spec_examples() {
        for (bytes, value) in [
            (&[0x06][..], 3),
            (&[0x7B][..], -3),
            (&[0x80, 0x80][..], 64),
            (&[0x01][..], -64),
            (&[0xC0, 0x00, 0x40, 0x00][..], 8192),
            (&[0x80, 0x01][..], -8192),
            (&[0xDF, 0xFF, 0xFF, 0xFE][..], 268_435_455),
            (&[0xC0, 0x00, 0x00, 0x01][..], -268_435_456),
        ] {
            assert_eq!(
                Reader::new(bytes, 0, "c").compressed_i32(),
                Ok(value),
                "{bytes:x?}"
            );
        }
        assert!(Reader::new(&[], 0, "c").compressed_i32().is_err());
    }

    #[test]
    fn strings_indices_and_alignment() {
        let data = b"abc\0rest";
        let mut r = Reader::new(data, 0, "s");
        assert_eq!(r.c_string(), Ok("abc"));
        assert_eq!(r.position(), 4);
        assert!(Reader::new(b"no nul", 0, "s").c_string().is_err());
        assert!(Reader::new(&[0xFF, 0], 0, "s").c_string().is_err());
        let mut r = Reader::new(&[1, 0, 2, 0, 0, 0], 0, "i");
        assert_eq!(r.index(false), Ok(1));
        assert_eq!(r.index(true), Ok(2));
        assert_eq!(align4(5), Some(8));
        assert_eq!(align4(8), Some(8));
        assert_eq!(align4(usize::MAX), None);
        let mut r = Reader::new(&[0; 4], 0, "k");
        assert_eq!(r.skip(3), Ok(()));
        r.seek(1);
        assert_eq!(r.bytes(3).map(<[u8]>::len), Ok(3));
        assert!(r.skip(1).is_err());
        assert_eq!(
            ReadError {
                what: "PE header",
                offset: 16
            }
            .to_string(),
            "malformed PE header at offset 0x10"
        );
    }

    proptest! {
        #[test]
        fn compressed_round_trips(value in 0u32..0x2000_0000) {
            let bytes = if value < 0x80 {
                vec![u8::try_from(value).unwrap_or(0)]
            } else if value < 0x4000 {
                vec![0x80 | u8::try_from(value >> 8).unwrap_or(0), u8::try_from(value & 0xFF).unwrap_or(0)]
            } else {
                let b = value.to_be_bytes();
                vec![0xC0 | b[0], b[1], b[2], b[3]]
            };
            prop_assert_eq!(Reader::new(&bytes, 0, "p").compressed_u32(), Ok(value));
        }

        #[test]
        fn reading_arbitrary_bytes_never_panics(data in proptest::collection::vec(any::<u8>(), 0..64)) {
            let mut r = Reader::new(&data, 0, "p");
            while r.compressed_u32().is_ok() {}
            let mut r = Reader::new(&data, 0, "p");
            while r.compressed_i32().is_ok() {}
            let _ = Reader::new(&data, 0, "p").c_string();
        }
    }
}
