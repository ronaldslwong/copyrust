#!/bin/bash

# Setup script for real-time scheduling capabilities
# This script helps configure the necessary permissions for SCHED_FIFO real-time priority

echo "🚀 Setting up real-time scheduling for Solana Trading Bot"
echo "=================================================="

# Check if running as root
if [[ $EUID -eq 0 ]]; then
   echo "✅ Running as root - real-time scheduling should work"
else
   echo "⚠️  Not running as root - will need to set capabilities"
fi

# Check if the binary exists
if [ ! -f "./target/release/copy_rust" ]; then
    echo "📦 Building release binary..."
    cargo build --release --bin copy_rust
fi

# Set capabilities for real-time scheduling
echo "🔧 Setting CAP_SYS_NICE capability..."
if command -v setcap &> /dev/null; then
    sudo setcap cap_sys_nice+ep ./target/release/copy_rust
    if [ $? -eq 0 ]; then
        echo "✅ Successfully set CAP_SYS_NICE capability"
    else
        echo "❌ Failed to set capability"
    fi
else
    echo "❌ setcap command not found"
fi

# Verify capabilities
echo "🔍 Verifying capabilities..."
if command -v getcap &> /dev/null; then
    getcap ./target/release/copy_rust
else
    echo "⚠️  getcap command not found - cannot verify"
fi

# Check real-time scheduling limits
echo "📊 Checking real-time scheduling limits..."
if [ -f "/proc/sys/kernel/sched_rt_runtime_us" ]; then
    echo "Current RT runtime limit: $(cat /proc/sys/kernel/sched_rt_runtime_us) microseconds"
    echo "Current RT period: $(cat /proc/sys/kernel/sched_rt_period_us) microseconds"
    
    # Check if RT runtime is unlimited (-1)
    if [ "$(cat /proc/sys/kernel/sched_rt_runtime_us)" = "-1" ]; then
        echo "✅ Real-time runtime is unlimited"
    else
        echo "⚠️  Real-time runtime is limited - consider setting to -1 for best performance"
        echo "   Run: echo -1 | sudo tee /proc/sys/kernel/sched_rt_runtime_us"
    fi
else
    echo "⚠️  Cannot check real-time scheduling limits"
fi

# Check CPU isolation (if available)
echo "🔍 Checking CPU isolation..."
if command -v isolcpus &> /dev/null || grep -q isolcpus /proc/cmdline; then
    echo "✅ CPU isolation detected"
    echo "Isolated CPUs: $(grep -o 'isolcpus=[^ ]*' /proc/cmdline 2>/dev/null || echo 'Not found in cmdline')"
else
    echo "ℹ️  No CPU isolation detected - consider adding isolcpus=0,1,2,3 to kernel parameters"
fi

echo ""
echo "🎯 Real-time scheduling setup complete!"
echo ""
echo "📋 Usage:"
echo "   ./target/release/copy_rust"
echo ""
echo "🔍 Monitor real-time threads:"
echo "   chrt -p \$(pgrep copy_rust)"
echo ""
echo "📊 Check thread priorities:"
echo "   ps -eo pid,ppid,cls,pri,cmd | grep copy_rust"
echo ""
echo "⚡ For maximum performance:"
echo "   1. Run as root: sudo ./target/release/copy_rust"
echo "   2. Or ensure CAP_SYS_NICE is set: sudo setcap cap_sys_nice+ep ./target/release/copy_rust"
echo "   3. Set RT runtime to unlimited: echo -1 | sudo tee /proc/sys/kernel/sched_rt_runtime_us"
echo "" 