#!/usr/bin/env bash
#
# test-full-flow.sh - Manual verification script for Nova Memory proxy flow
#
# This script runs the full integration test suite and provides instructions
# for manual verification once the daemon is fully implemented.
#
# Usage:
#   ./scripts/test-full-flow.sh           # Run all tests
#   ./scripts/test-full-flow.sh --manual  # Show manual testing instructions
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[OK]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

print_header() {
    echo ""
    echo "=============================================="
    echo "$1"
    echo "=============================================="
    echo ""
}

run_automated_tests() {
    print_header "Running Automated Integration Tests"

    cd "$PROJECT_ROOT"

    log_info "Building project..."
    if cargo build -p nova-memory 2>&1; then
        log_success "Build successful"
    else
        log_error "Build failed"
        exit 1
    fi

    log_info "Running integration test suite..."
    log_warn "Using --test-threads=1 due to ML model loading contention"
    echo ""

    if cargo test -p nova-memory --test integration_test -- --test-threads=1 2>&1; then
        log_success "All integration tests passed!"
    else
        log_error "Some integration tests failed"
        exit 1
    fi

    echo ""
    log_info "Running full test suite (all tests)..."
    if cargo test -p nova-memory -- --test-threads=1 2>&1; then
        log_success "All tests passed!"
    else
        log_warn "Some tests failed (check output above)"
    fi
}

show_manual_instructions() {
    print_header "Manual Testing Instructions"

    cat << 'EOF'
Once the Nova Memory daemon supports a 'serve' command, you can manually verify
the full proxy flow using these steps:

1. START THE DAEMON
   ----------------
   Set your OpenAI API key and start the proxy:

   export OPENAI_API_KEY="your-api-key"
   nova-cli serve --listen 127.0.0.1:8080 --upstream https://api.openai.com

2. VERIFY HEALTH CHECK
   --------------------
   curl http://localhost:8080/health
   # Expected: {"status":"ok"}

3. INGEST TEST MEMORIES
   ---------------------
   # Ingest some context that the proxy should remember
   nova-cli memory add "The user prefers Python for data science projects."
   nova-cli memory add "User mentioned they use VS Code as their primary editor."
   nova-cli memory add "The team meeting about the ML project is Friday at 3pm."

4. MAKE A PROXIED REQUEST
   -----------------------
   # Send a request through the proxy - memories should be injected
   curl -X POST http://localhost:8080/v1/chat/completions \
     -H "Content-Type: application/json" \
     -H "Authorization: Bearer $OPENAI_API_KEY" \
     -d '{
       "model": "gpt-4",
       "messages": [
         {"role": "user", "content": "What programming tools do I prefer?"}
       ],
       "stream": false
     }'

   # The response should reference the injected memories

5. VERIFY RESPONSE CAPTURE
   ------------------------
   # The assistant's response should be captured and stored
   nova-cli memory list --recent 5

   # You should see the new response content stored as a memory

6. VERIFY STREAMING FLOW
   ----------------------
   # Test with streaming enabled
   curl -X POST http://localhost:8080/v1/chat/completions \
     -H "Content-Type: application/json" \
     -H "Authorization: Bearer $OPENAI_API_KEY" \
     -d '{
       "model": "gpt-4",
       "messages": [
         {"role": "user", "content": "What editor should I use?"}
       ],
       "stream": true
     }'

   # Should stream SSE events while simultaneously buffering for ingestion

7. CHECK CAPACITY MANAGEMENT
   -------------------------
   # View storage statistics
   nova-cli stats

   # Trigger compaction manually
   nova-cli compact --tier warm

   # Check tier distribution
   nova-cli stats --by-tier

EXPECTED FLOW:
--------------
1. Request arrives at proxy
2. Proxy extracts user query from last message
3. Proxy retrieves relevant memories via vector search
4. Memories are injected into system prompt as <nova-memories> XML block
5. Request is forwarded to upstream (OpenAI API)
6. Response streams back to client immediately (zero latency added)
7. Response is buffered in parallel via tee stream
8. After streaming completes, content is captured and ingested
9. New memory is stored in hot tier for future retrieval

VERIFICATION CHECKLIST:
-----------------------
[ ] Health endpoint returns 200 OK
[ ] Memories can be added via CLI
[ ] Requests are proxied to upstream
[ ] Memory injection appears in system prompt
[ ] Streaming responses work correctly
[ ] Response content is captured and stored
[ ] Capacity management (tiers, compaction, eviction) works
EOF
}

show_quick_test() {
    print_header "Quick Test Commands"

    cat << 'EOF'
# Run just the integration tests:
cargo test -p nova-memory --test integration_test -- --test-threads=1

# Run all tests with verbose output:
cargo test -p nova-memory -- --test-threads=1 --nocapture

# Run specific test modules:
cargo test -p nova-memory --test integration_test full_proxy_flow -- --test-threads=1
cargo test -p nova-memory --test integration_test memory_injection -- --test-threads=1
cargo test -p nova-memory --test integration_test capacity_management -- --test-threads=1

# Run with backtrace on failures:
RUST_BACKTRACE=1 cargo test -p nova-memory --test integration_test -- --test-threads=1
EOF
}

main() {
    print_header "Nova Memory - Full Flow Test Script"

    if [[ "${1:-}" == "--manual" ]]; then
        show_manual_instructions
        exit 0
    fi

    if [[ "${1:-}" == "--quick" ]]; then
        show_quick_test
        exit 0
    fi

    if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
        echo "Usage: $0 [OPTIONS]"
        echo ""
        echo "Options:"
        echo "  --manual    Show manual testing instructions"
        echo "  --quick     Show quick test commands"
        echo "  --help, -h  Show this help message"
        echo ""
        echo "Without options, runs the automated integration test suite."
        exit 0
    fi

    run_automated_tests

    echo ""
    log_info "For manual testing instructions, run: $0 --manual"
    log_info "For quick test commands, run: $0 --quick"
}

main "$@"
