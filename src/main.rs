use boxer::delivery::Deliverer;
use boxer::pgoutput::{self, Column, PgoutputMessage};

use pgwire_replication::{Lsn, ReplicationClient, ReplicationConfig, ReplicationEvent, TlsConfig};

use std::collections::HashMap;
use std::env;
use std::time::Duration;

use tracing::{error, info, warn};

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

struct Config {
    database_url: String,
    delivery_url: String,
    slot: String,
    publication: String,
    table: String,
    health_port: u16,
}

impl Config {
    fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let database_url =
            env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required but not set")?;
        let delivery_url = env::var("BOXER_DELIVERY_URL")
            .map_err(|_| "BOXER_DELIVERY_URL is required but not set")?;
        let slot = env::var("BOXER_SLOT").unwrap_or_else(|_| "boxer_slot".into());
        let publication = env::var("BOXER_PUBLICATION").unwrap_or_else(|_| "boxer_pub".into());
        let table = env::var("BOXER_TABLE").unwrap_or_else(|_| "trackrequest".into());
        let health_port: u16 = env::var("BOXER_HEALTH_PORT")
            .unwrap_or_else(|_| "8080".into())
            .parse()
            .map_err(|e| format!("invalid BOXER_HEALTH_PORT: {e}"))?;

        Ok(Self {
            database_url,
            delivery_url,
            slot,
            publication,
            table,
            health_port,
        })
    }
}

// ---------------------------------------------------------------------------
// Parse DATABASE_URL into ReplicationConfig
// ---------------------------------------------------------------------------

fn parse_database_url(
    database_url: &str,
    slot: &str,
    publication: &str,
) -> Result<ReplicationConfig, Box<dyn std::error::Error>> {
    let parsed = url::Url::parse(database_url)?;

    let host = parsed
        .host_str()
        .ok_or("DATABASE_URL missing host")?
        .to_string();
    let port = parsed.port().unwrap_or(5432);
    let user = parsed.username().to_string();
    let password = parsed.password().unwrap_or("").to_string();
    let database = parsed.path().trim_start_matches('/').to_string();

    if database.is_empty() {
        return Err("DATABASE_URL missing database name".into());
    }

    // Check sslmode query parameter
    let sslmode = parsed
        .query_pairs()
        .find(|(k, _)| k == "sslmode")
        .map(|(_, v)| v.to_string())
        .unwrap_or_default();

    let tls = match sslmode.as_str() {
        "require" => TlsConfig::require(),
        "verify-ca" => TlsConfig::verify_ca(None),
        "verify-full" => TlsConfig::verify_full(None),
        _ => TlsConfig::disabled(),
    };

    Ok(ReplicationConfig {
        host,
        port,
        user,
        password,
        database,
        tls,
        slot: slot.into(),
        publication: publication.into(),
        start_lsn: Lsn::ZERO,
        stop_at_lsn: None,
        status_interval: Duration::from_secs(10),
        idle_wakeup_interval: Duration::from_secs(10),
        buffer_events: 8192,
    })
}

// ---------------------------------------------------------------------------
// row_to_json
// ---------------------------------------------------------------------------

pub fn row_to_json(columns: &[Column], values: &[Option<String>]) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (col, val) in columns.iter().zip(values.iter()) {
        let json_val = match val {
            Some(s) => serde_json::Value::String(s.clone()),
            None => serde_json::Value::Null,
        };
        map.insert(col.name.clone(), json_val);
    }
    serde_json::Value::Object(map)
}

// ---------------------------------------------------------------------------
// WAL consumer
// ---------------------------------------------------------------------------

