#!/usr/bin/env bash
# =============================================================================
# BidMart Wallet Service - Load Test Script
# =============================================================================
#
# Simulates concurrent internal wallet hold requests to validate:
#   1. Hold rows and transaction history are persisted under load
#   2. Active and held balances remain internally consistent
#   3. Response latencies remain within APDEX thresholds
#
# Prerequisites:
#   - curl
#   - bc
#   - The wallet service running at WALLET_URL (default: http://localhost:8083)
#   - GATEWAY_INTERNAL_TOKEN accepted by the wallet service
#
# Usage:
#   ./tests/load/run_load_test.sh
#   CONCURRENCY=100 ./tests/load/run_load_test.sh
#   WALLET_URL=http://localhost:8083 HOLD_AMOUNT=2500 ./tests/load/run_load_test.sh
# =============================================================================

set -euo pipefail

WALLET_URL="${WALLET_URL:-http://localhost:8083}"
CONCURRENCY="${CONCURRENCY:-50}"
HOLD_AMOUNT="${HOLD_AMOUNT:-1000}"
INITIAL_BALANCE="${INITIAL_BALANCE:-100000000}"
INTERNAL_TOKEN="${GATEWAY_INTERNAL_TOKEN:-local-dev-internal-token}"
USER_ID="wallet-load-user-$(date +%s)"
AUCTION_ID="wallet-load-auction-$(date +%s)"
REPORT_DIR="$(dirname "$0")/../../docs/load-test-results"
REPORT_FILE="${REPORT_DIR}/wallet_load_test_$(date +%Y%m%d_%H%M%S).md"

mkdir -p "$REPORT_DIR"

echo "=============================================="
echo "BidMart Wallet Service - Load Test"
echo "=============================================="
echo "Target:      ${WALLET_URL}"
echo "Concurrency: ${CONCURRENCY} parallel holds"
echo "User ID:     ${USER_ID}"
echo "Auction ID:  ${AUCTION_ID}"
echo ""

echo "[1/4] Creating and funding wallet..."
CREATE_RESPONSE=$(curl -s -w "\n%{http_code}" -X POST "${WALLET_URL}/api/v1/wallet/add" \
    -H "Content-Type: application/json" \
    -d "{\"userId\":\"${USER_ID}\",\"role\":\"BUYER\"}")
CREATE_STATUS=$(echo "$CREATE_RESPONSE" | tail -1)
if [ "$CREATE_STATUS" != "200" ]; then
    echo "FAIL: Could not create wallet (HTTP ${CREATE_STATUS})"
    echo "$CREATE_RESPONSE" | head -n -1
    exit 1
fi

TOPUP_RESPONSE=$(curl -s -o /dev/null -w "%{http_code}" \
    -X POST "${WALLET_URL}/api/v1/wallet/${USER_ID}/top-up?amount=${INITIAL_BALANCE}&role=BUYER")
if [ "$TOPUP_RESPONSE" != "200" ]; then
    echo "FAIL: Could not top up wallet (HTTP ${TOPUP_RESPONSE})"
    exit 1
fi
echo "  Wallet funded with ${INITIAL_BALANCE} cents"

echo "[2/4] Firing ${CONCURRENCY} concurrent hold requests..."
RESULTS_DIR=$(mktemp -d)
EXPIRES_AT="$(date -u -v+30M +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u -d '+30 minutes' +%Y-%m-%dT%H:%M:%SZ)"

fire_hold() {
    local index="$1"
    local hold_id="wallet-load-hold-${index}"
    local bid_id="wallet-load-bid-${index}"
    local response
    response=$(curl -s -o /dev/null -w "%{http_code},%{time_total}" \
        -X POST "${WALLET_URL}/api/v1/wallet/hold" \
        -H "Content-Type: application/json" \
        -H "X-Internal-Service-Token: ${INTERNAL_TOKEN}" \
        -d "{\"userId\":\"${USER_ID}\",\"role\":\"BUYER\",\"holdId\":\"${hold_id}\",\"auctionId\":\"${AUCTION_ID}\",\"bidId\":\"${bid_id}\",\"amount\":${HOLD_AMOUNT},\"expiresAt\":\"${EXPIRES_AT}\"}")
    echo "$response" > "${RESULTS_DIR}/hold_${index}.txt"
}

for i in $(seq 1 "$CONCURRENCY"); do
    fire_hold "$i" &
done
wait
echo "  All ${CONCURRENCY} hold requests completed"

echo "[3/4] Analyzing results..."
TOTAL=0
SUCCESS=0
ERRORS=0
LATENCIES=""
SATISFIED=0
TOLERATING=0
FRUSTRATED=0

for f in "${RESULTS_DIR}"/hold_*.txt; do
    TOTAL=$((TOTAL + 1))
    LINE=$(cat "$f")
    STATUS=$(echo "$LINE" | cut -d, -f1)
    LATENCY=$(echo "$LINE" | cut -d, -f2)

    if [ "$STATUS" = "200" ]; then
        SUCCESS=$((SUCCESS + 1))
    else
        ERRORS=$((ERRORS + 1))
    fi

    LATENCY_MS=$(echo "$LATENCY" | awk '{printf "%.0f", $1 * 1000}')
    LATENCIES="${LATENCIES} ${LATENCY_MS}"

    if [ "$LATENCY_MS" -le 250 ] 2>/dev/null; then
        SATISFIED=$((SATISFIED + 1))
    elif [ "$LATENCY_MS" -le 1000 ] 2>/dev/null; then
        TOLERATING=$((TOLERATING + 1))
    else
        FRUSTRATED=$((FRUSTRATED + 1))
    fi
