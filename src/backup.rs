#![cfg_attr(not(windows), allow(dead_code))]

//! Settings backups: one `.zip` with `config.json` and the pictures in
//! `icons\`, so a setup can move to another PC. Written uncompressed (the
//! pictures are already compressed and the settings are small), and read
//! back with every name, size and checksum checked, so a damaged or
//! hand-made file can't write outside the data folder or fill the disk.
//! Pure, so it can be unit-tested off Windows.

use crate::config::is_plain_file_name;

/// Limits on what a backup may hold.
pub const MAX_ENTRIES: usize = 5000;
pub const MAX_CONFIG: usize = 16 * 1024 * 1024;
/// The same limit as for a picture chosen in the app.
pub const MAX_PICTURE: usize = 8 * 1024 * 1024;
pub const MAX_TOTAL: usize = 256 * 1024 * 1024;

/// A file in a backup.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub data: Vec<u8>,
}

/// What a backup's file is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Config,
    /// A saved look (`look.json`, in a `.flexlook` file).
    Look,
    /// A picture for `icons\`, by its file name.
    Picture(String),
}

/// Which file `name` is, if a backup may hold it: `config.json`, or a plain
/// file name under `icons/`. Anything else (folders, `..`, drives, other
/// files) is refused.
pub fn kind_of(name: &str) -> Option<Kind> {
    if name == "config.json" {
        return Some(Kind::Config);
    }
    if name == "look.json" {
        return Some(Kind::Look);
    }
    let file = name.strip_prefix("icons/")?;
    is_plain_file_name(file).then(|| Kind::Picture(file.to_string()))
}

