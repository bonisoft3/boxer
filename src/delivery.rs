use reqwest::Client;
use std::time::Duration;
use tracing::{error, info, warn};

pub struct Deliverer {
    client: Client,
    url: String,
    max_retries: u32,
    initial_delay: Duration,
}

#[derive(Debug)]
pub enum DeliveryError {
    Exhausted,
}

impl std::fmt::Display for DeliveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "delivery exhausted all retries")
    }
}

impl std::error::Error for DeliveryError {}

impl Deliverer {
    pub fn new(url: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("failed to build HTTP client");
        Self {
            client,
            url,
            max_retries: 10,
            initial_delay: Duration::from_secs(2),
        }
    }

    pub async fn deliver(&self, payload: &serde_json::Value) -> Result<(), DeliveryError> {
        let mut delay = self.initial_delay;
        for attempt in 1..=self.max_retries {
            match self.client.post(&self.url).json(payload).send().await {
                Ok(resp) if resp.status().is_success() => {
                    info!(attempt, status = %resp.status(), "delivery succeeded");
                    return Ok(());
                }
                Ok(resp) => warn!(attempt, status = %resp.status(), "delivery rejected"),
                Err(e) => warn!(attempt, error = %e, "delivery failed"),
            }
            if attempt < self.max_retries {
                info!(delay_secs = delay.as_secs(), "retrying after backoff");
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(60));
            }
        }
        error!(max_retries = self.max_retries, url = %self.url, "delivery exhausted -- process should crash for WAL replay");
        Err(DeliveryError::Exhausted)
    }
}
