//! Minimal ZIP writer (stored, no compression: STL compresses poorly in a browser and this keeps it dependency-free).

pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *slot = c;
    }
    !data.iter().fold(!0u32, |c, &b| {
        table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8)
    })
}

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

/// Wall clock for entries, in ms since the Unix epoch. Native builds read the system clock; WASM has none, so the host sets it.
static CLOCK_MS: AtomicU64 = AtomicU64::new(0);
/// Local time minus UTC, in seconds, for the DOS date and time fields.
static UTC_OFFSET_S: AtomicI64 = AtomicI64::new(0);

pub fn set_clock(unix_ms: u64, utc_offset_s: i64) {
    CLOCK_MS.store(unix_ms, Ordering::Relaxed);
    UTC_OFFSET_S.store(utc_offset_s, Ordering::Relaxed);
}

pub fn set_utc_offset(utc_offset_s: i64) {
    UTC_OFFSET_S.store(utc_offset_s, Ordering::Relaxed);
}

fn unix_now() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
    #[cfg(target_arch = "wasm32")]
    {
        CLOCK_MS.load(Ordering::Relaxed) / 1000
    }
}

/// (DOS time, DOS date) of a Unix time shifted to local time. ZIP cannot hold years before 1980.
fn dos_stamp(unix: u64, offset: i64) -> (u16, u16) {
    let t = (unix as i64 + offset).max(0);
    let (days, secs) = (t / 86_400, t % 86_400);
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    if year < 1980 {
        return (0, 0x21);
    }
    let time = (secs / 3600) << 11 | (secs % 3600 / 60) << 5 | (secs % 60 / 2);
    (
        time as u16,
        (((year - 1980) << 9) | (month << 5) | day) as u16,
    )
}

pub fn zip(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    let now = unix_now();
    let (dos_time, dos_date) = dos_stamp(now, UTC_OFFSET_S.load(Ordering::Relaxed));
    for (name, data) in files {
        let crc = crc32(data);
        let off = out.len() as u32;
        let (n, sz) = (name.as_bytes(), data.len() as u32);
        let head = |v: &mut Vec<u8>, central: bool| {
            v.extend_from_slice(if central {
                &[0x50, 0x4b, 1, 2, 20, 0]
            } else {
                &[0x50, 0x4b, 3, 4]
            });
            v.extend_from_slice(&[20, 0, 0, 0, 0, 0]); // version, flags, stored
            v.extend_from_slice(&dos_time.to_le_bytes());
            v.extend_from_slice(&dos_date.to_le_bytes());
            v.extend_from_slice(&crc.to_le_bytes());
            v.extend_from_slice(&sz.to_le_bytes());
            v.extend_from_slice(&sz.to_le_bytes());
            v.extend_from_slice(&(n.len() as u16).to_le_bytes());
            v.extend_from_slice(&(if central { 0u16 } else { 9 }).to_le_bytes());
            // extra field length
        };
        head(&mut out, false);
        out.extend_from_slice(n);
        // extended timestamp (UTC modification time), which unzip and most tools prefer to the local DOS fields
        out.extend_from_slice(&[0x55, 0x54, 5, 0, 1]);
        out.extend_from_slice(&(now as u32).to_le_bytes());
        out.extend_from_slice(data);
        head(&mut central, true);
        central.extend_from_slice(&[0; 8]); // comment, disk, internal and external attributes (part)
        central.extend_from_slice(&[0, 0]);
        central.extend_from_slice(&off.to_le_bytes());
        central.extend_from_slice(n);
    }
    let start = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&[0x50, 0x4b, 5, 6, 0, 0, 0, 0]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn crc_known_value() {
        assert_eq!(super::crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn dos_stamp_is_the_given_local_time() {
        // 2026-10-20 17:36:48 UTC, and the same instant in UTC+1
        let u = 1_792_517_808;
        let (t, d) = super::dos_stamp(u, 0);
        assert_eq!((d >> 9) + 1980, 2026);
        assert_eq!(((d >> 5) & 15, d & 31), (10, 20));
        assert_eq!((t >> 11, (t >> 5) & 63, (t & 31) * 2), (17, 36, 48));
        assert_eq!(super::dos_stamp(u, 3600).0 >> 11, 18);
        assert_eq!(super::dos_stamp(0, 0), (0, 0x21));
    }

    #[test]
    fn entries_carry_the_current_time_not_1980() {
        let z = super::zip(&[("a.stl".into(), vec![1, 2, 3])]);
        let date = u16::from_le_bytes([z[12], z[13]]);
        assert!((date >> 9) + 1980 >= 2024);
    }
}
