# Nova Memory - Issues & Gotchas

## Known Issues

<!-- Subagents: APPEND issues here, never overwrite -->


## 2026-01-31: 24h Memory Leak Test - BLOCKED (Manual Verification Required)

### Task
- [ ] Daemon runs continuously without memory leaks for 24h

### Blocker
This task requires running the daemon for 24 hours and monitoring memory usage. This cannot be automated in the current session.

### How to Verify Manually
```bash
# 1. Build release binary
cargo build --release -p nova-memory

# 2. Start daemon in background
./target/release/nova-memory serve &
DAEMON_PID=$!

# 3. Monitor memory every minute for 24h
while true; do
  echo "$(date): $(ps -o rss= -p $DAEMON_PID) KB" >> memory_log.txt
  sleep 60
done

# 4. After 24h, analyze memory_log.txt
# Memory should be stable (not continuously growing)
# Some fluctuation is normal, but trend should be flat

# 5. Kill daemon
kill $DAEMON_PID
```

### Expected Behavior
- Initial memory: ~100-200 MB (model loading)
- Steady state: Should stabilize after warmup
- No continuous growth over 24h period
- Acceptable: Minor fluctuations due to GC/caching

### Status
BLOCKED - Requires manual 24h test by user
