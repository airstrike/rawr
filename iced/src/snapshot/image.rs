//! Validated RGBA8 image artifacts.

use std::fs;
use std::io::{BufWriter, Cursor};
use std::path::Path;

use super::Error;

#[derive(Clone)]
pub(super) struct Image {
    width: u32,
    height: u32,
    bytes: Vec<u8>,
}

impl Image {
    pub(super) fn new(width: u32, height: u32, bytes: Vec<u8>) -> Result<Self, Error> {
        let expected = usize::try_from(width)
            .ok()
            .zip(usize::try_from(height).ok())
            .and_then(|(width, height)| width.checked_mul(height))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(Error::Buffer {
                width,
                height,
                expected: usize::MAX,
                actual: bytes.len(),
            })?;
        if bytes.len() != expected {
            return Err(Error::Buffer {
                width,
                height,
                expected,
                actual: bytes.len(),
            });
        }
        Ok(Self {
            width,
            height,
            bytes,
        })
    }

    pub(super) const fn width(&self) -> u32 {
        self.width
    }

    pub(super) const fn height(&self) -> u32 {
        self.height
    }

    pub(super) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(super) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub(super) fn write(&self, path: &Path) -> Result<(), Error> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| Error::io(parent, source))?;
        }
        let file = fs::File::create(path).map_err(|source| Error::io(path, source))?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|source| Error::Encode {
            path: path.to_path_buf(),
            source,
        })?;
        writer
            .write_image_data(&self.bytes)
            .map_err(|source| Error::Encode {
                path: path.to_path_buf(),
                source,
            })?;
        writer.finish().map_err(|source| Error::Encode {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(())
    }

    pub(super) fn encode(&self) -> Result<Vec<u8>, Error> {
        let mut bytes = Vec::new();
        let encode = |source| Error::Encode {
            path: "<memory>".into(),
            source,
        };
        let mut encoder = png::Encoder::new(&mut bytes, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(encode)?;
        writer.write_image_data(&self.bytes).map_err(encode)?;
        writer.finish().map_err(encode)?;
        Ok(bytes)
    }

    pub(super) fn decode(path: &Path, bytes: &[u8]) -> Result<Self, Error> {
        let decoder = png::Decoder::new(Cursor::new(bytes));
        let mut reader = decoder.read_info().map_err(|source| Error::Decode {
            path: path.to_path_buf(),
            source,
        })?;
        let info = reader.info();
        if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
            return Err(Error::Format {
                path: path.to_path_buf(),
                color: info.color_type,
                depth: info.bit_depth,
            });
        }
        let width = info.width;
        let height = info.height;
        let size = reader.output_buffer_size().ok_or_else(|| {
            Error::io(
                path,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "PNG decoder did not report an output size",
                ),
            )
        })?;
        let mut bytes = vec![0; size];
        let frame = reader
            .next_frame(&mut bytes)
            .map_err(|source| Error::Decode {
                path: path.to_path_buf(),
                source,
            })?;
        bytes.truncate(frame.buffer_size());
        Self::new(width, height, bytes)
    }

    #[cfg(test)]
    pub(super) fn read(path: &Path) -> Result<Self, Error> {
        let bytes = fs::read(path).map_err(|source| Error::io(path, source))?;
        Self::decode(path, &bytes)
    }
}
