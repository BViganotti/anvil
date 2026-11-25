//! Minimal PNG encoder for grayscale preview images. Standard library only.
//!
//! Uses uncompressed DEFLATE stored blocks, so the output is larger than a real
//! encoder but valid, deterministic and dependency-free.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PngError {
    Empty,
    TooLarge,
    BadSize,
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = Vec::with_capacity(4 + data.len());
    body.extend_from_slice(kind);
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + raw.len() / 16384 + 16);
    out.push(0x78);
    out.push(0x01);
    if raw.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
    } else {
        let mut chunks = raw.chunks(65535).peekable();
        while let Some(chunk) = chunks.next() {
            let final_block = chunks.peek().is_none();
            out.push(if final_block { 0x01 } else { 0x00 });
            let len = chunk.len() as u16;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(chunk);
        }
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

/// Encode an 8-bit grayscale image as PNG.
pub fn encode_gray(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, PngError> {
    if width == 0 || height == 0 {
        return Err(PngError::Empty);
    }
    if width > 8192 || height > 8192 {
        return Err(PngError::TooLarge);
    }
    let expected = (width as usize)
        .checked_mul(height as usize)
        .ok_or(PngError::TooLarge)?;
    if pixels.len() != expected {
        return Err(PngError::BadSize);
    }
    let stride = width as usize;
    let mut raw = Vec::with_capacity(expected + height as usize);
    for y in 0..height as usize {
        raw.push(0); // filter: none
        raw.extend_from_slice(&pixels[y * stride..(y + 1) * stride]);
    }
    let mut out = Vec::with_capacity(raw.len() + 128);
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(0); // grayscale
    ihdr.push(0); // deflate
    ihdr.push(0); // adaptive filtering
    ihdr.push(0); // no interlace
    write_chunk(&mut out, b"IHDR", &ihdr);
    write_chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    write_chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_u32(data: &[u8], at: usize) -> u32 {
        u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
    }

    #[test]
    fn encodes_valid_png() {
        let px = vec![0u8, 128, 255, 64];
        let png = encode_gray(2, 2, &px).unwrap();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(read_u32(&png, 16), 2);
        assert_eq!(read_u32(&png, 20), 2);
        let mut i = 8;
        let mut idat = Vec::new();
        while i < png.len() {
            let l = read_u32(&png, i) as usize;
            let kind = &png[i + 4..i + 8];
            if kind == b"IDAT" {
                idat.extend_from_slice(&png[i + 8..i + 8 + l]);
            }
            i += 12 + l;
        }
        assert_eq!(idat[0], 0x78);
        assert_eq!(idat[1], 0x01);
        // First stored block header.
        assert_eq!(idat[2], 0x01);
        let n = u16::from_le_bytes([idat[3], idat[4]]) as usize;
        assert_eq!(n, 4 + 2); // two rows with filter bytes
        assert_eq!(&idat[7..13], &[0, 0, 128, 0, 255, 64]);
        let adler = u32::from_be_bytes([
            idat[idat.len() - 4],
            idat[idat.len() - 3],
            idat[idat.len() - 2],
            idat[idat.len() - 1],
        ]);
        assert_eq!(adler, adler32(&[0, 0, 128, 0, 255, 64]));
    }

    #[test]
    fn rejects_bad_sizes() {
        assert_eq!(encode_gray(0, 1, &[]), Err(PngError::Empty));
        assert_eq!(encode_gray(2, 2, &[0, 0]), Err(PngError::BadSize));
    }

    #[test]
    fn crc_known_value() {
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
    }
}