async fn consume_wal(
    config: ReplicationConfig,
    table_filter: &str,
    deliverer: &Deliverer,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        host = %config.host,
        port = config.port,
        slot = %config.slot,
        publication = %config.publication,
        "connecting to Postgres replication"
    );

    let mut client = ReplicationClient::connect(config).await?;

    // Relation metadata cache: relation_id -> (table_name, columns)
    let mut relations: HashMap<u32, (String, Vec<Column>)> = HashMap::new();

    // Pending deliveries for the current transaction
    let mut pending: Vec<serde_json::Value> = Vec::new();

    while let Some(event) = client.recv().await? {
        match event {
            ReplicationEvent::Begin { .. } => {
                pending.clear();
            }

            ReplicationEvent::XLogData {
                wal_end, data, ..
            } => {
                let msg = pgoutput::parse_message(&data)?;

                match msg {
                    PgoutputMessage::Begin { .. } => {
                        // Already handled by the Begin event above, but clear
                        // pending in case the crate emits it here too.
                        pending.clear();
                    }

                    PgoutputMessage::Relation {
                        id,
                        name,
                        columns,
                        ..
                    } => {
                        relations.insert(id, (name, columns));
                    }

                    PgoutputMessage::Insert {
                        relation_id,
                        values,
                    } => {
                        if let Some((table_name, columns)) = relations.get(&relation_id) {
                            if table_name == table_filter {
                                let json = row_to_json(columns, &values);
                                pending.push(json);
                            }
                        } else {
                            warn!(
                                relation_id,
                                "received Insert for unknown relation, skipping"
                            );
                        }
                    }

                    PgoutputMessage::Commit { .. } => {
                        // Deliver pending items and advance LSN
                        if !pending.is_empty() {
                            for payload in &pending {
                                deliverer.deliver(payload).await?;
                            }
                            info!(
                                count = pending.len(),
                                wal_end = %wal_end,
                                "delivered transaction"
                            );
                            pending.clear();
                        }
                        client.update_applied_lsn(wal_end);
                    }

                    PgoutputMessage::Update { .. }
                    | PgoutputMessage::Delete { .. }
                    | PgoutputMessage::Other(_) => {
                        // For non-insert DML with no pending deliveries,
                        // advance LSN to prevent slot bloat.
                        if pending.is_empty() {
                            client.update_applied_lsn(wal_end);
                        }
                    }
                }
            }

            ReplicationEvent::Commit { end_lsn, .. } => {
                // The crate can also emit Commit as a top-level event.
                // Deliver any remaining pending items.
                if !pending.is_empty() {
                    for payload in &pending {
                        deliverer.deliver(payload).await?;
                    }
                    info!(
                        count = pending.len(),
                        wal_end = %end_lsn,
                        "delivered transaction (commit event)"
                    );
                    pending.clear();
                }
                client.update_applied_lsn(end_lsn);
            }

            ReplicationEvent::KeepAlive { .. } => {
                // pgwire-replication handles keepalive responses internally.
            }

            ReplicationEvent::Message { .. } => {
                // Logical decoding messages -- not used by boxer.
            }

            ReplicationEvent::StoppedAt { reached } => {
                info!(lsn = %reached, "replication stopped at target LSN");
                break;
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Health endpoint
// ---------------------------------------------------------------------------

async fn run_health_server(port: u16) {
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    let addr = format!("0.0.0.0:{port}");
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => {
            info!(addr = %addr, "health endpoint listening");
            l
        }
        Err(e) => {
            error!(error = %e, addr = %addr, "failed to bind health endpoint");
            return;
        }
    };

    loop {
        match listener.accept().await {
            Ok((mut stream, _)) => {
                let body = r#"{"status":"ok"}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
            Err(e) => {
                warn!(error = %e, "health endpoint accept error");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    // Init structured JSON logging
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cfg = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "configuration error");
            std::process::exit(1);
        }
    };

    info!(
        slot = %cfg.slot,
        publication = %cfg.publication,
        table = %cfg.table,
        health_port = cfg.health_port,
        "boxer starting"
    );

    // Spawn health endpoint
    tokio::spawn(run_health_server(cfg.health_port));

    let deliverer = Deliverer::new(cfg.delivery_url.clone());

    // Reconnection loop
    loop {
        let repl_config = match parse_database_url(&cfg.database_url, &cfg.slot, &cfg.publication) {
            Ok(c) => c,
            Err(e) => {
                error!(error = %e, "failed to parse DATABASE_URL");
                std::process::exit(1);
            }
        };

        match consume_wal(repl_config, &cfg.table, &deliverer).await {
            Ok(()) => {
                info!("WAL consumer exited cleanly, reconnecting");
            }
            Err(e) => {
                error!(error = %e, "WAL consumer error, reconnecting in 5s");
            }
        }

        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use boxer::pgoutput::Column;

    #[test]
    fn test_row_to_json_basic() {
        let columns = vec![
            Column {
                flags: 0,
                name: "id".into(),
                type_oid: 23,
                type_modifier: -1,
            },
            Column {
                flags: 0,
                name: "name".into(),
                type_oid: 25,
                type_modifier: -1,
            },
        ];
        let values = vec![Some("42".into()), Some("hello".into())];
        let json = row_to_json(&columns, &values);

        assert_eq!(json["id"], "42");
        assert_eq!(json["name"], "hello");
    }

    #[test]
    fn test_row_to_json_with_null() {
        let columns = vec![
            Column {
                flags: 0,
                name: "a".into(),
                type_oid: 25,
                type_modifier: -1,
            },
            Column {
                flags: 0,
                name: "b".into(),
                type_oid: 25,
                type_modifier: -1,
            },
        ];
        let values = vec![Some("val".into()), None];
        let json = row_to_json(&columns, &values);

        assert_eq!(json["a"], "val");
        assert!(json["b"].is_null());
    }

    #[test]
    fn test_parse_database_url_basic() {
        let config = parse_database_url(
            "postgresql://user:pass@host:5432/mydb",
            "my_slot",
            "my_pub",
        )
        .unwrap();

        assert_eq!(config.host, "host");
        assert_eq!(config.port, 5432);
        assert_eq!(config.user, "user");
        assert_eq!(config.password, "pass");
        assert_eq!(config.database, "mydb");
        assert_eq!(config.slot, "my_slot");
        assert_eq!(config.publication, "my_pub");
        assert!(!config.tls.mode.requires_tls());
    }

    #[test]
    fn test_parse_database_url_sslmode_require() {
        let config = parse_database_url(
            "postgresql://u:p@h:5432/db?sslmode=require",
            "slot",
            "pub",
        )
        .unwrap();

        assert!(config.tls.mode.requires_tls());
        assert!(!config.tls.mode.verifies_certificate());
    }

    #[test]
    fn test_parse_database_url_sslmode_verify_full() {
        let config = parse_database_url(
            "postgresql://u:p@h:5432/db?sslmode=verify-full",
            "slot",
            "pub",
        )
        .unwrap();

        assert!(config.tls.mode.requires_tls());
        assert!(config.tls.mode.verifies_hostname());
    }

    #[test]
    fn test_parse_database_url_default_port() {
        let config =
            parse_database_url("postgresql://u:p@host/db", "slot", "pub").unwrap();

        assert_eq!(config.port, 5432);
    }

    #[test]
    fn test_parse_database_url_missing_db() {
        let result = parse_database_url("postgresql://u:p@host/", "slot", "pub");
        assert!(result.is_err());
    }
}