/// CRC-32 (the zip one).
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn u16le(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn u32le(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

/// A zip of `entries`, stored without compression. Names use `/`.
pub fn write_zip(entries: &[Entry]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for e in entries {
        let (crc, size, name) = (crc32(&e.data), e.data.len() as u32, e.name.as_bytes());
        let offset = out.len() as u32;
        // Local header.
        u32le(&mut out, 0x0403_4B50);
        u16le(&mut out, 20); // version needed
        u16le(&mut out, 0x0800); // flags: UTF-8 names
        u16le(&mut out, 0); // stored
        u16le(&mut out, 0); // time
        u16le(&mut out, 0x21); // date: 1980-01-01
        u32le(&mut out, crc);
        u32le(&mut out, size);
        u32le(&mut out, size);
        u16le(&mut out, name.len() as u16);
        u16le(&mut out, 0);
        out.extend_from_slice(name);
        out.extend_from_slice(&e.data);
        // Central directory record.
        u32le(&mut central, 0x0201_4B50);
        u16le(&mut central, 20); // made by
        u16le(&mut central, 20);
        u16le(&mut central, 0x0800);
        u16le(&mut central, 0);
        u16le(&mut central, 0);
        u16le(&mut central, 0x21);
        u32le(&mut central, crc);
        u32le(&mut central, size);
        u32le(&mut central, size);
        u16le(&mut central, name.len() as u16);
        u16le(&mut central, 0); // extra
        u16le(&mut central, 0); // comment
        u16le(&mut central, 0); // disk
        u16le(&mut central, 0); // internal attributes
        u32le(&mut central, 0); // external attributes
        u32le(&mut central, offset);
        central.extend_from_slice(name);
    }
    let (cd_offset, cd_size) = (out.len() as u32, central.len() as u32);
    out.extend_from_slice(&central);
    u32le(&mut out, 0x0605_4B50);
    u16le(&mut out, 0);
    u16le(&mut out, 0);
    u16le(&mut out, entries.len() as u16);
    u16le(&mut out, entries.len() as u16);
    u32le(&mut out, cd_size);
    u32le(&mut out, cd_offset);
    u16le(&mut out, 0);
    out
}

fn rd16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn rd32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

const NOT_OURS: &str = "This isn't a FlexTaskbar backup, or it has been damaged or re-compressed.";

/// Reads a backup made by [`write_zip`]: every entry must be stored (not
/// compressed), match its checksum, be a file a backup may hold
/// ([`kind_of`]) and stay within the limits. Errors are readable messages.
pub fn read_zip(b: &[u8]) -> Result<Vec<(Kind, Vec<u8>)>, String> {
    let entries = read_entries(b)?;
    if !entries.iter().any(|(k, _)| *k == Kind::Config) {
        return Err("This backup has no settings in it (config.json is missing).".into());
    }
    Ok(entries)
}

/// Reads a saved look (a `.flexlook` file): `look.json` and its pictures,
/// checked like a backup.
pub fn read_look(b: &[u8]) -> Result<Vec<(Kind, Vec<u8>)>, String> {
    let entries = read_entries(b).map_err(|e| e.replace("FlexTaskbar backup", "FlexTaskbar look"))?;
    if entries.iter().any(|(k, _)| *k == Kind::Config) || !entries.iter().any(|(k, _)| *k == Kind::Look) {
        return Err("This isn't a FlexTaskbar look file.".into());
    }
    Ok(entries)
}

/// Every entry, checked; `config.json` and `look.json` at most once each.
fn read_entries(b: &[u8]) -> Result<Vec<(Kind, Vec<u8>)>, String> {
    let bad = || NOT_OURS.to_string();
    // The end record is in the last 64 KiB (it may be followed by a comment).
    let from = b.len().saturating_sub(22 + 0xFFFF);
    let end = (from..b.len().saturating_sub(21)).rev().find(|&i| rd32(b, i) == Some(0x0605_4B50)).ok_or_else(bad)?;
    let count = rd16(b, end + 10).ok_or_else(bad)? as usize;
    let cd_offset = rd32(b, end + 16).ok_or_else(bad)? as usize;
    if count > MAX_ENTRIES {
        return Err("This backup holds too many files.".into());
    }
    let mut out = Vec::new();
    let mut total = 0usize;
    let mut at = cd_offset;
    let mut seen: Vec<Kind> = Vec::new();
    for _ in 0..count {
        if rd32(b, at) != Some(0x0201_4B50) {
            return Err(bad());
        }
        let method = rd16(b, at + 10).ok_or_else(bad)?;
        let crc = rd32(b, at + 16).ok_or_else(bad)?;
        let packed = rd32(b, at + 20).ok_or_else(bad)? as usize;
        let size = rd32(b, at + 24).ok_or_else(bad)? as usize;
        let name_len = rd16(b, at + 28).ok_or_else(bad)? as usize;
        let skip = rd16(b, at + 30).ok_or_else(bad)? as usize + rd16(b, at + 32).ok_or_else(bad)? as usize;
        let local = rd32(b, at + 42).ok_or_else(bad)? as usize;
        let name = std::str::from_utf8(b.get(at + 46..at + 46 + name_len).ok_or_else(bad)?).map_err(|_| bad())?;
        at += 46 + name_len + skip;
        if method != 0 || packed != size {
            return Err(bad());
        }
        let Some(kind) = kind_of(name) else {
            return Err(format!("This backup holds a file FlexTaskbar doesn't use: {name}"));
        };
        let limit = if matches!(kind, Kind::Picture(_)) { MAX_PICTURE } else { MAX_CONFIG };
        total = total.saturating_add(size);
        if size > limit || total > MAX_TOTAL {
            return Err(format!("A file in this backup is too large: {name}"));
        }
        if rd32(b, local) != Some(0x0403_4B50) {
            return Err(bad());
        }
        let local_name = rd16(b, local + 26).ok_or_else(bad)? as usize;
        let local_extra = rd16(b, local + 28).ok_or_else(bad)? as usize;
        let start = local + 30 + local_name + local_extra;
        let data = b.get(start..start.checked_add(size).ok_or_else(bad)?).ok_or_else(bad)?;
        if crc32(data) != crc {
            return Err(bad());
        }
        if !matches!(kind, Kind::Picture(_)) {
            if seen.contains(&kind) {
                return Err(bad());
            }
            seen.push(kind.clone());
        }
        out.push((kind, data.to_vec()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Entry> {
        vec![
            Entry { name: "config.json".into(), data: br#"{"version":2}"#.to_vec() },
            Entry { name: "icons/18dbae98d1de954c.png".into(), data: vec![0x89, b'P', b'N', b'G', 0, 1, 2, 3] },
            Entry { name: "icons/empty.ico".into(), data: Vec::new() },
        ]
    }

    #[test]
    fn crc() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926); // the standard check value
    }

    #[test]
    fn round_trip() {
        let zip = write_zip(&sample());
        let back = read_zip(&zip).unwrap();
        assert_eq!(back.len(), 3);
        assert_eq!(back[0], (Kind::Config, br#"{"version":2}"#.to_vec()));
        assert_eq!(back[1].0, Kind::Picture("18dbae98d1de954c.png".into()));
        assert_eq!(back[2], (Kind::Picture("empty.ico".into()), Vec::new()));
    }

    #[test]
    fn names_are_checked() {
        assert_eq!(kind_of("config.json"), Some(Kind::Config));
        assert_eq!(kind_of("icons/a.png"), Some(Kind::Picture("a.png".into())));
        for bad in
            ["icons/../config.json", "icons/sub/a.png", "../x", "C:/x", "icons/", "apps-cache.json", "icons\\a.png"]
        {
            assert_eq!(kind_of(bad), None, "{bad}");
        }
        let mut entries = sample();
        entries.push(Entry { name: "icons/../../evil.exe".into(), data: vec![1] });
        assert!(read_zip(&write_zip(&entries)).unwrap_err().contains("doesn't use"));
    }

    #[test]
    fn damage_is_caught() {
        let mut zip = write_zip(&sample());
        // Flip a byte of the picture's data: its checksum no longer matches.
        let i = zip.windows(4).position(|w| w == [0x89, b'P', b'N', b'G']).unwrap();
        zip[i + 5] ^= 0xFF;
        assert_eq!(read_zip(&zip).unwrap_err(), NOT_OURS);
        // Cut short, or not a zip at all.
        let zip = write_zip(&sample());
        assert!(read_zip(&zip[..zip.len() / 2]).is_err());
        assert!(read_zip(b"hello").is_err());
        assert!(read_zip(&[]).is_err());
    }

    #[test]
    fn look_files() {
        let look = vec![
            Entry { name: "look.json".into(), data: b"{}".to_vec() },
            Entry { name: "icons/a.png".into(), data: vec![1, 2] },
        ];
        let back = read_look(&write_zip(&look)).unwrap();
        assert_eq!(back[0], (Kind::Look, b"{}".to_vec()));
        // A backup isn't a look, and a look isn't a backup.
        assert!(read_look(&write_zip(&sample())).is_err());
        assert!(read_zip(&write_zip(&look)).is_err());
        assert!(read_look(b"nope").unwrap_err().contains("look"));
    }

    #[test]
    fn settings_are_required_and_sizes_limited() {
        let only_pictures = vec![Entry { name: "icons/a.png".into(), data: vec![1] }];
        assert!(read_zip(&write_zip(&only_pictures)).unwrap_err().contains("config.json"));
        let big = vec![
            Entry { name: "config.json".into(), data: b"{}".to_vec() },
            Entry { name: "icons/big.png".into(), data: vec![0; MAX_PICTURE + 1] },
        ];
        assert!(read_zip(&write_zip(&big)).unwrap_err().contains("too large"));
        let twice = vec![
            Entry { name: "config.json".into(), data: b"{}".to_vec() },
            Entry { name: "config.json".into(), data: b"{}".to_vec() },
        ];
        assert!(read_zip(&write_zip(&twice)).is_err());
    }
}
