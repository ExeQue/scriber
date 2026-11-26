//! Just enough of V8's value serializer (what Chromium stores in IndexedDB) to read plain JSON-like
//! values: objects, arrays, strings, numbers, booleans, null. Anything else fails the record.

use super::leveldb::Bytes;
use serde_json::{Map, Value};

/// Decode an IndexedDB value: a Blink envelope (`FF <version>` …) followed by V8's `FF <version>` and the value.
pub(super) fn decode(v: &[u8]) -> Option<Value> {
    // V8's header follows Blink's; search for it (versions 13–15 are current V8 formats).
    let start = (1..v.len().saturating_sub(1)).find(|&i| v[i] == 0xff && (13..=15).contains(&v[i + 1]) && v[..i].contains(&0xff))?;
    let mut r = Bytes::new(v);
    r.i = start + 2;
    read(&mut r, 0)
}

fn read(r: &mut Bytes, depth: u32) -> Option<Value> {
    if depth > 64 {
        return None;
    }
    let tag = loop {
        match r.byte()? {
            0 => continue, // padding
            t => break t,
        }
    };
    Some(match tag {
        b'_' | b'0' | b'-' => Value::Null,
        b'T' => Value::Bool(true),
        b'F' => Value::Bool(false),
        b'I' => {
            let z = r.varint()?;
            Value::from(((z >> 1) as i64) ^ -((z & 1) as i64))
        }
        b'U' => Value::from(r.varint()?),
        b'N' => Value::from(f64::from_le_bytes(r.take(8)?.try_into().ok()?)),
        b'"' => {
            let n = r.varint()? as usize;
            Value::String(r.take(n)?.iter().map(|&c| char::from(c)).collect())
        }
        b'S' => {
            let n = r.varint()? as usize;
            Value::String(String::from_utf8_lossy(r.take(n)?).into_owned())
        }
        b'c' => {
            let n = r.varint()? as usize;
            let units: Vec<u16> = r.take(n)?.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
            Value::String(String::from_utf16_lossy(&units))
        }
        b'o' => {
            let mut m = Map::new();
            loop {
                if r.b.get(r.i) == Some(&b'{') {
                    r.i += 1;
                    r.varint()?;
                    break;
                }
                let k = match read(r, depth + 1)? {
                    Value::String(s) => s,
                    other => other.to_string(),
                };
                m.insert(k, read(r, depth + 1)?);
            }
            Value::Object(m)
        }
        b'A' => {
            let n = r.varint()? as usize;
            let mut a = Vec::with_capacity(n.min(4096));
            for _ in 0..n {
                a.push(read(r, depth + 1)?);
            }
            // Extra named properties, then the end tag.
            while r.b.get(r.i) != Some(&b'$') {
                read(r, depth + 1)?;
                read(r, depth + 1)?;
            }
            r.i += 1;
            r.varint()?;
            r.varint()?;
            Value::Array(a)
        }
        b'a' => {
            let n = r.varint()? as usize;
            let mut a = vec![Value::Null; n.min(4096)];
            while r.b.get(r.i) != Some(&b'@') {
                let k = read(r, depth + 1)?;
                let v = read(r, depth + 1)?;
                if let Some(slot) = k.as_u64().and_then(|i| a.get_mut(i as usize)) {
                    *slot = v;
                }
            }
            r.i += 1;
            r.varint()?;
            r.varint()?;
            Value::Array(a)
        }
        b'D' => Value::from(f64::from_le_bytes(r.take(8)?.try_into().ok()?)),
        _ => return None,
    })
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use serde_json::json;

    /// A tiny V8 serializer for the test fixtures (objects, arrays, strings, numbers, bools, null).
    pub fn encode(v: &Value) -> Vec<u8> {
        fn varint(mut n: u64, out: &mut Vec<u8>) {
            while n >= 0x80 {
                out.push((n as u8) | 0x80);
                n >>= 7;
            }
            out.push(n as u8);
        }
        fn put(v: &Value, out: &mut Vec<u8>) {
            match v {
                Value::Null => out.push(b'0'),
                Value::Bool(b) => out.push(if *b { b'T' } else { b'F' }),
                Value::Number(n) if n.is_i64() => {
                    out.push(b'I');
                    let i = n.as_i64().unwrap();
                    varint(((i << 1) ^ (i >> 63)) as u64, out);
                }
                Value::Number(n) => {
                    out.push(b'N');
                    out.extend_from_slice(&n.as_f64().unwrap().to_le_bytes());
                }
                Value::String(s) if s.is_ascii() => {
                    out.push(b'"');
                    varint(s.len() as u64, out);
                    out.extend_from_slice(s.as_bytes());
                }
                Value::String(s) => {
                    let units: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
                    out.push(b'c');
                    varint(units.len() as u64, out);
                    out.extend_from_slice(&units);
                }
                Value::Array(a) => {
                    out.push(b'A');
                    varint(a.len() as u64, out);
                    a.iter().for_each(|x| put(x, out));
                    out.push(b'$');
                    varint(0, out);
                    varint(a.len() as u64, out);
                }
                Value::Object(m) => {
                    out.push(b'o');
                    for (k, x) in m {
                        put(&Value::String(k.clone()), out);
                        put(x, out);
                    }
                    out.push(b'{');
                    varint(m.len() as u64, out);
                }
            }
        }
        // IndexedDB version varint, Blink envelope (FF 14), then V8 (FF 0F).
        let mut out = vec![0x01, 0xff, 0x14, 0xff, 0x0f];
        put(v, &mut out);
        out
    }

    #[test]
    fn round_trip() {
        let v = json!({"id": 7, "name": {"en": "Test board", "ja": "テスト"}, "t": 2.5, "ok": true, "none": null, "list": [1, "two", {"x": -3}]});
        assert_eq!(decode(&encode(&v)).unwrap(), v);
    }

    #[test]
    fn sparse_array_and_garbage() {
        // a, length 2, [1] = "b", end @, props 1, length 2.
        let v = [0x01, 0xff, 0x14, 0xff, 0x0f, b'a', 2, b'I', 2, b'"', 1, b'b', b'@', 1, 2];
        assert_eq!(decode(&v).unwrap(), json!([null, "b"]));
        assert!(decode(&[0x01, 0xff, 0x14, 0xff, 0x0f, b'?']).is_none());
        assert!(decode(&[0x01, 0x02]).is_none());
    }
}
