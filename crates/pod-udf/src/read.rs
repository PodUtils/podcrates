//! The source of the bytes of an image: the caller's reader.

use std::io::{self, Read, Seek, SeekFrom};

/// Reads bytes of an image at an offset. A file, a buffer in memory or a
/// client of HTTP Range requests implements it; the reader adds no cache,
/// and a caller over a slow line keeps its own.
pub trait ReadAt {
    /// Fill `buf` with the bytes of the image from `offset`. An image that
    /// ends before the end of `buf` is [`io::ErrorKind::UnexpectedEof`].
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()>;
}

impl ReadAt for &[u8] {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let start = usize::try_from(offset).map_err(|_| io::ErrorKind::UnexpectedEof)?;
        let end = start.checked_add(buf.len()).ok_or(io::ErrorKind::UnexpectedEof)?;
        let bytes = self.get(start..end).ok_or(io::ErrorKind::UnexpectedEof)?;
        buf.copy_from_slice(bytes);
        Ok(())
    }
}

impl ReadAt for Vec<u8> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        self.as_slice().read_at(offset, buf)
    }
}

impl<T: ReadAt + ?Sized> ReadAt for &mut T {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        (**self).read_at(offset, buf)
    }
}

/// A [`ReadAt`] over a reader that seeks, such as a [`std::fs::File`].
#[derive(Debug)]
pub struct Seeker<R>(pub R);

impl<R: Read + Seek> ReadAt for Seeker<R> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        self.0.seek(SeekFrom::Start(offset))?;
        self.0.read_exact(buf)
    }
}
