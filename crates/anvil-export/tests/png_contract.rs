//! PNG structural contract for larger images and error paths.

use anvil_export::png::{encode_gray, PngError};

fn read_u32(data: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

#[test]
fn large_images_use_multiple_stored_blocks() {
    let (w, h) = (300u32, 300u32);
    let pixels = vec![128u8; (w * h) as usize];
    let png = encode_gray(w, h, &pixels).unwrap();
    assert_eq!(read_u32(&png, 16), w);
    assert_eq!(read_u32(&png, 20), h);
    let mut i = 8;
    let mut idat_blocks = 0;
    let mut total = 0usize;
    while i < png.len() {
        let len = read_u32(&png, i) as usize;
        let kind = &png[i + 4..i + 8];
        if kind == b"IDAT" {
            idat_blocks += 1;
            total += len;
        }
        i += 12 + len;
    }
    assert_eq!(idat_blocks, 1, "the encoder emits one IDAT chunk");
    // Raw scanlines exceed 65535, so the zlib stream has at least two blocks.
    assert!(total > 90_000);
    let idat = {
        let mut start = 8;
        loop {
            let len = read_u32(&png, start) as usize;
            if &png[start + 4..start + 8] == b"IDAT" {
                break png[start + 8..start + 8 + len].to_vec();
            }
            start += 12 + len;
        }
    };
    assert_eq!(idat[0], 0x78);
    assert_eq!(idat[1], 0x01);
    // Count stored-block headers after the two-byte zlib header.
    let mut p = 2;
    let mut blocks = 0;
    while p + 5 <= idat.len() - 4 {
        let header = idat[p];
        let len = u16::from_le_bytes([idat[p + 1], idat[p + 2]]) as usize;
        blocks += 1;
        p += 5 + len;
        if header & 1 == 1 {
            break;
        }
    }
    assert!(blocks >= 2, "blocks={blocks}");
    assert_eq!(p, idat.len() - 4, "adler trailer follows the final block");
}

#[test]
fn size_mismatches_are_rejected() {
    assert_eq!(encode_gray(0, 0, &[]), Err(PngError::Empty));
    assert_eq!(encode_gray(1, 1, &[]), Err(PngError::BadSize));
    assert_eq!(encode_gray(10_000, 1, &[0; 10_000]), Err(PngError::TooLarge));
}

#[test]
fn one_pixel_image_is_valid() {
    let png = encode_gray(1, 1, &[0]).unwrap();
    assert_eq!(read_u32(&png, 16), 1);
    assert_eq!(read_u32(&png, 20), 1);
    assert!(png.len() > 50);
}