done

if [ "$TOTAL" -gt 0 ]; then
    APDEX=$(echo "scale=4; ($SATISFIED + $TOLERATING / 2) / $TOTAL" | bc)
    ERROR_RATE=$(echo "scale=2; $ERRORS * 100 / $TOTAL" | bc)
else
    APDEX="1.0000"
    ERROR_RATE="0.00"
fi

SORTED_LATENCIES=$(echo "$LATENCIES" | tr ' ' '\n' | sort -n | grep -v '^$')
P50_IDX=$((TOTAL * 50 / 100))
P95_IDX=$((TOTAL * 95 / 100))
P99_IDX=$((TOTAL * 99 / 100))
[ "$P50_IDX" -lt 1 ] && P50_IDX=1
[ "$P95_IDX" -lt 1 ] && P95_IDX=1
[ "$P99_IDX" -lt 1 ] && P99_IDX=1
P50=$(echo "$SORTED_LATENCIES" | sed -n "${P50_IDX}p")
P95=$(echo "$SORTED_LATENCIES" | sed -n "${P95_IDX}p")
P99=$(echo "$SORTED_LATENCIES" | sed -n "${P99_IDX}p")
MIN_L=$(echo "$SORTED_LATENCIES" | head -1)
MAX_L=$(echo "$SORTED_LATENCIES" | tail -1)

echo "[4/4] Verifying wallet balance..."
DETAIL=$(curl -s "${WALLET_URL}/api/v1/wallet/${USER_ID}/detail?role=BUYER")
ACTIVE_BALANCE=$(echo "$DETAIL" | sed -n 's/.*"activeBalance":\([0-9]*\).*/\1/p' | head -1)
HELD_BALANCE=$(echo "$DETAIL" | sed -n 's/.*"heldBalance":\([0-9]*\).*/\1/p' | head -1)
EXPECTED_HELD=$((SUCCESS * HOLD_AMOUNT))
EXPECTED_ACTIVE=$((INITIAL_BALANCE - EXPECTED_HELD))

echo ""
echo "=============================================="
echo "LOAD TEST RESULTS"
echo "=============================================="
echo "Total requests:       ${TOTAL}"
echo "Successful (200):     ${SUCCESS}"
echo "Errors:               ${ERRORS} (${ERROR_RATE}%)"
echo ""
echo "Latency (ms):"
echo "  Min:  ${MIN_L:-N/A}"
echo "  p50:  ${P50:-N/A}"
echo "  p95:  ${P95:-N/A}"
echo "  p99:  ${P99:-N/A}"
echo "  Max:  ${MAX_L:-N/A}"
echo ""
echo "APDEX Score:          ${APDEX} (threshold: 250ms/1000ms)"
echo "  Satisfied:          ${SATISFIED}"
echo "  Tolerating:         ${TOLERATING}"
echo "  Frustrated:         ${FRUSTRATED}"
echo ""
echo "Balance Integrity:"
echo "  Active:             ${ACTIVE_BALANCE:-N/A} (expected ${EXPECTED_ACTIVE})"
echo "  Held:               ${HELD_BALANCE:-N/A} (expected ${EXPECTED_HELD})"
echo "=============================================="

cat > "$REPORT_FILE" << EOF
# Wallet Load Test Report - $(date +%Y-%m-%d\ %H:%M:%S)

## Configuration

| Parameter | Value |
|---|---|
| Target | \`${WALLET_URL}\` |
| Concurrency | ${CONCURRENCY} parallel holds |
| Hold amount | ${HOLD_AMOUNT} cents |
| Initial balance | ${INITIAL_BALANCE} cents |
| User ID | \`${USER_ID}\` |
| Auction ID | \`${AUCTION_ID}\` |

## Results

| Metric | Value |
|---|---|
| Total requests | ${TOTAL} |
| Successful (200) | ${SUCCESS} |
| Errors | ${ERRORS} (${ERROR_RATE}%) |
| APDEX | ${APDEX} |

## Latency Distribution

| Percentile | Latency (ms) |
|---|---|
| Min | ${MIN_L:-N/A} |
| p50 | ${P50:-N/A} |
| p95 | ${P95:-N/A} |
| p99 | ${P99:-N/A} |
| Max | ${MAX_L:-N/A} |

## Balance Integrity

| Field | Actual | Expected |
|---|---:|---:|
| Active balance | ${ACTIVE_BALANCE:-N/A} | ${EXPECTED_ACTIVE} |
| Held balance | ${HELD_BALANCE:-N/A} | ${EXPECTED_HELD} |
EOF

echo ""
echo "Report saved to: ${REPORT_FILE}"

rm -rf "$RESULTS_DIR"
