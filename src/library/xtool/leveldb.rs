//! Just enough LevelDB to read a copied Chromium IndexedDB folder: table files (`.ldb`, Snappy blocks)
//! and the write-ahead log (`.log`). Read-only and tolerant: a damaged block or record is skipped.

use std::collections::HashMap;
use std::path::Path;

/// (user key, sequence, value or None for a deletion).
type Entry = (Vec<u8>, u64, Option<Vec<u8>>);

/// A cursor over bytes; every read returns None past the end instead of panicking.
pub(super) struct Bytes<'a> {
    pub b: &'a [u8],
    pub i: usize,
}

impl<'a> Bytes<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Self { b, i: 0 }
    }

    pub fn byte(&mut self) -> Option<u8> {
        let c = *self.b.get(self.i)?;
        self.i += 1;
        Some(c)
    }

    pub fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.i..self.i.checked_add(n)?)?;
        self.i += n;
        Some(s)
    }

    pub fn varint(&mut self) -> Option<u64> {
        let mut r = 0u64;
        for shift in (0..64).step_by(7) {
            let c = self.byte()?;
            r |= u64::from(c & 0x7f) << shift;
            if c < 0x80 {
                return Some(r);
            }
        }
        None
    }

    fn sized(&mut self) -> Option<&'a [u8]> {
        let n = self.varint()?;
        self.take(usize::try_from(n).ok()?)
    }
}

/// Snappy raw-format decompression (the block format LevelDB uses).
pub(super) fn snappy(b: &[u8]) -> Option<Vec<u8>> {
    let mut r = Bytes::new(b);
    let len = usize::try_from(r.varint()?).ok()?;
    let mut out = Vec::with_capacity(len.min(1 << 24));
    while r.i < b.len() {
        let tag = r.byte()?;
        let (n, offset) = match tag & 3 {
            0 => {
                let mut n = usize::from(tag >> 2);
                if n >= 60 {
                    let k = n - 59;
                    n = r.take(k)?.iter().rev().fold(0, |a, c| a << 8 | usize::from(*c));
                }
                out.extend_from_slice(r.take(n + 1)?);
                continue;
            }
            1 => (usize::from((tag >> 2) & 7) + 4, usize::from(tag >> 5) << 8 | usize::from(r.byte()?)),
            2 => (usize::from(tag >> 2) + 1, r.take(2)?.iter().rev().fold(0, |a, c| a << 8 | usize::from(*c))),
            _ => (usize::from(tag >> 2) + 1, r.take(4)?.iter().rev().fold(0, |a, c| a << 8 | usize::from(*c))),
        };
        if offset == 0 || offset > out.len() {
            return None;
        }
        for _ in 0..n {
            out.push(out[out.len() - offset]);
        }
    }
    (out.len() == len).then_some(out)
}

/// The key/value entries of one table block (prefix-compressed keys, restart array at the end).
fn block_entries(blk: &[u8]) -> Vec<(Vec<u8>, &[u8])> {
    let mut out = Vec::new();
    let Some(n) = blk.len().checked_sub(4).and_then(|p| blk.get(p..)).map(|t| u32::from_le_bytes(t.try_into().unwrap()) as usize) else { return out };
    let Some(end) = n.checked_mul(4).and_then(|r| blk.len().checked_sub(4 + r)) else { return out };
    let mut r = Bytes::new(&blk[..end]);
    let mut key: Vec<u8> = Vec::new();
    while r.i < end {
        let Some((shared, fresh, vlen)) = (|| Some((r.varint()? as usize, r.varint()? as usize, r.varint()? as usize)))() else { break };
        let (Some(k), true) = (r.take(fresh), shared <= key.len()) else { break };
        key.truncate(shared);
        key.extend_from_slice(k);
        let Some(v) = r.take(vlen) else { break };
        out.push((key.clone(), v));
    }
    out
}

fn read_block(file: &[u8], handle: &[u8]) -> Option<Vec<u8>> {
    let mut h = Bytes::new(handle);
    let (off, size) = (usize::try_from(h.varint()?).ok()?, usize::try_from(h.varint()?).ok()?);
    let raw = file.get(off..off.checked_add(size)?)?;
    match file.get(off + size)? {
        0 => Some(raw.to_vec()),
        1 => snappy(raw),
        _ => None,
    }
}

