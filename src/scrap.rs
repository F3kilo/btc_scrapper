use reqwest::{Client, ClientBuilder, Response, Url, header};
use sqlx::types::{JsonValue, chrono};
use tokio::sync::Mutex;

use crate::{Price, PriceInfo};

const WEBSITE: &str = "https://www.blockchain.com/ru/explorer/assets/btc";

/// Query latest BTC price in USD.
pub async fn query_price(agent: &Agent) -> Result<Price, anyhow::Error> {
    tracing::info!("Querying price...");
    let mut resp = agent.request().await?;

    resp = if !resp.status().is_success() {
        agent.refresh_data().await?;
        agent.request().await?
    } else {
        resp
    };

    let resp = resp.error_for_status()?;

    let text = resp.text().await?;
    println!("{}", text);

    let price =
        find_price(text).ok_or_else(|| anyhow::Error::msg("Failed to find price in response."))?;

    Ok(Price {
        bitcoin: PriceInfo {
            usd: price,
            last_updated_at: chrono::Utc::now().timestamp_millis() as u64 / 1000,
        },
    })
}

/// Agent for making requests and bypass Cloudflare.
#[derive(Debug)]
pub struct Agent(Mutex<Client>);

impl Default for Agent {
    fn default() -> Self {
        Self(Client::builder().cookie_store(true).build().unwrap().into())
    }
}

impl Agent {
    /// Create new agent and refresh cookies.
    pub async fn new() -> anyhow::Result<Self> {
        let s = Self::default();
        s.refresh_data().await?;
        Ok(s)
    }

    /// Query price info, refreshing cookies if needed.
    pub async fn request(&self) -> Result<Response, anyhow::Error> {
        if let Err(e) = self.request_inner().await {
            tracing::debug!("Failed to query price info: {e}. Refresing data...");
            self.refresh_data().await?;
        }

        self.request_inner().await
    }

    async fn request_inner(&self) -> Result<Response, anyhow::Error> {
        tracing::info!("Performing request...");
        let response_result = self.0.lock().await.get(WEBSITE).send().await;
        tracing::info!("Request finished: {:?}.", response_result);

        let response = response_result?;
        Ok(response)
    }

    async fn refresh_data(&self) -> Result<(), anyhow::Error> {
        tracing::info!("Refreshing cookies...");
        let client = Client::new();
        let query = [("url", WEBSITE), ("retries", "5")];
        let response = client
            .get(Url::parse("http://localhost:8000/cookies")?)
            .query(&query)
            .send()
            .await?;

        let json: JsonValue = response.json().await?;

        let mut request_headers = header::HeaderMap::new();

        if let Some(json_cookies) = json
            .as_object()
            .and_then(|o| o.get("cookies"))
            .and_then(|c| c.as_object())
        {
            let mut cookies = Vec::new();
            for (k, v) in dbg!(json_cookies) {
                cookies.push(format!(
                    "{}={}",
                    k,
                    v.as_str().expect("Must return string cookie.")
                ));
            }
            let header_value: String = cookies.join(";");

            request_headers.insert(
                header::COOKIE,
                header::HeaderValue::from_str(&header_value)?,
            );
        };

        if let Some(agent) = json
            .as_object()
            .and_then(|o| o.get("user_agent"))
            .and_then(|a| a.as_str())
        {
            request_headers.insert(header::USER_AGENT, header::HeaderValue::from_str(agent)?);
        };

        tracing::info!("Refreshed cookies: {:?}", request_headers);

        *self.0.lock().await = ClientBuilder::new()
            .default_headers(request_headers)
            .cookie_store(true)
            .build()?;

        Ok(())
    }
}

fn find_price(text: String) -> Option<f64> {
    let start = dbg!(text.find(r#"{"name":"Bitcoin","price":"#))?;
    let tail = &text[start + 26..];
    dbg!(&tail[..42]);
    let price_str = tail
        .chars()
        .take_while(|c| c.is_digit(10))
        .collect::<String>();
    dbg!(&price_str);
    let price = price_str.parse::<f64>().ok()?;

    Some(price)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_query_price() {
        let agent = Agent::default();

        for i in 0..10 {
            println!("Try {i}");
            let price = query_price(&agent).await.unwrap();
            dbg!(&price);
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }

        assert!(false);
    }
}
