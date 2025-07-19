pub mod ata;
pub mod logger;
pub mod rt_scheduler;
pub mod token_balance;

#[cfg(target_os = "linux")]
pub fn get_memory_usage() -> Option<(usize, usize)> {
    use std::fs;
    
    if let Ok(contents) = fs::read_to_string("/proc/self/status") {
        let mut rss = None;
        let mut vm_size = None;
        
        for line in contents.lines() {
            if line.starts_with("VmRSS:") {
                if let Some(kb_str) = line.split_whitespace().nth(1) {
                    if let Ok(kb) = kb_str.parse::<usize>() {
                        rss = Some(kb * 1024); // Convert KB to bytes
                    }
                }
            } else if line.starts_with("VmSize:") {
                if let Some(kb_str) = line.split_whitespace().nth(1) {
                    if let Ok(kb) = kb_str.parse::<usize>() {
                        vm_size = Some(kb * 1024); // Convert KB to bytes
                    }
                }
            }
        }
        
        if let (Some(r), Some(v)) = (rss, vm_size) {
            return Some((r, v));
        }
    }
    None
}

#[cfg(target_os = "macos")]
pub fn get_memory_usage() -> Option<(usize, usize)> {
    use std::process::Command;
    
    if let Ok(output) = Command::new("ps")
        .args(&["-o", "rss,vsz", "-p", &std::process::id().to_string()])
        .output() {
        
        if let Ok(output_str) = String::from_utf8(output.stdout) {
            let lines: Vec<&str> = output_str.lines().collect();
            if lines.len() >= 2 {
                let parts: Vec<&str> = lines[1].split_whitespace().collect();
                if parts.len() >= 2 {
                    if let (Ok(rss), Ok(vsz)) = (parts[0].parse::<usize>(), parts[1].parse::<usize>()) {
                        return Some((rss * 1024, vsz * 1024)); // Convert KB to bytes
                    }
                }
            }
        }
    }
    None
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn get_memory_usage() -> Option<(usize, usize)> {
    None
}

pub fn format_bytes(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    
    if bytes as f64 >= GB {
        format!("{:.2} GB", bytes as f64 / GB)
    } else if bytes as f64 >= MB {
        format!("{:.2} MB", bytes as f64 / MB)
    } else if bytes as f64 >= KB {
        format!("{:.2} KB", bytes as f64 / KB)
    } else {
        format!("{} B", bytes)
    }
}
