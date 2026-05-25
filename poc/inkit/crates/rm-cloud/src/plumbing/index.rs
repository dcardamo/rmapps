//! Index (de)serialization and the three reMarkable hash rules.
//!
//! Root index = schema 4; per-doc index = schema 3. Hash rules:
//! file hash = sha256(content); doc hash = sha256(concat raw file hashes sorted by id);
//! root hash = sha256(serialized root index bytes). See `docs/rm-cloud-protocol.md`.

use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// One file inside a document (a line of a per-doc index).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Logical file id, e.g. `<uuid>.metadata` or `<uuid>/<page>.rm`.
    pub id: String,
    /// sha256(content) hex.
    pub hash: String,
    /// Byte size of the content.
    pub size: u64,
}

/// One document (a line of the root index).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocEntry {
    /// Document UUID.
    pub id: String,
    /// Doc hash (`HashEntries`) hex — also the doc-index blob key.
    pub hash: String,
    /// Number of files in the document.
    pub num_files: u32,
    /// Sum of file sizes.
    pub size: u64,
}

/// Lowercase hex sha256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Doc hash = sha256(concat of hexdecoded file hashes, files sorted by id).
pub fn doc_hash(files: &[FileEntry]) -> String {
    let mut sorted: Vec<&FileEntry> = files.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let mut h = Sha256::new();
    for f in sorted {
        // File hashes are always crate-generated lowercase hex; a non-hex value is a
        // programmer error that would silently corrupt the doc hash, so fail loudly.
        let raw = hex::decode(&f.hash).expect("file hash must be valid hex");
        h.update(raw);
    }
    hex::encode(h.finalize())
}

/// Sum of file sizes (a doc's `size` field).
pub fn doc_size(files: &[FileEntry]) -> u64 {
    files.iter().map(|f| f.size).sum()
}

/// Serialize a per-doc index (schema 3). Files are sorted by id.
pub fn serialize_doc_index(files: &[FileEntry]) -> Vec<u8> {
    let mut sorted: Vec<&FileEntry> = files.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let mut s = String::from("3\n");
    for f in sorted {
        s.push_str(&format!("{}:0:{}:0:{}\n", f.hash, f.id, f.size));
    }
    s.into_bytes()
}

/// Serialize a root index (schema 4). Docs are sorted by id.
pub fn serialize_root_index(docs: &[DocEntry]) -> Vec<u8> {
    let mut sorted: Vec<&DocEntry> = docs.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let total: u64 = sorted.iter().map(|d| d.size).sum();
    let mut s = format!("4\n0:.:{}:{}\n", sorted.len(), total);
    for d in sorted {
        s.push_str(&format!(
            "{}:0:{}:{}:{}\n",
            d.hash, d.id, d.num_files, d.size
        ));
    }
    s.into_bytes()
}

/// Root hash = sha256(serialized root index bytes).
pub fn root_hash(docs: &[DocEntry]) -> String {
    sha256_hex(&serialize_root_index(docs))
}

/// Parse a per-doc index (schema 3). Returns files in id order.
pub fn parse_doc_index(bytes: &[u8]) -> Result<Vec<FileEntry>> {
    let text = std::str::from_utf8(bytes).map_err(|e| Error::Parse(e.to_string()))?;
    let mut lines = text.lines();
    let schema = lines
        .next()
        .ok_or_else(|| Error::Parse("empty doc index".into()))?;
    if schema != "3" && schema != "4" {
        return Err(Error::Parse(format!("unsupported doc schema {schema:?}")));
    }
    let mut out = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(':').collect();
        if f.len() != 5 {
            return Err(Error::Parse(format!(
                "doc line wrong field count: {line:?}"
            )));
        }
        out.push(FileEntry {
            hash: f[0].to_string(),
            id: f[2].to_string(),
            size: f[4]
                .parse()
                .map_err(|_| Error::Parse(format!("bad size: {line:?}")))?,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Parse a root index (schema 3 or 4). Returns docs in id order.
pub fn parse_root_index(bytes: &[u8]) -> Result<Vec<DocEntry>> {
    let text = std::str::from_utf8(bytes).map_err(|e| Error::Parse(e.to_string()))?;
    let mut lines = text.lines();
    let schema = lines
        .next()
        .ok_or_else(|| Error::Parse("empty root index".into()))?;
    let mut out = Vec::new();
    match schema {
        "4" => {
            let _ = lines.next(); // skip the "0:.:count:size" header line
        }
        "3" => {}
        other => return Err(Error::Parse(format!("unsupported root schema {other:?}"))),
    }
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(':').collect();
        if f.len() != 5 {
            return Err(Error::Parse(format!(
                "root line wrong field count: {line:?}"
            )));
        }
        out.push(DocEntry {
            hash: f[0].to_string(),
            id: f[2].to_string(),
            num_files: f[3]
                .parse()
                .map_err(|_| Error::Parse(format!("bad numFiles: {line:?}")))?,
            size: f[4]
                .parse()
                .map_err(|_| Error::Parse(format!("bad size: {line:?}")))?,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_vector() {
        // sha256("abc")
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn doc_hash_concats_sorted_raw_file_hashes() {
        // Two files; doc hash = sha256( raw(hashA) ++ raw(hashB) ) sorted by id.
        let fa = FileEntry {
            id: "b.file".into(),
            hash: sha256_hex(b"A"),
            size: 1,
        };
        let fb = FileEntry {
            id: "a.file".into(),
            hash: sha256_hex(b"B"),
            size: 1,
        };
        let got = doc_hash(&[fa.clone(), fb.clone()]);
        // expected: sort by id -> a.file (hash of "B"), then b.file (hash of "A")
        let mut h = sha2::Sha256::new();
        use sha2::Digest;
        h.update(hex::decode(sha256_hex(b"B")).unwrap());
        h.update(hex::decode(sha256_hex(b"A")).unwrap());
        assert_eq!(got, hex::encode(h.finalize()));
    }

    #[test]
    fn root_index_roundtrip() {
        let docs = vec![
            DocEntry {
                id: "zzz".into(),
                hash: "11".repeat(32),
                num_files: 2,
                size: 30,
            },
            DocEntry {
                id: "aaa".into(),
                hash: "22".repeat(32),
                num_files: 1,
                size: 10,
            },
        ];
        let bytes = serialize_root_index(&docs);
        let text = String::from_utf8(bytes.clone()).unwrap();
        // header + sorted-by-id docs
        assert!(text.starts_with("4\n0:.:2:40\n"));
        assert!(text.contains(
            "\n2222222222222222222222222222222222222222222222222222222222222222:0:aaa:1:10\n"
        ));
        let parsed = parse_root_index(&bytes).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "aaa"); // sorted
        assert_eq!(parsed[1].id, "zzz");
    }

    #[test]
    fn doc_index_roundtrip() {
        let files = vec![
            FileEntry {
                id: "x.content".into(),
                hash: "33".repeat(32),
                size: 5,
            },
            FileEntry {
                id: "x.metadata".into(),
                hash: "44".repeat(32),
                size: 7,
            },
        ];
        let bytes = serialize_doc_index(&files);
        assert!(bytes.starts_with(b"3\n"));
        let parsed = parse_doc_index(&bytes).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "x.content");
        assert_eq!(parsed[1].size, 7);
    }

    #[test]
    fn root_hash_is_sha256_of_index_text() {
        let docs = vec![DocEntry {
            id: "a".into(),
            hash: "ab".repeat(32),
            num_files: 1,
            size: 1,
        }];
        let bytes = serialize_root_index(&docs);
        assert_eq!(root_hash(&docs), sha256_hex(&bytes));
    }
}
