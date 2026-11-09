//! Save state serialization and deserialization.
//!
//! Captures the complete architectural state of the Game Boy Color, allowing
//! an execution session to be frozen into bytes and restored byte-for-byte.

use std::fmt;

const MAGIC: &[u8; 10] = b"GBCE_STATE";
const STATE_VERSION: u32 = 1;

/// Errors that can occur when restoring a save state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveStateError {
    /// The state header does not match the expected magic bytes.
    InvalidMagic,
    /// The save state format version is not supported by this build.
    UnsupportedVersion(u32),
    /// The byte slice ended prematurely while reading fields.
    TruncatedData,
    /// An encoded enum or field value is out of valid range.
    InvalidData(&'static str),
}

impl fmt::Display for SaveStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => write!(f, "invalid save state magic header"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported save state version {v}"),
            Self::TruncatedData => write!(f, "save state data ended prematurely"),
            Self::InvalidData(msg) => write!(f, "corrupted save state: {msg}"),
        }
    }
}

impl std::error::Error for SaveStateError {}

/// Binary writer for packing console state into a contiguous byte buffer.
pub(crate) struct StateWriter {
    buf: Vec<u8>,
}

impl StateWriter {
    pub(crate) fn new() -> Self {
        let mut s = Self {
            buf: Vec::with_capacity(160 * 1024),
        };
        s.buf.extend_from_slice(MAGIC);
        s.write_u32(STATE_VERSION);
        s
    }

    pub(crate) fn write_u8(&mut self, val: u8) {
        self.buf.push(val);
    }

    pub(crate) fn write_bool(&mut self, val: bool) {
        self.buf.push(if val { 1 } else { 0 });
    }

    pub(crate) fn write_u16(&mut self, val: u16) {
        self.buf.extend_from_slice(&val.to_le_bytes());
    }

    pub(crate) fn write_u32(&mut self, val: u32) {
        self.buf.extend_from_slice(&val.to_le_bytes());
    }

    pub(crate) fn write_raw(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    pub(crate) fn write_bytes(&mut self, bytes: &[u8]) {
        self.write_u32(bytes.len() as u32);
        self.buf.extend_from_slice(bytes);
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.buf
    }
}

/// Binary reader for extracting console state from a serialized byte buffer.
pub(crate) struct StateReader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> StateReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Result<Self, SaveStateError> {
        if data.len() < MAGIC.len() + 4 {
            return Err(SaveStateError::TruncatedData);
        }

        if &data[..MAGIC.len()] != MAGIC {
            return Err(SaveStateError::InvalidMagic);
        }

        let mut reader = Self {
            data,
            offset: MAGIC.len(),
        };

        let version = reader.read_u32()?;
        if version != STATE_VERSION {
            return Err(SaveStateError::UnsupportedVersion(version));
        }

        Ok(reader)
    }

    pub(crate) fn read_u8(&mut self) -> Result<u8, SaveStateError> {
        if self.offset < self.data.len() {
            let val = self.data[self.offset];
            self.offset += 1;
            Ok(val)
        } else {
            Err(SaveStateError::TruncatedData)
        }
    }

    pub(crate) fn read_bool(&mut self) -> Result<bool, SaveStateError> {
        Ok(self.read_u8()? != 0)
    }

    pub(crate) fn read_u16(&mut self) -> Result<u16, SaveStateError> {
        let raw = self.read_raw(2)?;
        Ok(u16::from_le_bytes([raw[0], raw[1]]))
    }

    pub(crate) fn read_u32(&mut self) -> Result<u32, SaveStateError> {
        let raw = self.read_raw(4)?;
        Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
    }

    pub(crate) fn read_raw(&mut self, len: usize) -> Result<&'a [u8], SaveStateError> {
        if self.offset + len <= self.data.len() {
            let slice = &self.data[self.offset..self.offset + len];
            self.offset += len;
            Ok(slice)
        } else {
            Err(SaveStateError::TruncatedData)
        }
    }

    pub(crate) fn read_bytes(&mut self) -> Result<&'a [u8], SaveStateError> {
        let len = self.read_u32()? as usize;
        self.read_raw(len)
    }
}