/// (user key, sequence, value or None for a deletion) of every entry in a table file.
fn table(file: &[u8], out: &mut Vec<Entry>, keep: &dyn Fn(&[u8]) -> bool) {
    let Some(footer) = file.len().checked_sub(48).map(|p| &file[p..]) else { return };
    let mut f = Bytes::new(footer);
    let index = (|| {
        f.varint()?;
        f.varint()?;
        let start = f.i;
        f.varint()?;
        f.varint()?;
        Some(footer[start..f.i].to_vec())
    })();
    let Some(index) = index.and_then(|h| read_block(file, &h)) else { return };
    for (_, handle) in block_entries(&index) {
        let Some(data) = read_block(file, handle) else { continue };
        for (k, v) in block_entries(&data) {
            let Some(split) = k.len().checked_sub(8) else { continue };
            let trailer = u64::from_le_bytes(k[split..].try_into().unwrap());
            let user = &k[..split];
            if keep(user) {
                out.push((user.to_vec(), trailer >> 8, (trailer & 0xff == 1).then(|| v.to_vec())));
            }
        }
    }
}

/// The same for the write-ahead log: 32 KiB blocks of records holding write batches.
fn log(file: &[u8], out: &mut Vec<Entry>, keep: &dyn Fn(&[u8]) -> bool) {
    const BLOCK: usize = 32 * 1024;
    let (mut i, mut batch) = (0, Vec::new());
    while i + 7 <= file.len() {
        if BLOCK - i % BLOCK < 7 {
            i += BLOCK - i % BLOCK;
            continue;
        }
        let len = usize::from(u16::from_le_bytes([file[i + 4], file[i + 5]]));
        let kind = file[i + 6];
        let Some(data) = file.get(i + 7..i + 7 + len) else { break };
        i += 7 + len;
        match kind {
            1 | 2 => batch = data.to_vec(),
            3 | 4 => batch.extend_from_slice(data),
            _ => continue,
        }
        if kind == 1 || kind == 4 {
            write_batch(&batch, out, keep);
        }
    }
}

fn write_batch(b: &[u8], out: &mut Vec<Entry>, keep: &dyn Fn(&[u8]) -> bool) {
    let Some(head) = b.get(..12) else { return };
    let seq = u64::from_le_bytes(head[..8].try_into().unwrap());
    let count = u32::from_le_bytes(head[8..].try_into().unwrap());
    let mut r = Bytes::new(b);
    r.i = 12;
    for n in 0..u64::from(count) {
        let Some(kind) = r.byte() else { return };
        let Some(key) = r.sized() else { return };
        let value = match kind {
            1 => match r.sized() {
                Some(v) => Some(v),
                None => return,
            },
            _ => None,
        };
        if keep(key) {
            out.push((key.to_vec(), seq + n, value.map(<[u8]>::to_vec)));
        }
    }
}

