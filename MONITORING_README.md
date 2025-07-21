# DEX Activity Monitoring System

This monitoring system allows you to track DEX activity across multiple programs using **separate feeds** that don't interfere with your high-frequency trading pipes.

## Architecture

The monitoring system is designed to be completely separate from your trading infrastructure:

- **Trading Pipes**: Run on cores 1-3 with critical real-time priority
  - **Triton gRPC**: High-frequency trading signals
  - **ARPC**: High-frequency trading signals
- **Monitoring System**: Runs on core 4 with low priority to avoid interference
  - **Separate ARPC Feed**: Dedicated monitoring endpoint
  - **Independent Storage**: Uses `GLOBAL_DEX_LOGS` instead of `GLOBAL_TX_MAP`

## Key Features

### 1. Non-Interfering Design
- Runs on a separate CPU core (core 4)
- Uses lower priority scheduling
- **Separate ARPC endpoint** (different from trading feeds)
- Independent ARPC connection
- Separate memory storage

### 2. High-Volume Handling
- Large message buffer (256 vs 128 for trading)
- Asynchronous processing with tokio tasks
- Automatic cleanup of old logs (5-minute retention)
- Efficient DashMap storage

### 3. Flexible Monitoring
- Monitor any number of programs
- Configurable retention periods
- Real-time statistics reporting
- Program-specific querying

## Usage

### 1. Basic Integration

Add to your `main.rs`:

```rust
use crate::grpc::monitoring_client::start_arpc_monitoring_with_retry;
use crate::constants::monitoring::{MONITORING_PROGRAMS, MONITORING_ARPC_ENDPOINT};

// In your main function, after initializing your trading systems:
let monitoring_handle = tokio::spawn(async move {
    let programs_to_monitor: Vec<String> = MONITORING_PROGRAMS
        .iter()
        .map(|&s| s.to_string())
        .collect();
    
    if let Err(e) = start_arpc_monitoring_with_retry(
        MONITORING_ARPC_ENDPOINT,  // Separate endpoint from trading
        programs_to_monitor, 
        config_arc.clone()
    ).await {
        eprintln!("[Monitoring] Error: {}", e);
    }
});

handles.push(monitoring_handle);
```

### 2. Querying Monitoring Data

```rust
use crate::grpc::monitoring_client::{
    get_monitoring_stats,
    get_dex_logs_by_program,
    get_recent_dex_logs,
    get_dex_logs_count
};

// Get statistics
let (received, logged, errors) = get_monitoring_stats();
println!("Monitoring stats: {} received, {} logged, {} errors", received, logged, errors);

// Get logs for specific program
let raydium_logs = get_dex_logs_by_program("LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj");
println!("Raydium Launchpad logs: {}", raydium_logs.len());

// Get recent logs
let recent_logs = get_recent_dex_logs(5); // Last 5 minutes
println!("Recent logs: {}", recent_logs.len());

// Get total logs count
let total_logs = get_dex_logs_count();
println!("Total logs in memory: {}", total_logs);
```

### 3. Configuration

Edit `src/constants/monitoring.rs` to customize:

```rust
// Add programs to monitor
pub const MONITORING_PROGRAMS: &[&str] = &[
    RAYDIUM_LAUNCHPAD_PROGRAM_ID,
    AXIOM_PUMP_SWAP_PROGRAM_ID,
    AXIOM_PUMP_FUN_PROGRAM_ID,
    // Add more programs here
];

// Separate endpoint for monitoring (different from trading)
pub const MONITORING_ARPC_ENDPOINT: &str = "https://arpc.monitoring.example.com";

// Fallback endpoint if monitoring endpoint unavailable
pub const MONITORING_FALLBACK_ENDPOINT: &str = "https://api.mainnet-beta.solana.com";

// Adjust retention period
pub const MONITORING_LOG_RETENTION_MINUTES: i64 = 5;
```

## Performance Characteristics

### Memory Usage
- Each log entry: ~200-500 bytes
- 1000 logs: ~200-500 KB
- Automatic cleanup every 30 seconds

### CPU Usage
- Runs on separate core (core 4)
- Low priority scheduling
- Minimal impact on trading performance

### Network Usage
- Separate gRPC connection
- Larger buffer for high-volume handling
- Automatic reconnection on failure

## Monitoring vs Trading Comparison

| Aspect | Trading System | Monitoring System |
|--------|----------------|-------------------|
| CPU Core | 1-3 | 4 |
| Priority | Critical | Low |
| Buffer Size | 128 | 256 |
| Storage | GLOBAL_TX_MAP | GLOBAL_DEX_LOGS |
| Retention | 10 seconds | 5 minutes |
| Purpose | Execute trades | Track activity |
| **Feed Endpoints** | **Trading ARPC/gRPC** | **Separate Monitoring ARPC** |

## Example Output

```
[2024-01-15 10:30:15.123] - [MONITORING ARPC] DEX activity subscription established. Monitoring 3 programs...
[2024-01-15 10:30:15.456] - [MONITORING ARPC] Thread pinned to core 4
[2024-01-15 10:30:16.789] - [MONITORING ARPC] Logged DEX activity: 5J7X... (processing time: 1.23ms)
[2024-01-15 10:31:15.000] - [MONITORING ARPC STATS] Received: 1250, Logged: 1248, Errors: 2, Processing Rate: 99.84%
[2024-01-15 10:31:15.000] - [MONITORING ARPC STATS] GLOBAL_DEX_LOGS size: 847
```

## Troubleshooting

### High Memory Usage
- Check if logs are being purged properly
- Reduce `MONITORING_LOG_RETENTION_MINUTES`
- Monitor `GLOBAL_DEX_LOGS` size in stats

### Performance Issues
- Ensure monitoring runs on core 4
- Verify low priority scheduling is set
- Check for network connectivity issues

### Missing Logs
- Verify program IDs are correct
- Check gRPC connection status
- Review error counts in stats

## Integration with Existing Stats

Add monitoring stats to your existing stats monitoring:

```rust
// In your stats monitoring function
let (monitoring_received, monitoring_logged, monitoring_errors) = get_monitoring_stats();
let monitoring_logs_count = get_dex_logs_count();

println!("[{}] MONITORING: Received={}, Logged={}, Errors={}, Active Logs={}", 
    now.format("%Y-%m-%d %H:%M:%S%.3f"),
    monitoring_received, monitoring_logged, monitoring_errors, monitoring_logs_count
);
```

This monitoring system provides comprehensive DEX activity tracking while ensuring your trading performance remains unaffected. 