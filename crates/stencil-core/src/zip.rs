//! Minimal ZIP writer (stored, no compression: STL compresses poorly in a browser and this keeps it dependency-free).

fn crc32(data: &[u8]) -> u32 {
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

pub fn zip(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
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
            v.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0x21, 0]); // version, flags, stored, time, date (1980-01-01)
            v.extend_from_slice(&crc.to_le_bytes());
            v.extend_from_slice(&sz.to_le_bytes());
            v.extend_from_slice(&sz.to_le_bytes());
            v.extend_from_slice(&(n.len() as u16).to_le_bytes());
            v.extend_from_slice(&[0, 0]);
        };
        head(&mut out, false);
        out.extend_from_slice(n);
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
}
