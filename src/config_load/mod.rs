use once_cell::sync::OnceCell;
use serde::Deserialize;
use std::fs;

pub static GLOBAL_CONFIG: OnceCell<Config> = OnceCell::new();
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(rename = "grpcEndpoint")]
    pub grpc_endpoint: String,
    #[serde(rename = "arpcEndpoint")]
    pub arpc_endpoint: String,
    #[serde(rename = "rpcEndpoint")]
    pub rpc_endpoint: String,
    #[serde(rename = "sendRPC")]
    pub send_rpc: Vec<String>,
    #[serde(rename = "zeroSlotUrl")]
    pub zero_slot_url: String,
    #[serde(rename = "zeroslot_buy_tip")]
    pub zeroslot_buy_tip: f64,
    #[serde(rename = "zeroslot_sell_tip")]
    pub zeroslot_sell_tip: f64,
    #[serde(rename = "cuPrice0Slot")]
    pub cu_price0_slot: u64,
    #[serde(rename = "jitoUrl")]
    pub jito_url: String,
    #[serde(rename = "jitoTip")]
    pub jito_tip: f64,
    #[serde(rename = "cuPriceJito")]
    pub cu_price_jito: u64,
    #[serde(rename = "jitoBlockEngine")]
    pub jito_block_engine: String,
    #[serde(rename = "solFilter")]
    pub sol_filter: f64,
    #[serde(rename = "cuPrice")]
    pub cu_price: u64,
    #[serde(rename = "mintsMonitor")]
    pub mints_monitor: Vec<String>,
    #[serde(rename = "nonceAc")]
    pub nonce_ac: Vec<String>,
    #[serde(rename = "meteoraMkts")]
    pub meteora_mkts: u8,
    #[serde(rename = "pumpMkts")]
    pub pump_mkts: u8,
    #[serde(rename = "rayCpmm")]
    pub ray_cpmm: u8,
    #[serde(rename = "meteoraAmm")]
    pub meteora_amm: u8,
    #[serde(rename = "cuPricePercentile")]
    pub cu_price_percentile: f64,
    #[serde(rename = "cuLimit")]
    pub cu_limit: u32,
    #[serde(rename = "maxCUPrice")]
    pub max_cuprice: u64,
    #[serde(rename = "totalVolumeFilter")]
    pub total_volume_filter: u64,
    #[serde(rename = "poolLiqFilter")]
    pub pool_liq_filter: u64,
    #[serde(rename = "numArbsFilter")]
    pub num_arbs_filter: u64,
    #[serde(rename = "accountsMonitor")]
    pub accounts_monitor: Vec<String>,
    #[serde(rename = "mintsIgnore")]
    pub mints_ignore: Vec<String>,
    #[serde(rename = "dynamicLoopInterval")]
    pub dynamic_loop_interval: u64,
    #[serde(rename = "targetMinLandingRate")]
    pub target_min_landing_rate: f64,
    #[serde(rename = "targetMaxLandingRate")]
    pub target_max_landing_rate: f64,
    #[serde(rename = "priceAdjustmentFactor")]
    pub price_adjustment_factor: f64,
    #[serde(rename = "trackWallet")]
    pub track_wallet: String,
    #[serde(rename = "slotsToCheck")]
    pub slots_to_check: u64,
    #[serde(rename = "bufferSize")]
    pub buffer_size: u64,
    #[serde(rename = "numWorkers")]
    pub num_workers: u8,
    #[serde(rename = "windowSeconds")]
    pub window_seconds: u64,
    #[serde(rename = "checkInterval")]
    pub check_interval: u64,
    #[serde(rename = "binsToSearch")]
    pub bins_to_search: u64,
    #[serde(rename = "showTx")]
    pub show_tx: bool,
    #[serde(rename = "birdEyeNumToken")]
    pub bird_eye_num_token: u64,
    #[serde(rename = "birdEyeApi")]
    pub birdeye_api: String,
    #[serde(rename = "nextblock_url")]
    pub nextblock_url: String,
    #[serde(rename = "nextblock_api")]
    pub nextblock_api: String,
    #[serde(rename = "nextblock_cu_price")]
    pub nextblock_cu_price: u64,
    #[serde(rename = "buy_sol")]
    pub buy_sol: f64,
    #[serde(rename = "buy_slippage_bps")]
    pub buy_slippage_bps: u64,
    #[serde(rename = "sell_slippage_bps")]
    pub sell_slippage_bps: u64,
    #[serde(rename = "nextblock_buy_tip")]
    pub nextblock_buy_tip: f64,
    #[serde(rename = "nextblock_sell_tip")]
    pub nextblock_sell_tip: f64,
}

pub fn load_config() -> Config {
    let config_str =
        fs::read_to_string("config.toml").expect("Failed to read config.toml in current directory");
    toml::from_str(&config_str).expect("Failed to parse config.toml")
}
