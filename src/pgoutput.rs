use std::io::{self, Cursor, Read};

/// A column definition from a Relation message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub flags: u8,
    pub name: String,
    pub type_oid: u32,
    pub type_modifier: i32,
}

/// A parsed pgoutput logical replication message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PgoutputMessage {
    Begin {
        final_lsn: u64,
        timestamp: i64,
        xid: u32,
    },
    Commit {
        commit_lsn: u64,
        end_lsn: u64,
        timestamp: i64,
    },
    Relation {
        id: u32,
        namespace: String,
        name: String,
        replica_identity: u8,
        columns: Vec<Column>,
    },
    Insert {
        relation_id: u32,
        values: Vec<Option<String>>,
    },
    Update {
        relation_id: u32,
        values: Vec<Option<String>>,
    },
    Delete {
        relation_id: u32,
    },
    Other(u8),
}

/// Parse a pgoutput binary message from the given byte slice.
///
/// The binary format uses big-endian integers and null-terminated C strings.
/// The first byte identifies the message type.
pub fn parse_message(data: &[u8]) -> io::Result<PgoutputMessage> {
    if data.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "empty message",
        ));
    }

    let tag = data[0];
    let mut cur = Cursor::new(&data[1..]);

    match tag {
        b'B' => parse_begin(&mut cur),
        b'C' => parse_commit(&mut cur),
        b'R' => parse_relation(&mut cur),
        b'I' => parse_insert(&mut cur),
        b'U' => parse_update(&mut cur),
        b'D' => parse_delete(&mut cur),
        other => Ok(PgoutputMessage::Other(other)),
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn read_u8(cur: &mut Cursor<&[u8]>) -> io::Result<u8> {
    let mut buf = [0u8; 1];
    cur.read_exact(&mut buf)?;
    Ok(buf[0])
}

fn read_u16(cur: &mut Cursor<&[u8]>) -> io::Result<u16> {
    let mut buf = [0u8; 2];
    cur.read_exact(&mut buf)?;
    Ok(u16::from_be_bytes(buf))
}

fn read_u32(cur: &mut Cursor<&[u8]>) -> io::Result<u32> {
    let mut buf = [0u8; 4];
    cur.read_exact(&mut buf)?;
    Ok(u32::from_be_bytes(buf))
}

fn read_i32(cur: &mut Cursor<&[u8]>) -> io::Result<i32> {
    let mut buf = [0u8; 4];
    cur.read_exact(&mut buf)?;
    Ok(i32::from_be_bytes(buf))
}

fn read_u64(cur: &mut Cursor<&[u8]>) -> io::Result<u64> {
    let mut buf = [0u8; 8];
    cur.read_exact(&mut buf)?;
    Ok(u64::from_be_bytes(buf))
}

fn read_i64(cur: &mut Cursor<&[u8]>) -> io::Result<i64> {
    let mut buf = [0u8; 8];
    cur.read_exact(&mut buf)?;
    Ok(i64::from_be_bytes(buf))
}

/// Read a null-terminated C string (excludes the trailing 0x00).
fn read_cstring(cur: &mut Cursor<&[u8]>) -> io::Result<String> {
    let mut bytes = Vec::new();
    loop {
        let b = read_u8(cur)?;
        if b == 0 {
            break;
        }
        bytes.push(b);
    }
    String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Parse TupleData: u16 n_cols, then per column one of 'n'(null), 'u'(unchanged TOAST), 't'(text).
fn read_tuple_data(cur: &mut Cursor<&[u8]>) -> io::Result<Vec<Option<String>>> {
    let n_cols = read_u16(cur)?;
    let mut values = Vec::with_capacity(n_cols as usize);
    for _ in 0..n_cols {
        let col_type = read_u8(cur)?;
        match col_type {
            b'n' => values.push(None),
            b'u' => values.push(None), // unchanged TOAST — treat as null for consumers
            b't' => {
                let len = read_i32(cur)? as usize;
                let mut buf = vec![0u8; len];
                cur.read_exact(&mut buf)?;
                let s = String::from_utf8(buf)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                values.push(Some(s));
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unknown tuple column type: 0x{other:02X}"),
                ));
            }
        }
    }
    Ok(values)
}

// ---------------------------------------------------------------------------
// Message parsers
// ---------------------------------------------------------------------------

fn parse_begin(cur: &mut Cursor<&[u8]>) -> io::Result<PgoutputMessage> {
    let final_lsn = read_u64(cur)?;
    let timestamp = read_i64(cur)?;
    let xid = read_u32(cur)?;
    Ok(PgoutputMessage::Begin {
        final_lsn,
        timestamp,
        xid,
    })
}

fn parse_commit(cur: &mut Cursor<&[u8]>) -> io::Result<PgoutputMessage> {
    let _flags = read_u8(cur)?;
    let commit_lsn = read_u64(cur)?;
    let end_lsn = read_u64(cur)?;
    let timestamp = read_i64(cur)?;
    Ok(PgoutputMessage::Commit {
        commit_lsn,
        end_lsn,
        timestamp,
    })
}

fn parse_relation(cur: &mut Cursor<&[u8]>) -> io::Result<PgoutputMessage> {
    let id = read_u32(cur)?;
    let namespace = read_cstring(cur)?;
    let name = read_cstring(cur)?;
    let replica_identity = read_u8(cur)?;
    let n_cols = read_u16(cur)?;

    let mut columns = Vec::with_capacity(n_cols as usize);
    for _ in 0..n_cols {
        let flags = read_u8(cur)?;
        let col_name = read_cstring(cur)?;
        let type_oid = read_u32(cur)?;
        let type_modifier = read_i32(cur)?;
        columns.push(Column {
            flags,
            name: col_name,
            type_oid,
            type_modifier,
        });
    }

    Ok(PgoutputMessage::Relation {
        id,
        namespace,
        name,
        replica_identity,
        columns,
    })
}

fn parse_insert(cur: &mut Cursor<&[u8]>) -> io::Result<PgoutputMessage> {
    let relation_id = read_u32(cur)?;
    let marker = read_u8(cur)?;
    if marker != b'N' {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected 'N' marker in Insert, got 0x{marker:02X}"),
        ));
    }
    let values = read_tuple_data(cur)?;
    Ok(PgoutputMessage::Insert {
        relation_id,
        values,
    })
}

fn parse_update(cur: &mut Cursor<&[u8]>) -> io::Result<PgoutputMessage> {
    let relation_id = read_u32(cur)?;

    // The next byte is either 'O' (old tuple), 'K' (key), or 'N' (new tuple).
    // If 'O' or 'K', skip the old tuple data and then read the 'N' new tuple.
    let marker = read_u8(cur)?;
    match marker {
        b'N' => {
            let values = read_tuple_data(cur)?;
            Ok(PgoutputMessage::Update {
                relation_id,
                values,
            })
        }
        b'O' | b'K' => {
            // Skip old/key tuple data
            let _old = read_tuple_data(cur)?;
            let new_marker = read_u8(cur)?;
            if new_marker != b'N' {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "expected 'N' marker after old tuple in Update, got 0x{new_marker:02X}",
                    ),
                ));
            }
            let values = read_tuple_data(cur)?;
            Ok(PgoutputMessage::Update {
                relation_id,
                values,
            })
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected marker in Update: 0x{other:02X}"),
        )),
    }
}

fn parse_delete(cur: &mut Cursor<&[u8]>) -> io::Result<PgoutputMessage> {
    let relation_id = read_u32(cur)?;
    // Skip remaining bytes (old key/tuple data) — caller only needs relation_id.
    Ok(PgoutputMessage::Delete { relation_id })
}
