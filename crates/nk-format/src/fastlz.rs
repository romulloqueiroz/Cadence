//! FastLZ decompression (levels 1 and 2).
//!
//! Ported from the algorithm reference used by ConvertWithMoss, itself based on
//! <https://ariya.github.io/FastLZ/>. Only decompression is needed here: the
//! NI container stores some subtrees FastLZ-compressed.

use crate::Error;

const MAX_DISTANCE_LZ2: usize = 8191;

/// Decompress `input` into at most `output_size` bytes.
pub fn decompress(input: &[u8], output_size: usize) -> Result<Vec<u8>, Error> {
    if input.is_empty() {
        return Err(Error::Malformed("FastLZ: empty input".into()));
    }

    let level = (input[0] >> 5) + 1;
    if level == 0 {
        return Err(Error::Malformed("FastLZ: level 0".into()));
    }

    let mut out = vec![0u8; output_size];
    let mut ip = 1usize;
    let mut op = 0usize;
    let mut ctrl = (input[0] & 31) as usize;

    let mut looping = true;
    while looping {
        let mut r = op as isize;
        let mut len = ctrl >> 5;
        let mut ofs = (ctrl & 31) << 8;

        if ctrl >= 32 {
            len -= 1;
            r -= ofs as isize;

            if len == 6 {
                if level == 1 {
                    len += read_byte(input, &mut ip)? as usize;
                } else {
                    loop {
                        let code = read_byte(input, &mut ip)? as usize;
                        len += code;
                        if code != 255 {
                            break;
                        }
                    }
                }
            }

            if level == 1 {
                r -= read_byte(input, &mut ip)? as isize;
            } else {
                let code = read_byte(input, &mut ip)? as usize;
                r -= code as isize;

                if code == 255 && ofs == (31 << 8) {
                    ofs = (read_byte(input, &mut ip)? as usize) << 8;
                    ofs += read_byte(input, &mut ip)? as usize;
                    r = op as isize - ofs as isize - MAX_DISTANCE_LZ2 as isize;
                }
            }

            if op + len + 3 > out.len() {
                return Err(Error::Malformed("FastLZ: output overflow".into()));
            }
            if r < 1 {
                return Err(Error::Malformed("FastLZ: invalid back-reference".into()));
            }

            if ip < input.len() {
                ctrl = input[ip] as usize;
                ip += 1;
            } else {
                looping = false;
            }

            if r == op as isize {
                let b = out[(r - 1) as usize];
                for _ in 0..3 + len {
                    out[op] = b;
                    op += 1;
                }
            } else {
                r -= 1;
                for _ in 0..3 + len {
                    let index = r as usize;
                    if index >= out.len() {
                        return Err(Error::Malformed("FastLZ: reference out of range".into()));
                    }
                    out[op] = out[index];
                    op += 1;
                    r += 1;
                }
            }
        } else {
            ctrl += 1;

            if op + ctrl > out.len() {
                return Err(Error::Malformed("FastLZ: literal overflow".into()));
            }
            if ip + ctrl > input.len() {
                return Err(Error::Malformed("FastLZ: truncated literal run".into()));
            }

            for _ in 0..ctrl {
                out[op] = input[ip];
                op += 1;
                ip += 1;
            }

            looping = ip < input.len();
            if looping {
                ctrl = input[ip] as usize;
                ip += 1;
            }
        }
    }

    out.truncate(op);
    Ok(out)
}

fn read_byte(input: &[u8], ip: &mut usize) -> Result<u8, Error> {
    let byte = *input
        .get(*ip)
        .ok_or_else(|| Error::Malformed("FastLZ: unexpected end of input".into()))?;
    *ip += 1;
    Ok(byte)
}
