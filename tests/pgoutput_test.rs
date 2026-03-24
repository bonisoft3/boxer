use boxer::pgoutput::{parse_message, Column, PgoutputMessage};

/// Helper: build a Begin message in pgoutput binary format.
/// Layout: 'B' u64(final_lsn) i64(timestamp) u32(xid)
fn make_begin(final_lsn: u64, timestamp: i64, xid: u32) -> Vec<u8> {
    let mut buf = vec![b'B'];
    buf.extend_from_slice(&final_lsn.to_be_bytes());
    buf.extend_from_slice(&timestamp.to_be_bytes());
    buf.extend_from_slice(&xid.to_be_bytes());
    buf
}

/// Helper: build a Commit message.
/// Layout: 'C' u8(flags) u64(commit_lsn) u64(end_lsn) i64(timestamp)
fn make_commit(commit_lsn: u64, end_lsn: u64, timestamp: i64) -> Vec<u8> {
    let mut buf = vec![b'C'];
    buf.push(0u8); // flags
    buf.extend_from_slice(&commit_lsn.to_be_bytes());
    buf.extend_from_slice(&end_lsn.to_be_bytes());
    buf.extend_from_slice(&timestamp.to_be_bytes());
    buf
}

/// Helper: write a null-terminated C string into the buffer.
fn push_cstring(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(s.as_bytes());
    buf.push(0);
}

/// Helper: build a Relation message with the given columns.
/// Layout: 'R' u32(id) cstring(namespace) cstring(name) u8(replica_identity) u16(n_cols)
///         per column: u8(flags) cstring(name) u32(type_oid) i32(type_modifier)
fn make_relation(
    id: u32,
    namespace: &str,
    name: &str,
    replica_identity: u8,
    columns: &[(u8, &str, u32, i32)],
) -> Vec<u8> {
    let mut buf = vec![b'R'];
    buf.extend_from_slice(&id.to_be_bytes());
    push_cstring(&mut buf, namespace);
    push_cstring(&mut buf, name);
    buf.push(replica_identity);
    buf.extend_from_slice(&(columns.len() as u16).to_be_bytes());
    for &(flags, col_name, type_oid, type_modifier) in columns {
        buf.push(flags);
        push_cstring(&mut buf, col_name);
        buf.extend_from_slice(&type_oid.to_be_bytes());
        buf.extend_from_slice(&type_modifier.to_be_bytes());
    }
    buf
}

/// Helper: build TupleData from a slice of Option<&str>.
/// Layout: u16(n_cols) per column: 'n' | ('t' i32(len) bytes)
fn push_tuple_data(buf: &mut Vec<u8>, values: &[Option<&str>]) {
    buf.extend_from_slice(&(values.len() as u16).to_be_bytes());
    for val in values {
        match val {
            None => buf.push(b'n'),
            Some(s) => {
                buf.push(b't');
                buf.extend_from_slice(&(s.len() as i32).to_be_bytes());
                buf.extend_from_slice(s.as_bytes());
            }
        }
    }
}

/// Helper: build an Insert message.
/// Layout: 'I' u32(relation_id) 'N' TupleData
fn make_insert(relation_id: u32, values: &[Option<&str>]) -> Vec<u8> {
    let mut buf = vec![b'I'];
    buf.extend_from_slice(&relation_id.to_be_bytes());
    buf.push(b'N');
    push_tuple_data(&mut buf, values);
    buf
}

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

#[test]
fn parse_begin_message() {
    let data = make_begin(0x1234, 1_000_000, 42);
    let msg = parse_message(&data).unwrap();
    assert_eq!(
        msg,
        PgoutputMessage::Begin {
            final_lsn: 0x1234,
            timestamp: 1_000_000,
            xid: 42,
        }
    );
}

#[test]
fn parse_commit_message() {
    let data = make_commit(0xABCD, 0x1234, 2_000_000);
    let msg = parse_message(&data).unwrap();
    assert_eq!(
        msg,
        PgoutputMessage::Commit {
            commit_lsn: 0xABCD,
            end_lsn: 0x1234,
            timestamp: 2_000_000,
        }
    );
}

#[test]
fn parse_relation_message() {
    let columns = vec![
        (0u8, "id", 23u32, -1i32),          // int4, OID 23
        (0u8, "imageUrl", 25u32, -1i32),     // text, OID 25
    ];
    let data = make_relation(16385, "public", "items", b'd', &columns);
    let msg = parse_message(&data).unwrap();
    assert_eq!(
        msg,
        PgoutputMessage::Relation {
            id: 16385,
            namespace: "public".to_string(),
            name: "items".to_string(),
            replica_identity: b'd',
            columns: vec![
                Column { flags: 0, name: "id".to_string(), type_oid: 23, type_modifier: -1 },
                Column { flags: 0, name: "imageUrl".to_string(), type_oid: 25, type_modifier: -1 },
            ],
        }
    );
}

#[test]
fn parse_insert_message() {
    let data = make_insert(16385, &[Some("abc-123"), None]);
    let msg = parse_message(&data).unwrap();
    assert_eq!(
        msg,
        PgoutputMessage::Insert {
            relation_id: 16385,
            values: vec![Some("abc-123".to_string()), None],
        }
    );
}

#[test]
fn parse_insert_empty_text_value() {
    let data = make_insert(1, &[Some("")]);
    let msg = parse_message(&data).unwrap();
    assert_eq!(
        msg,
        PgoutputMessage::Insert {
            relation_id: 1,
            values: vec![Some("".to_string())],
        }
    );
}

#[test]
fn parse_unknown_message_returns_other() {
    let data = vec![b'Z', 0xFF];
    let msg = parse_message(&data).unwrap();
    assert_eq!(msg, PgoutputMessage::Other(b'Z'));
}

#[test]
fn parse_empty_message_returns_error() {
    let result = parse_message(&[]);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
}

#[test]
fn parse_truncated_begin_returns_error() {
    // Begin needs 20 bytes after the tag; supply only 4.
    let data = vec![b'B', 0x00, 0x00, 0x00, 0x01];
    let result = parse_message(&data);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::UnexpectedEof);
}
