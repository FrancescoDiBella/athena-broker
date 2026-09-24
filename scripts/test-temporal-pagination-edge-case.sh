#!/usr/bin/env bash
# ==============================================================================
# Athena Broker - Temporal Query Pagination & Edge Case Verification
# TEST CASE:
# - Create N+1 entities of a specific type (N=5, total=6).
# - Entities have incremental timestamps.
# - The target entity is the OLDEST entity, positioned on Page 2 under all default sorts:
#   * Under ID ASC: target is named 'Z_TARGET', alphabetically after A01-A05.
#   * Under modified_at DESC: target is created first, before A01-A05.
# - The target entity is the ONLY entity matching the temporal condition (timerel=before).
# - Verify that SQL pushdown evaluates the temporal predicate BEFORE limit/offset,
#   guaranteeing that the target entity is returned on Page 1.
# ==============================================================================
set -e

BROKER_URL="${BROKER_URL:-http://localhost:8080}"
GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
YELLOW='\033[0;33m'
NC='\033[0m'

echo -e "${BLUE}===================================================================${NC}"
echo -e "${BLUE}   NGSI-LD Temporal Query Pagination & Pushdown Edge Case Test     ${NC}"
echo -e "${BLUE}===================================================================${NC}\n"

# 1. Health check
echo -n "Checking broker health at ${BROKER_URL}... "
curl -sf "${BROKER_URL}/health" > /dev/null
echo -e "${GREEN}OK${NC}"

RUN_ID=$(date +%s)
TYPE="TemporalEdgeDevice_${RUN_ID}"
N=5

TARGET_ID="urn:ngsi-ld:${TYPE}:${RUN_ID}:Z_TARGET"
TARGET_TIME="2026-01-01T10:00:00Z"
FILTER_BEFORE="2026-01-15T00:00:00Z"

echo -e "\n${YELLOW}Setting up test entities:${NC}"
echo "Step 1: Creating target entity (Oldest: ${TARGET_TIME}) with ID ${TARGET_ID}"

# Create target entity first
curl -s -X POST "${BROKER_URL}/ngsi-ld/v1/entities" \
  -H "Content-Type: application/ld+json" \
  -d "{
    \"id\": \"${TARGET_ID}\",
    \"type\": \"${TYPE}\",
    \"temperature\": {
      \"type\": \"Property\",
      \"value\": 15.0
    },
    \"@context\": \"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld\"
  }" > /dev/null

curl -s -X POST "${BROKER_URL}/ngsi-ld/v1/temporal/entities" \
  -H "Content-Type: application/json" \
  -d "{
    \"id\": \"${TARGET_ID}\",
    \"type\": \"${TYPE}\",
    \"temperature\": [
      {
        \"type\": \"Property\",
        \"value\": 15.0,
        \"observedAt\": \"${TARGET_TIME}\"
      }
    ]
  }" > /dev/null

# Create N other entities (A01 to A05) with newer timestamps (June 2026)
echo "Step 2: Creating ${N} newer entities (A01..A05) with timestamps in June 2026"
for i in 1 2 3 4 5; do
  PAD_I=$(printf "%02d" $i)
  E_ID="urn:ngsi-ld:${TYPE}:${RUN_ID}:A${PAD_I}"
  OBS_TIME="2026-06-0${i}T10:00:00Z"
  TEMP_VAL=$(echo "20.0 + $i" | bc)

  curl -s -X POST "${BROKER_URL}/ngsi-ld/v1/entities" \
    -H "Content-Type: application/ld+json" \
    -d "{
      \"id\": \"${E_ID}\",
      \"type\": \"${TYPE}\",
      \"temperature\": {
        \"type\": \"Property\",
        \"value\": ${TEMP_VAL}
      },
      \"@context\": \"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld\"
    }" > /dev/null

  curl -s -X POST "${BROKER_URL}/ngsi-ld/v1/temporal/entities" \
    -H "Content-Type: application/json" \
    -d "{
      \"id\": \"${E_ID}\",
      \"type\": \"${TYPE}\",
      \"temperature\": [
        {
          \"type\": \"Property\",
          \"value\": ${TEMP_VAL},
          \"observedAt\": \"${OBS_TIME}\"
        }
      ]
    }" > /dev/null
done

echo -e "\n${BLUE}--- Verification Step 1: Baseline Entity Query without Temporal Filter ---${NC}"
PAGE1_NO_TEMPORAL=$(curl -sG "${BROKER_URL}/ngsi-ld/v1/entities" \
  --data-urlencode "type=${TYPE}" \
  --data-urlencode "limit=${N}" \
  --data-urlencode "offset=0")

COUNT_P1=$(echo "$PAGE1_NO_TEMPORAL" | grep -o '"id":' | wc -l | tr -d ' ')
echo "Base query (type=${TYPE}, limit=${N}, offset=0) returned ${COUNT_P1} entities."

if echo "$PAGE1_NO_TEMPORAL" | grep -q "${TARGET_ID}"; then
  echo -e "${RED}FAILURE IN TEST PRECONDITION: Target was found on Page 1 without temporal filter.${NC}"
  exit 1
else
  echo -e "${GREEN}Confirmed: Target '${TARGET_ID}' is OUTSIDE Page 1 (it sits on Page 2).${NC}"
fi

echo -e "\n${BLUE}--- Verification Step 2: Temporal Query with SQL Pushdown ---${NC}"
echo "Querying: GET /ngsi-ld/v1/temporal/entities?type=${TYPE}&timerel=before&timeAt=${FILTER_BEFORE}&limit=${N}&offset=0"

TEMPORAL_PAGE1=$(curl -sG "${BROKER_URL}/ngsi-ld/v1/temporal/entities" \
  --data-urlencode "type=${TYPE}" \
  --data-urlencode "timerel=before" \
  --data-urlencode "timeAt=${FILTER_BEFORE}" \
  --data-urlencode "limit=${N}" \
  --data-urlencode "offset=0")

echo "Response payload: $TEMPORAL_PAGE1"

MATCH_COUNT=$(echo "$TEMPORAL_PAGE1" | grep -o '"id":' | wc -l | tr -d ' ')

if [ "$MATCH_COUNT" -eq 1 ] && echo "$TEMPORAL_PAGE1" | grep -q "${TARGET_ID}"; then
  echo -e "\n${GREEN}===================================================================${NC}"
  echo -e "${GREEN}TEST PASSED!${NC}"
  echo -e "${GREEN}Target entity '${TARGET_ID}' was CORRECTLY returned on Page 1!${NC}"
  echo -e "${GREEN}Proof: Without pushdown it would have been on Page 2 and dropped.${NC}"
  echo -e "${GREEN}With SQL Pushdown, filtering occurs in PostgreSQL BEFORE LIMIT/OFFSET.${NC}"
  echo -e "${GREEN}===================================================================${NC}"
else
  echo -e "\n${RED}===================================================================${NC}"
  echo -e "${RED}TEST FAILED!${NC}"
  echo -e "${RED}Expected 1 entity (${TARGET_ID}), but got count=${MATCH_COUNT}${NC}"
  echo -e "${RED}===================================================================${NC}"
  exit 1
fi
