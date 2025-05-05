use std::io::{self, Write};

/// Wraps any `Write`, counts how many `\n` have been written.
pub struct LineCountWriter<W: Write> {
    inner: W,
    lines: usize,
}

impl<W: Write> LineCountWriter<W> {
    /// Create a new wrapper around `inner`.
    pub fn new(inner: W) -> Self {
        LineCountWriter { inner, lines: 0 }
    }

    /// Number of newline characters written so far.
    pub fn lines(&self) -> usize {
        self.lines
    }

    /// Unwraps, returning the inner writer and the final line count.
    pub fn into_inner(self) -> (W, usize) {
        (self.inner, self.lines)
    }
}

impl<W: Write> Write for LineCountWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        // only count in the bytes actually written
        self.lines += buf[..n].iter().filter(|&&b| b == b'\n').count();
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