/// The live value of every key `keep` accepts, from all table and log files in `dir` (newest wins,
/// deleted keys are left out). Keys `keep` refuses are never copied out of the file buffers.
pub(super) fn read_dir(dir: &Path, keep: &dyn Fn(&[u8]) -> bool) -> std::io::Result<HashMap<Vec<u8>, Vec<u8>>> {
    let mut all = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        match path.extension().and_then(|e| e.to_str()) {
            Some("ldb" | "sst") => table(&std::fs::read(&path)?, &mut all, keep),
            Some("log") => log(&std::fs::read(&path)?, &mut all, keep),
            _ => {}
        }
    }
    let mut newest: HashMap<Vec<u8>, (u64, Option<Vec<u8>>)> = HashMap::new();
    for (k, seq, v) in all {
        if newest.get(&k).is_none_or(|(s, _)| seq >= *s) {
            newest.insert(k, (seq, v));
        }
    }
    Ok(newest.into_iter().filter_map(|(k, (_, v))| Some((k, v?))).collect())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// Snappy with one literal and one copy: "abcabcabc".
    #[test]
    fn snappy_literal_and_copy() {
        // len 9; literal 3 "abc"; copy-2 len 6 offset 3.
        let b = [9, 2 << 2, b'a', b'b', b'c', (5 << 2) | 2, 3, 0];
        assert_eq!(snappy(&b).unwrap(), b"abcabcabc");
        assert!(snappy(&[9, 0, b'a']).is_none(), "too short");
    }

    fn varint(mut n: usize, out: &mut Vec<u8>) {
        while n >= 0x80 {
            out.push((n as u8) | 0x80);
            n >>= 7;
        }
        out.push(n as u8);
    }

    /// One uncompressed block with the given (key, value) entries, no prefix sharing, one restart.
    fn block(entries: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
        let mut b = Vec::new();
        for (k, v) in entries {
            varint(0, &mut b);
            varint(k.len(), &mut b);
            varint(v.len(), &mut b);
            b.extend_from_slice(k);
            b.extend_from_slice(v);
        }
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        b
    }

    fn internal(key: &[u8], seq: u64, kind: u64) -> Vec<u8> {
        [key, &(seq << 8 | kind).to_le_bytes()].concat()
    }

    /// A borrowed `Entry`.
    pub type RefEntry<'a> = (&'a [u8], u64, Option<&'a [u8]>);

    /// A minimal table file: one data block, an empty metaindex, the index and the footer.
    pub fn table_file(entries: &[RefEntry]) -> Vec<u8> {
        let data = block(&entries.iter().map(|(k, s, v)| (internal(k, *s, u64::from(v.is_some())), v.unwrap_or_default().to_vec())).collect::<Vec<_>>());
        let mut file = data.clone();
        file.extend_from_slice(&[0, 0, 0, 0, 0]);
        let handle = |off: usize, size: usize| {
            let mut h = Vec::new();
            varint(off, &mut h);
            varint(size, &mut h);
            h
        };
        let meta = block(&[]);
        let meta_at = file.len();
        file.extend_from_slice(&meta);
        file.extend_from_slice(&[0, 0, 0, 0, 0]);
        let index = block(&[(b"zz".to_vec(), handle(0, data.len()))]);
        let index_at = file.len();
        file.extend_from_slice(&index);
        file.extend_from_slice(&[0, 0, 0, 0, 0]);
        let mut footer = [handle(meta_at, meta.len()), handle(index_at, index.len())].concat();
        footer.resize(40, 0);
        footer.extend_from_slice(&0xdb4775248b80fb57u64.to_le_bytes());
        file.extend_from_slice(&footer);
        file
    }

    /// A log file with one batch.
    pub fn log_file(seq: u64, puts: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut batch = seq.to_le_bytes().to_vec();
        batch.extend_from_slice(&(puts.len() as u32).to_le_bytes());
        for (k, v) in puts {
            batch.push(1);
            varint(k.len(), &mut batch);
            batch.extend_from_slice(k);
            varint(v.len(), &mut batch);
            batch.extend_from_slice(v);
        }
        let mut f = vec![0, 0, 0, 0];
        f.extend_from_slice(&(batch.len() as u16).to_le_bytes());
        f.push(1);
        f.extend_from_slice(&batch);
        f
    }

    #[test]
    fn newest_wins_deletes_drop_and_keep_filters() {
        let dir = std::env::temp_dir().join(format!("scriber-ldb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("000001.ldb"), table_file(&[(&b"a"[..], 1, Some(&b"old"[..])), (&b"b"[..], 2, Some(&b"gone"[..])), (&b"c"[..], 3, Some(&b"c1"[..])), (&b"x"[..], 4, Some(&b"secret"[..]))])).unwrap();
        std::fs::write(dir.join("000002.ldb"), table_file(&[(&b"b"[..], 5, None)])).unwrap();
        std::fs::write(dir.join("000003.log"), log_file(10, &[(&b"a"[..], &b"new"[..])])).unwrap();
        let got = read_dir(&dir, &|k| k != b"x").unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(got.get(b"a".as_slice()).map(Vec::as_slice), Some(b"new".as_slice()));
        assert_eq!(got.get(b"c".as_slice()).map(Vec::as_slice), Some(b"c1".as_slice()));
        assert!(!got.contains_key(b"b".as_slice()), "deleted");
        assert!(!got.contains_key(b"x".as_slice()), "filtered out");
    }
}
