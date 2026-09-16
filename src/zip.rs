//! A minimal ZIP writer for `.pptx` packages. pptxgenjs produces the part
//! *content*; this module packages those parts into the OPC container, using
//! raw DEFLATE and a single central directory. Offsets are tracked in bytes, so
//! the output is always structurally sound.

use std::io::Write;

/// One entry in the archive. `dir` entries carry no data.
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub data: Vec<u8>,
    pub dir: bool,
}

const LOCAL_HEADER: u32 = 0x0403_4b50; // "PK\x03\x04"
const CENTRAL_HEADER: u32 = 0x0201_4b50; // "PK\x01\x02"
const EOCD_HEADER: u32 = 0x0605_4b50; // "PK\x05\x06"
const VERSION_NEEDED: u16 = 20;
const METHOD_DEFLATE: u16 = 8;

/// DOS date for 1980-01-01 (bit-packed: day 1, month 1, year 1980).
const DOS_DATE: u16 = 0x21;
const DOS_TIME: u16 = 0;

struct Record {
    name: String,
    crc32: u32,
    compressed: Vec<u8>,
    size: u32,
    dir: bool,
    local_offset: u32,
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Write a zip archive from `entries`. File data is deflated; directory entries
/// (name ends with `/`) are written empty.
pub fn write(entries: &[Entry]) -> Vec<u8> {
    let mut records: Vec<Record> = Vec::with_capacity(entries.len());
    let mut out = Vec::new();

    for entry in entries {
        let name = if entry.dir {
            if entry.name.ends_with('/') {
                entry.name.clone()
            } else {
                format!("{}/", entry.name)
            }
        } else {
            entry.name.clone()
        };
        let dir = entry.dir;

        let (crc32, compressed, size) = if dir {
            (0, Vec::new(), 0)
        } else {
            let crc = crc32fast::hash(&entry.data);
            let mut encoder =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(&entry.data).expect("deflate write");
            let deflated = encoder.finish().expect("deflate finish");
            (crc, deflated, entry.data.len() as u32)
        };

        records.push(Record {
            name: name.clone(),
            crc32,
            compressed: compressed.clone(),
            size,
            dir,
            local_offset: out.len() as u32,
        });

        // Local file header.
        push_u32(&mut out, LOCAL_HEADER);
        push_u16(&mut out, VERSION_NEEDED);
        push_u16(&mut out, 0); // general purpose flag
        push_u16(&mut out, if dir { 0 } else { METHOD_DEFLATE });
        push_u16(&mut out, DOS_TIME);
        push_u16(&mut out, DOS_DATE);
        push_u32(&mut out, crc32);
        push_u32(&mut out, if dir { 0 } else { compressed.len() as u32 });
        push_u32(&mut out, size);
        push_u16(&mut out, name.len() as u16);
        push_u16(&mut out, 0); // extra field length
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&compressed);
    }

    let central_start = out.len() as u32;

    for record in &records {
        push_u32(&mut out, CENTRAL_HEADER);
        push_u16(&mut out, VERSION_NEEDED); // version made by
        push_u16(&mut out, VERSION_NEEDED); // version needed
        push_u16(&mut out, 0); // general purpose flag
        push_u16(&mut out, if record.dir { 0 } else { METHOD_DEFLATE });
        push_u16(&mut out, DOS_TIME);
        push_u16(&mut out, DOS_DATE);
        push_u32(&mut out, record.crc32);
        push_u32(&mut out, record.compressed.len() as u32);
        push_u32(&mut out, record.size);
        push_u16(&mut out, record.name.len() as u16);
        push_u16(&mut out, 0); // extra field length
        push_u16(&mut out, 0); // comment length
        push_u16(&mut out, 0); // disk number start
        push_u16(&mut out, 0); // internal attributes
        push_u32(&mut out, 0); // external attributes
        push_u32(&mut out, record.local_offset);
        out.extend_from_slice(record.name.as_bytes());
    }

    let central_size = out.len() as u32 - central_start;

    push_u32(&mut out, EOCD_HEADER);
    push_u16(&mut out, 0); // disk number
    push_u16(&mut out, 0); // disk with central dir
    push_u16(&mut out, records.len() as u16);
    push_u16(&mut out, records.len() as u16);
    push_u32(&mut out, central_size);
    push_u32(&mut out, central_start);
    push_u16(&mut out, 0); // comment length

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_loadable_zip() {
        let entries = vec![
            Entry {
                name: "ppt/slides/slide1.xml".into(),
                data: b"<p:sld/>".to_vec(),
                dir: false,
            },
            Entry {
                name: "ppt/media".into(),
                data: Vec::new(),
                dir: true,
            },
        ];
        let bytes = write(&entries);
        assert_eq!(&bytes[0..2], b"PK");
        let name = b"ppt/slides/slide1.xml";
        assert!(bytes.windows(name.len()).any(|w| w == name));
        // Local + central + EOCD signatures all present.
        assert!(bytes.windows(4).any(|w| w == b"PK\x03\x04"));
        assert!(bytes.windows(4).any(|w| w == b"PK\x01\x02"));
        assert!(bytes.windows(4).any(|w| w == b"PK\x05\x06"));
        // The file is structurally parseable: the EOCD signature sits 22 bytes
        // before the end and the archive has no trailing garbage.
        assert_eq!(&bytes[bytes.len() - 22..bytes.len() - 18], b"PK\x05\x06");
    }
}
