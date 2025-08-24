use crate::constants::pump_fun::PUMP_FUN_PROGRAM_ID;    
use crate::constants::raydium_launchpad::RAYDIUM_LAUNCHPAD_PROGRAM_ID;
use crate::config_load::GLOBAL_CONFIG;

// Programs to monitor for DEX activity
pub const MONITORING_PROGRAMS: &[&str] = &[
    RAYDIUM_LAUNCHPAD_PROGRAM_ID,
    PUMP_FUN_PROGRAM_ID,
    // Add more programs as needed
];

// Function to get monitoring ARPC endpoint from config
pub fn get_monitoring_arpc_endpoint() -> String {
    GLOBAL_CONFIG.get()
        .map(|config| config.arpc_endpoint.clone())
        .unwrap_or_else(|| "http://86.105.224.13:20202".to_string()) // Fallback to default
}

// Fallback to public endpoints if monitoring endpoint not available
pub const MONITORING_FALLBACK_ENDPOINT: &str = "http://86.105.224.13:20202";

pub const MONITORING_LOG_RETENTION_MINUTES: i64 = 5; // How long to keep logs in memory
pub const MONITORING_STATS_INTERVAL_SECONDS: u64 = 60; // How often to report stats

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_monitoring_arpc_endpoint_fallback() {
        // Test with fallback when config is not loaded
        let endpoint = get_monitoring_arpc_endpoint();
        assert_eq!(endpoint, "http://86.105.224.13:20202");
    }
}

 