//! Exact Float32 packing: [lead][cell, level, field] -> [field, cell, level, lead].
//! The Rust loader validates staged shapes, dtypes and pointer lifetimes.
//! No floating arithmetic: finite checks inspect bits, preserving signed zeros.
#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;

#[no_mangle]
pub extern "C" fn has_neon() -> i32 {
    cfg!(target_arch = "aarch64") as i32
}

/// # Safety
/// source contains t valid pointers to c*l*f floats; out has c*l*f*t floats.
#[no_mangle]
pub unsafe extern "C" fn pack_scalar(
    source: *const *const f32,
    out: *mut f32,
    c: usize,
    l: usize,
    f: usize,
    t: usize,
) -> i32 {
    let mut bad = false;
    for time in 0..t {
        for cell in 0..c {
            for level in 0..l {
                for field in 0..f {
                    let v = *(*source.add(time)).add((cell * l + level) * f + field);
                    bad |= v.to_bits() & 0x7fffffff >= 0x7f800000;
                    *out.add(((field * c + cell) * l + level) * t + time) = v;
                }
            }
        }
    }
    bad as i32
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn finite(v: float32x4_t) -> uint32x4_t {
    vcltq_u32(
        vandq_u32(vreinterpretq_u32_f32(v), vdupq_n_u32(0x7fffffff)),
        vdupq_n_u32(0x7f800000),
    )
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn six_columns(p: *const f32) -> [float32x4_t; 6] {
    let a = vld3q_f32(p);
    let b = vld3q_f32(p.add(12));
    [
        vuzp1q_f32(a.0, b.0),
        vuzp1q_f32(a.1, b.1),
        vuzp1q_f32(a.2, b.2),
        vuzp2q_f32(a.0, b.0),
        vuzp2q_f32(a.1, b.1),
        vuzp2q_f32(a.2, b.2),
    ]
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn transpose4(
    a: float32x4_t,
    b: float32x4_t,
    c: float32x4_t,
    d: float32x4_t,
) -> [float32x4_t; 4] {
    let ab0 = vtrn1q_f32(a, b);
    let ab1 = vtrn2q_f32(a, b);
    let cd0 = vtrn1q_f32(c, d);
    let cd1 = vtrn2q_f32(c, d);
    [
        vcombine_f32(vget_low_f32(ab0), vget_low_f32(cd0)),
        vcombine_f32(vget_low_f32(ab1), vget_low_f32(cd1)),
        vcombine_f32(vget_high_f32(ab0), vget_high_f32(cd0)),
        vcombine_f32(vget_high_f32(ab1), vget_high_f32(cd1)),
    ]
}

/// # Safety
/// Same buffer contract as pack_scalar; no buffers may overlap.
#[no_mangle]
pub unsafe extern "C" fn pack_neon(
    source: *const *const f32,
    out: *mut f32,
    c: usize,
    l: usize,
    f: usize,
    t: usize,
) -> i32 {
    pack_tiled::<16>(source, out, c, l, f, t)
}

/// Keep a small group of output profiles in cache while visiting every lead.
/// TILE >= c reproduces the original lead-first traversal for measurement.
/// # Safety
/// Same buffer contract as pack_scalar; TILE must be positive.
pub unsafe fn pack_tiled<const TILE: usize>(
    source: *const *const f32,
    out: *mut f32,
    c: usize,
    l: usize,
    f: usize,
    t: usize,
) -> i32 {
    assert!(TILE > 0);
    #[cfg(not(target_arch = "aarch64"))]
    {
        return pack_scalar(source, out, c, l, f, t);
    }
    #[cfg(target_arch = "aarch64")]
    {
        let mut good = vdupq_n_u32(u32::MAX);
        let mut bad = false;
        for first in (0..c).step_by(TILE) {
            let last = first.saturating_add(TILE).min(c);
            let mut time = 0;
            while time + 4 <= t {
                for cell in first..last {
                    let mut level = 0;
                    if f == 6 {
                        while level + 4 <= l {
                            let columns = [
                                six_columns((*source.add(time)).add((cell * l + level) * 6)),
                                six_columns((*source.add(time + 1)).add((cell * l + level) * 6)),
                                six_columns((*source.add(time + 2)).add((cell * l + level) * 6)),
                                six_columns((*source.add(time + 3)).add((cell * l + level) * 6)),
                            ];
                            for (field, _) in columns[0].iter().enumerate() {
                                let values = transpose4(
                                    columns[0][field],
                                    columns[1][field],
                                    columns[2][field],
                                    columns[3][field],
                                );
                                for (k, v) in values.into_iter().enumerate() {
                                    good = vandq_u32(good, finite(v));
                                    vst1q_f32(
                                        out.add(((field * c + cell) * l + level + k) * t + time),
                                        v,
                                    );
                                }
                            }
                            level += 4;
                        }
                    }
                    while level < l {
                        for field in 0..f {
                            let i = (cell * l + level) * f + field;
                            let values = [
                                *(*source.add(time)).add(i),
                                *(*source.add(time + 1)).add(i),
                                *(*source.add(time + 2)).add(i),
                                *(*source.add(time + 3)).add(i),
                            ];
                            let v = vld1q_f32(values.as_ptr());
                            good = vandq_u32(good, finite(v));
                            vst1q_f32(out.add(((field * c + cell) * l + level) * t + time), v);
                        }
                        level += 1;
                    }
                }
                time += 4;
            }
            while time < t {
                for cell in first..last {
                    for level in 0..l {
                        for field in 0..f {
                            let v = *(*source.add(time)).add((cell * l + level) * f + field);
                            bad |= v.to_bits() & 0x7fffffff >= 0x7f800000;
                            *out.add(((field * c + cell) * l + level) * t + time) = v;
                        }
                    }
                }
                time += 1;
            }
        }
        (bad || vminvq_u32(good) == 0) as i32
    }
}

fn varuint(out: &mut Vec<u8>, mut n: usize) {
    while n >= 128 {
        out.push((n as u8 & 127) | 128);
        n >>= 7;
    }
    out.push(n as u8);
}
fn text(out: &mut Vec<u8>, s: &[u8]) {
    varuint(out, s.len());
    out.extend_from_slice(s);
}

/// Append exact UInt16 indices without a scalar Vec append for every value.
/// All inputs are checked before narrowing; no overflowing word is accepted.
unsafe fn append_uint16(out: &mut Vec<u8>, values: &[u32]) -> bool {
    let bytes = values.len() * 2;
    out.reserve(bytes);
    let target = out.as_mut_ptr().add(out.len());
    let mut i = 0;
    #[cfg(target_arch = "aarch64")]
    {
        let mut maximum = vdupq_n_u32(0);
        while i + 8 <= values.len() {
            let a = vld1q_u32(values.as_ptr().add(i));
            let b = vld1q_u32(values.as_ptr().add(i + 4));
            maximum = vmaxq_u32(maximum, vmaxq_u32(a, b));
            vst1q_u16(
                target.add(i * 2).cast(),
                vcombine_u16(vmovn_u32(a), vmovn_u32(b)),
            );
            i += 8;
        }
        if vmaxvq_u32(maximum) > u16::MAX as u32 {
            return false;
        }
    }
    while i < values.len() {
        let Ok(value) = u16::try_from(values[i]) else {
            return false;
        };
        target.add(i * 2).cast::<u16>().write_unaligned(value);
        i += 1;
    }
    out.set_len(out.len() + bytes);
    true
}

/// Encode a plain HTTP Native-format block. Names are NUL-terminated ASCII;
/// each column contains cells*lengths[f] 32-bit values. The Rust caller owns all inputs.
/// # Safety
/// All pointers must be valid for their declared sizes; out_len must be writable.
#[no_mangle]
pub unsafe extern "C" fn encode_native(
    names: *const *const std::ffi::c_char,
    columns: *const *const u32,
    lengths: *const usize,
    fields: usize,
    ids: *const u32,
    cells: usize,
    encoded: i32,
    out_len: *mut usize,
) -> *mut u8 {
    if cfg!(target_endian = "big") || fields == 0 || cells == 0 || !(0..=2).contains(&encoded) {
        return std::ptr::null_mut();
    }
    let value_bytes = if encoded == 2 { 2 } else { 4 };
    fn vsize(mut n: usize) -> usize {
        let mut size = 1;
        while n >= 128 {
            size += 1;
            n >>= 7;
        }
        size
    }
    let mut size =
        vsize(fields + 2) + vsize(cells) + 1 + 11 + 1 + 6 + cells * 4 + 1 + 7 + 1 + 6 + cells * 4;
    let kind: &[u8] = match encoded {
        2 => b"Array(UInt16)",
        1 => b"Array(UInt32)",
        _ => b"Array(Float32)",
    };
    for f in 0..fields {
        let n = std::ffi::CStr::from_ptr(*names.add(f)).to_bytes().len();
        size += vsize(n)
            + n
            + vsize(kind.len())
            + kind.len()
            + cells * 8
            + cells * (*lengths.add(f)) * value_bytes;
    }
    let mut out = Vec::with_capacity(size);
    varuint(&mut out, fields + 2);
    varuint(&mut out, cells);
    text(&mut out, b"revision_id");
    text(&mut out, b"UInt32");
    for _ in 0..cells {
        out.extend_from_slice(&1u32.to_le_bytes());
    }
    text(&mut out, b"cell_id");
    text(&mut out, b"UInt32");
    out.extend_from_slice(std::slice::from_raw_parts(ids as *const u8, cells * 4));
    for f in 0..fields {
        text(&mut out, std::ffi::CStr::from_ptr(*names.add(f)).to_bytes());
        text(&mut out, kind);
        let width = *lengths.add(f);
        for row in 1..=cells {
            out.extend_from_slice(&((row * width) as u64).to_le_bytes());
        }
        if encoded == 2 {
            if !append_uint16(
                &mut out,
                std::slice::from_raw_parts(*columns.add(f), cells * width),
            ) {
                return std::ptr::null_mut();
            }
        } else {
            out.extend_from_slice(std::slice::from_raw_parts(
                *columns.add(f) as *const u8,
                cells * width * 4,
            ));
        }
    }
    let mut data = out.into_boxed_slice();
    *out_len = data.len();
    let p = data.as_mut_ptr();
    std::mem::forget(data);
    p
}

/// # Safety
/// pointer and len must be the exact pair returned by encode_native, used once.
#[no_mangle]
pub unsafe extern "C" fn free_native(pointer: *mut u8, len: usize) {
    if !pointer.is_null() {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            pointer, len,
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vector_uint16_rejects_overflow_in_vector_and_tail() {
        for bad in [None, Some(0), Some(8), Some(16)] {
            let mut values = [65535u32; 17];
            if let Some(i) = bad {
                values[i] = 65536;
            }
            let mut output = vec![42u8];
            assert_eq!(
                unsafe { append_uint16(&mut output, &values) },
                bad.is_none()
            );
            if bad.is_some() {
                assert_eq!(output, [42]);
            } else {
                assert_eq!(output.len(), 35);
                assert!(output[1..].iter().all(|v| *v == 255));
            }
        }
    }
    #[test]
    fn uint16_native_wire_preserves_the_entire_code_domain() {
        let values: Vec<u32> = (0..=65535).collect();
        let mut bytes = vec![7u8];
        assert!(unsafe { append_uint16(&mut bytes, &values) });
        assert_eq!(bytes.len(), 1 + 65536 * 2);
        for (code, word) in bytes[1..].as_chunks::<2>().0.iter().enumerate() {
            assert_eq!(u16::from_le_bytes(*word) as usize, code);
        }
    }
    #[test]
    fn native_uint16_keeps_boundary_and_rejects_overflow() {
        let name = std::ffi::CString::new("temperature").unwrap();
        let names = [name.as_ptr()];
        let widths = [2];
        let ids = [7];
        for (codes, mode, succeeds) in [
            ([0u32, 65535], 2, true),
            ([0u32, 65536], 2, false),
            ([0u32, 65536], 1, true),
        ] {
            let columns = [codes.as_ptr()];
            let mut len = 0;
            let pointer = unsafe {
                encode_native(
                    names.as_ptr(),
                    columns.as_ptr(),
                    widths.as_ptr(),
                    1,
                    ids.as_ptr(),
                    1,
                    mode,
                    &mut len,
                )
            };
            assert_eq!(!pointer.is_null(), succeeds);
            if succeeds {
                let bytes = unsafe { std::slice::from_raw_parts(pointer, len) };
                if mode == 2 {
                    assert!(bytes.windows(13).any(|w| w == b"Array(UInt16)"));
                    assert_eq!(&bytes[len - 4..], &[0, 0, 255, 255]);
                } else {
                    assert_eq!(&bytes[len - 4..], &65536u32.to_le_bytes());
                }
                unsafe { free_native(pointer, len) };
            }
        }
    }
}
