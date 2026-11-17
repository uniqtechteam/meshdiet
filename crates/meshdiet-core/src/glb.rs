//! The GLB container: a 12-byte header, a JSON chunk and an optional BIN chunk.
//!
//! Only the header and the JSON chunk are read; the BIN chunk is located, not loaded.

use std::io::{Read, Seek, SeekFrom};

const MAGIC: &[u8; 4] = b"glTF";
const JSON: u32 = 0x4E4F_534A;
const BIN: u32 = 0x004E_4942;
/// JSON chunks bigger than this are refused; real ones are kilobytes to a few megabytes.
const MAX_JSON: u64 = 256 * 1024 * 1024;

/// Where the BIN chunk sits in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Byte offset of the chunk data in the file.
    pub offset: u64,
    /// Length of the chunk data.
    pub len: u64,
}

/// The parts of a GLB file.
#[derive(Debug)]
pub struct Container {
    /// The JSON chunk.
    pub json: Vec<u8>,
    /// The BIN chunk, if there is one.
    pub bin: Option<Span>,
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    let mut word = [0u8; 4];
    if let Some(src) = bytes.get(at..at + 4) {
        word.copy_from_slice(src);
    }
    u32::from_le_bytes(word)
}

fn describe_start(head: &[u8]) -> String {
    if head.first() == Some(&b'{') {
        return "it starts with `{`, so it looks like glTF JSON saved with a .glb name".to_owned();
    }
    let shown: String = head
        .iter()
        .take(4)
        .map(|b| {
            if b.is_ascii_graphic() {
                char::from(*b).to_string()
            } else {
                format!("\\x{b:02x}")
            }
        })
        .collect();
    format!("it starts with `{shown}` instead of `glTF`")
}

/// Read the container of a GLB file of `size` bytes.
///
/// # Errors
/// A sentence describing the first thing wrong with the container. Every error is final: the
/// file cannot be read past it.
pub fn read<R: Read + Seek>(file: &mut R, size: u64) -> Result<Container, String> {
    if size < 20 {
        return Err(format!(
            "the file is {size} bytes, too short to be a GLB (header and one chunk need 20)"
        ));
    }
    let mut head = [0u8; 20];
    file.read_exact(&mut head)
        .map_err(|e| format!("cannot read the header: {e}"))?;
    if &head[0..4] != MAGIC {
        return Err(format!("not a GLB file: {}", describe_start(&head)));
    }
    let version = u32_at(&head, 4);
    if version != 2 {
        return Err(format!(
            "GLB version {version}; only glTF 2.0 (version 2) is supported"
        ));
    }
    let declared = u64::from(u32_at(&head, 8));
    if declared != size {
        return Err(format!(
            "the header says the file is {declared} bytes but it has {size}{}",
            if declared > size {
                " (cut off during upload or copy?)"
            } else {
                ""
            }
        ));
    }
    let json_len = u64::from(u32_at(&head, 12));
    if u32_at(&head, 16) != JSON {
        return Err("the first chunk is not the JSON chunk".to_owned());
    }
    if 20 + json_len > size {
        return Err(format!(
            "the JSON chunk says it is {json_len} bytes but only {} are left in the file",
            size - 20
        ));
    }
    if json_len > MAX_JSON {
        return Err(format!(
            "the JSON chunk is {json_len} bytes, too big to read"
        ));
    }
    let mut json = vec![0u8; usize::try_from(json_len).unwrap_or(0)];
    file.read_exact(&mut json)
        .map_err(|e| format!("cannot read the JSON chunk: {e}"))?;
    let next = 20 + json_len;
    if size < next + 8 {
        return Ok(Container { json, bin: None });
    }
    let mut chunk = [0u8; 8];
    file.seek(SeekFrom::Start(next))
        .and_then(|_| file.read_exact(&mut chunk))
        .map_err(|e| format!("cannot read the second chunk: {e}"))?;
    let bin_len = u64::from(u32_at(&chunk, 0));
    if u32_at(&chunk, 4) != BIN {
        // unknown chunk types must be ignored by loaders
        return Ok(Container { json, bin: None });
    }
    let offset = next + 8;
    if offset + bin_len > size {
        return Err(format!(
            "the BIN chunk says it is {bin_len} bytes but only {} are left in the file",
            size - offset
        ));
    }
    Ok(Container {
        json,
        bin: Some(Span {
            offset,
            len: bin_len,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn glb(json: &[u8], bin: &[u8]) -> Vec<u8> {
        let total = 12 + 8 + json.len() + 8 + bin.len();
        let mut out = Vec::new();
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&u32::try_from(total).unwrap_or(0).to_le_bytes());
        out.extend_from_slice(&u32::try_from(json.len()).unwrap_or(0).to_le_bytes());
        out.extend_from_slice(&JSON.to_le_bytes());
        out.extend_from_slice(json);
        out.extend_from_slice(&u32::try_from(bin.len()).unwrap_or(0).to_le_bytes());
        out.extend_from_slice(&BIN.to_le_bytes());
        out.extend_from_slice(bin);
        out
    }

    fn parse(bytes: &[u8]) -> Result<Container, String> {
        read(&mut Cursor::new(bytes), bytes.len() as u64)
    }

    #[test]
    fn reads_json_and_locates_bin() {
        let c = parse(&glb(b"{\"a\":1} ", &[1, 2, 3, 4])).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(c.json, b"{\"a\":1} ");
        assert_eq!(c.bin, Some(Span { offset: 36, len: 4 }));
    }

    #[test]
    fn json_saved_as_glb_gets_a_hint() {
        let err = parse(b"{\"asset\":{\"version\":\"2.0\"}}")
            .err()
            .unwrap_or_default();
        assert!(err.contains("looks like glTF JSON"), "{err}");
    }

    #[test]
    fn version_1_is_refused() {
        let mut bytes = glb(b"{}  ", &[]);
        bytes[4] = 1;
        assert!(
            parse(&bytes)
                .err()
                .unwrap_or_default()
                .contains("version 1")
        );
    }

    #[test]
    fn bin_chunk_overrun_is_reported() {
        let mut bytes = glb(b"{}  ", &[0; 8]);
        let at = bytes.len() - 16;
        bytes[at..at + 4].copy_from_slice(&100u32.to_le_bytes());
        assert!(
            parse(&bytes)
                .err()
                .unwrap_or_default()
                .contains("BIN chunk")
        );
    }
}
