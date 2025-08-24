use serde::Deserialize;
use std::error::Error;

// Data structures for deserializing the DexScreener API response
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DexScreenerEntry {
    pub dex_id: String,
    #[serde(default)]
    pub labels: Vec<String>,
    pub pair_address: String,
    pub quote_token: QuoteToken,
    pub volume: Volume,
    pub liquidity: Liquidity,
    pub base_token: BaseToken,
}

pub type DexScreenerResponse = Vec<DexScreenerEntry>;

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuoteToken {
    pub address: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Volume {
    pub m5: f64,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Liquidity {
    pub usd: f64,
    pub quote: f64,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BaseToken {
    pub symbol: String,
}

// Internal representation of processed data
#[derive(Debug, Clone)]
pub struct DexPairData {
    pub symbol: String,
    pub token_ca: String,
    pub pumpswap: Vec<String>,
    pub raydium_cpmm: Vec<String>,
    pub meteora_amm: Vec<String>,
    pub meteora: Vec<String>,
    pub volume: f64,
}

pub async fn query_dexscreener(
    token_ca: &str,
    pool_vol_filter: f64,
    pool_liq_filter: f64,
    tot_volume_filter: f64,
) -> Result<Option<DexPairData>, Box<dyn Error>> {
    let wsol_mint = "So11111111111111111111111111111111111111112";
    let url = format!(
        "https://api.dexscreener.com/token-pairs/v1/solana/{}",
        token_ca
    );

    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await?;

    if !resp.status().is_success() {
        return Err(format!("DexScreener API Error: {}", resp.status()).into());
    }

    let body = resp.text().await?;
    let mut data: DexScreenerResponse = match serde_json::from_str(&body) {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "Failed to parse DexScreener JSON for {}: {}. Body: {}",
                token_ca, e, body
            );
            return Ok(None);
        }
    };

    if data.is_empty() {
        return Ok(None);
    }

    data.sort_by(|a, b| {
        b.liquidity
            .quote
            .partial_cmp(&a.liquidity.quote)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let valid_dex_ids: std::collections::HashSet<&str> =
        ["pumpswap", "raydium", "meteora"].iter().cloned().collect();
    let mut meteora = vec![];
    let mut raydium_cpmm = vec![];
    let mut pumpswap = vec![];
    let mut meteora_amm = vec![];
    let mut volumes = vec![];

    for entry in data.iter() {
        if valid_dex_ids.contains(entry.dex_id.as_str())
            && entry.quote_token.address == wsol_mint
            && entry.volume.m5 >= pool_vol_filter
            && entry.liquidity.quote >= pool_liq_filter
        {
            if entry.dex_id == "pumpswap" {
                pumpswap.push(entry.pair_address.clone());
            }

            if entry.dex_id == "raydium" && !entry.labels.is_empty() && entry.labels[0] == "CPMM" {
                raydium_cpmm.push(entry.pair_address.clone());
            }

            if entry.dex_id == "meteora" && !entry.labels.is_empty() && entry.labels[0] == "DLMM" {
                meteora.push(entry.pair_address.clone());
            }

            if entry.dex_id == "meteora" && !entry.labels.is_empty() && entry.labels[0] == "DYN" {
                meteora_amm.push(entry.pair_address.clone());
            }
            volumes.push(entry.volume.m5);
        }
    }

    let sum_vol: f64 = volumes.iter().sum();

    if sum_vol >= tot_volume_filter {
        let return_data = DexPairData {
            symbol: data[0].base_token.symbol.clone(),
            token_ca: token_ca.to_string(),
            pumpswap,
            raydium_cpmm,
            meteora_amm,
            meteora,
            volume: sum_vol,
        };

        println!("Processed DexScreener for {}:", return_data.symbol);
        println!("  Pumpfun pools: {:?}", return_data.pumpswap);
        println!("  Raydium pools: {:?}", return_data.raydium_cpmm);
        println!("  Meteora pools: {:?}", return_data.meteora);
        println!("  Meteora AMM pools: {:?}", return_data.meteora_amm);

        Ok(Some(return_data))
    } else {
        Ok(None)
    }
}
