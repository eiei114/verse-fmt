//! One bounded read path for every sized input: source (stdin and files) and
//! configuration. Callers keep their own diagnostics; this module owns the
//! off-by-one behaviour, so a limit can never silently return a truncated
//! buffer that later looks like a short but valid input.
use std::io::{self, Read};

/// Largest accepted `.verse.toml` or ignore file.
pub const CONFIG_BYTES: usize = 256 * 1024;

/// Read at most `limit` bytes.
///
/// `Ok(None)` means the input is larger than `limit`. The oversized prefix is
/// discarded rather than returned, so no caller can format or write a partial
/// document after a limit failure.
pub fn read_bounded(reader: impl Read, limit: usize) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    Ok((bytes.len() <= limit).then_some(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_exactly_the_limit_and_refuses_one_byte_more() {
        assert_eq!(
            read_bounded(&b"abcde"[..], 5).unwrap().unwrap(),
            b"abcde".to_vec()
        );
        assert!(read_bounded(&b"abcdef"[..], 5).unwrap().is_none());
        assert_eq!(read_bounded(&b""[..], 0).unwrap().unwrap(), b"".to_vec());
        assert!(read_bounded(&b"a"[..], 0).unwrap().is_none());
    }

    #[test]
    fn stops_one_byte_past_the_limit_instead_of_reading_everything() {
        // A reader that refuses to be read past the announced cap proves the
        // bound, not just the final length comparison.
        struct Capped {
            remaining: usize,
            overreads: usize,
        }
        impl Read for Capped {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                if self.remaining == 0 {
                    self.overreads += 1;
                    return Err(io::Error::other("read past the byte limit"));
                }
                let take = self.remaining.min(buf.len());
                buf[..take].fill(b'x');
                self.remaining -= take;
                Ok(take)
            }
        }
        let mut capped = Capped {
            remaining: 6,
            overreads: 0,
        };
        assert!(read_bounded(&mut capped, 5).unwrap().is_none());
        assert_eq!(capped.overreads, 0);
    }

    #[test]
    fn propagates_reader_failures_without_returning_bytes() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("injected read failure"))
            }
        }
        assert_eq!(
            read_bounded(Broken, 16).unwrap_err().to_string(),
            "injected read failure"
        );
    }
}
