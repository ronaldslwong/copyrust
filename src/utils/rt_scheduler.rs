//! Real-time scheduling utilities for critical trading threads
//! 
//! This module provides SCHED_FIFO real-time priority scheduling for Linux systems.
//! Priority range: 1-99 (99 = highest priority)
//! 
//! Usage:
//!   use crate::utils::rt_scheduler::{set_realtime_priority, RealtimePriority};
//!   set_realtime_priority(RealtimePriority::Critical); // Priority 99

use std::error::Error;
use std::fmt;

#[cfg(target_os = "linux")]
use thread_priority::{ThreadExt, ThreadSchedulePolicy, RealtimeThreadSchedulePolicy};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealtimePriority {
    /// Low priority (1-20)
    Low = 10,
    /// Medium priority (21-50) 
    Medium = 35,
    /// High priority (51-80)
    High = 65,
    /// Critical priority (81-99) - Use sparingly
    Critical = 99,
}

impl RealtimePriority {
    pub fn value(&self) -> u8 {
        *self as u8
    }
    
    pub fn description(&self) -> &'static str {
        match self {
            RealtimePriority::Low => "Low priority (10)",
            RealtimePriority::Medium => "Medium priority (35)", 
            RealtimePriority::High => "High priority (65)",
            RealtimePriority::Critical => "Critical priority (99)",
        }
    }
}

#[derive(Debug)]
pub enum SchedulerError {
    NotSupported,
    PermissionDenied,
    InvalidPriority,
    OsError(String),
}

impl fmt::Display for SchedulerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchedulerError::NotSupported => write!(f, "Real-time scheduling not supported on this platform"),
            SchedulerError::PermissionDenied => write!(f, "Permission denied - requires root or CAP_SYS_NICE capability"),
            SchedulerError::InvalidPriority => write!(f, "Invalid priority value"),
            SchedulerError::OsError(msg) => write!(f, "OS error: {}", msg),
        }
    }
}

impl Error for SchedulerError {}

/// Set real-time SCHED_FIFO priority for the current thread
/// 
/// # Arguments
/// * `priority` - The real-time priority level (1-99)
/// 
/// # Returns
/// * `Ok(())` - Successfully set priority
/// * `Err(SchedulerError)` - Failed to set priority
/// 
/// # Example
/// ```
/// use crate::utils::rt_scheduler::{set_realtime_priority, RealtimePriority};
/// 
/// // Set critical priority for detection threads
/// set_realtime_priority(RealtimePriority::Critical)?;
/// 
/// // Set high priority for processing threads  
/// set_realtime_priority(RealtimePriority::High)?;
/// ```
pub fn set_realtime_priority(priority: RealtimePriority) -> Result<(), SchedulerError> {
    #[cfg(target_os = "linux")]
    {
        let priority_value = priority.value();
        
        // Validate priority range
        if priority_value < 1 || priority_value > 99 {
            return Err(SchedulerError::InvalidPriority);
        }
        
        // Set SCHED_FIFO real-time policy with specified priority
        match std::thread::current().set_priority_and_policy(
            ThreadSchedulePolicy::Realtime(RealtimeThreadSchedulePolicy::Fifo),
            thread_priority::ThreadPriority::Crossplatform(priority_value.try_into().unwrap()),
        ) {
            Ok(_) => {
                // println!("[RT-Scheduler] Set {} for thread: {}", 
                //     priority.description(), 
                //     std::thread::current().name().unwrap_or("unnamed")
                // );
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("Failed to set real-time priority: {:?}", e);
                eprintln!("[RT-Scheduler] {}", error_msg);
                
                // Check if it's a permission error
                if error_msg.contains("Permission denied") || error_msg.contains("EACCES") {
                    Err(SchedulerError::PermissionDenied)
                } else {
                    Err(SchedulerError::OsError(error_msg))
                }
            }
        }
    }
    
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("[RT-Scheduler] Real-time scheduling not supported on this platform");
        Err(SchedulerError::NotSupported)
    }
}

/// Check if real-time scheduling is supported and available
pub fn is_realtime_supported() -> bool {
    #[cfg(target_os = "linux")]
    {
        // Try to set a low priority to test if we have permission
        match set_realtime_priority(RealtimePriority::Low) {
            Ok(_) => true,
            Err(SchedulerError::PermissionDenied) => {
                eprintln!("[RT-Scheduler] Real-time scheduling requires root or CAP_SYS_NICE capability");
                false
            }
            Err(_) => false,
        }
    }
    
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Get current thread's scheduling policy and priority
pub fn get_current_scheduling_info() -> Result<String, SchedulerError> {
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        
        let thread_id = std::thread::current().id();
        let pid = std::process::id();
        
        // Get scheduling info from /proc
        match Command::new("chrt")
            .args(&["-p", &pid.to_string()])
            .output() {
            Ok(output) => {
                let info = String::from_utf8_lossy(&output.stdout);
                Ok(format!("Thread {:?}: {}", thread_id, info.trim()))
            }
            Err(_) => {
                // Fallback: just return basic info
                Ok(format!("Thread {:?} (PID: {})", thread_id, pid))
            }
        }
    }
    
    #[cfg(not(target_os = "linux"))]
    {
        Ok("Scheduling info not available on this platform".to_string())
    }
}

/// Initialize real-time scheduling for critical threads
/// 
/// This function should be called early in your application startup
/// to verify real-time scheduling is available.
pub fn init_realtime_scheduling() -> Result<(), SchedulerError> {
    println!("[RT-Scheduler] Initializing real-time scheduling...");
    
    if !is_realtime_supported() {
        eprintln!("[RT-Scheduler] Warning: Real-time scheduling not available");
        eprintln!("[RT-Scheduler] For optimal performance, run with:");
        eprintln!("[RT-Scheduler]   sudo setcap cap_sys_nice+ep ./target/release/copy_rust");
        eprintln!("[RT-Scheduler]   OR run as root");
        return Err(SchedulerError::NotSupported);
    }
    
    println!("[RT-Scheduler] Real-time scheduling is available");
    println!("[RT-Scheduler] Current scheduling info: {}", get_current_scheduling_info()?);
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_priority_values() {
        assert_eq!(RealtimePriority::Low.value(), 10);
        assert_eq!(RealtimePriority::Medium.value(), 35);
        assert_eq!(RealtimePriority::High.value(), 65);
        assert_eq!(RealtimePriority::Critical.value(), 99);
    }
    
    #[test]
    fn test_priority_descriptions() {
        assert!(RealtimePriority::Low.description().contains("Low"));
        assert!(RealtimePriority::Critical.description().contains("Critical"));
    }
} 