use reqwest::Client;
use serde::Deserialize;
use std::collections::HashSet;
use std::error::Error;

#[derive(Deserialize, Debug)]
pub struct BirdEyeToken {
    pub address: String,
}

#[derive(Deserialize, Debug)]
struct BirdEyeData {
    tokens: Vec<BirdEyeToken>,
}

#[derive(Deserialize, Debug)]
struct BirdEyeListResponse {
    data: BirdEyeData,
}

/// LoadBirdEyeTokenAddresses fetches up to maxTokens addresses from the Birdeye V3 token list (non-scroll API),
/// sorted by 24h volume USD descending, paginated by offset/limit.
/// apiKey is required for the X-API-KEY header.
pub async fn load_birdeye_token_addresses(
    api_key: &str,
    max_tokens: usize,
) -> Result<Vec<String>, Box<dyn Error>> {
    let base_url = "https://public-api.birdeye.so/defi/tokenlist?sort_by=v24hUSD&sort_type=desc&min_liquidity=10000&max_liquidity=2000000";
    let mut address_set = HashSet::new();
    let mut addresses = Vec::with_capacity(max_tokens);
    let limit = 50;
    let mut offset = 0;
    let client = Client::new();

    loop {
        let url = format!("{}&offset={}&limit={}", base_url, offset, limit);
        let res = client
            .get(&url)
            .header("accept", "application/json")
            .header("x-chain", "solana")
            .header("X-API-KEY", api_key)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let text = res
                .text()
                .await
                .unwrap_or_else(|_| "Could not read response body".to_string());
            eprintln!("Birdeye API request failed with status: {}", status);
            eprintln!("Response body: {}", text);
            return Err(format!("Birdeye API Error: {} - {}", status, text).into());
        }

        let api_resp: BirdEyeListResponse = res.json().await?;

        let tokens = api_resp.data.tokens;
        if tokens.is_empty() {
            break;
        }

        for token in tokens {
            if !token.address.is_empty() {
                if address_set.insert(token.address.clone()) {
                    addresses.push(token.address);
                    if addresses.len() >= max_tokens {
                        addresses.truncate(max_tokens);
                        return Ok(addresses);
                    }
                }
            }
        }

        offset += limit;
    }

    if addresses.len() > max_tokens {
        addresses.truncate(max_tokens);
    }
    println!("Loaded {} token addresses from Birdeye.", addresses.len());
    Ok(addresses)
}
