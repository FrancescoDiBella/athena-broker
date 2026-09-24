#!/usr/bin/env bash
# ==============================================================================
# Athena Broker - SQL Query & Temporal Edge Cases Verification Suite
# ==============================================================================
set -e

BROKER_URL="${BROKER_URL:-http://localhost:8080}"
GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}=== Starting SQL Query & Temporal Edge Cases Suite on ${BROKER_URL} ===${NC}\n"

# Check health
echo -n "1. Checking broker health: "
curl -sf "${BROKER_URL}/health" > /dev/null
echo -e "${GREEN}OK${NC}"

# Seed initial test entities if not already present
echo -n "2. Seeding test sensor & vehicle entities: "
curl -s -X POST "${BROKER_URL}/ngsi-ld/v1/entities" \
  -H "Content-Type: application/ld+json" \
  -d '{
    "id": "urn:ngsi-ld:Sensor:Edge01",
    "type": "Sensor",
    "co2": { "type": "Property", "value": 415.0 },
    "temperature": { "type": "Property", "value": 22.0 },
    "location": {
      "type": "GeoProperty",
      "value": { "type": "Point", "coordinates": [12.4924, 41.8902] }
    },
    "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
  }' > /dev/null || true

# Seed temporal history points for Sensor:Edge01
curl -s -X POST "${BROKER_URL}/ngsi-ld/v1/temporal/entities" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "urn:ngsi-ld:Sensor:Edge01",
    "type": "Sensor",
    "co2": [
      { "type": "Property", "value": 410.0, "observedAt": "2026-09-22T08:00:00Z" },
      { "type": "Property", "value": 420.0, "observedAt": "2026-09-22T08:30:00Z" },
      { "type": "Property", "value": 435.0, "observedAt": "2026-09-22T09:00:00Z" },
      { "type": "Property", "value": 450.0, "observedAt": "2026-09-22T09:30:00Z" },
      { "type": "Property", "value": 425.0, "observedAt": "2026-09-22T10:00:00Z" }
    ]
  }' > /dev/null || true
echo -e "${GREEN}OK${NC}"

# ------------------------------------------------------------------------------
# TEST SUITE A: PAGINATION & LIMIT EDGE CASES
# ------------------------------------------------------------------------------
echo -e "\n${BLUE}--- Suite A: Pagination & Limits ---${NC}"

# A.1: Limit = 1 (Exact count returned)
echo -n "Test A.1: Query with limit=1: "
RES=$(curl -sG "${BROKER_URL}/ngsi-ld/v1/entities" --data-urlencode "limit=1")
COUNT=$(echo "$RES" | grep -o '"id":' | wc -l | tr -d ' ')
if [ "$COUNT" -eq 1 ]; then
  echo -e "${GREEN}PASSED (exactly 1 returned)${NC}"
else
  echo -e "${RED}FAILED (expected 1, got $COUNT)${NC}" && exit 1
fi

# A.2: Limit clamping (limit=0 clamped to 1)
echo -n "Test A.2: Limit=0 clamped to 1: "
RES=$(curl -sG "${BROKER_URL}/ngsi-ld/v1/entities" --data-urlencode "limit=0")
COUNT=$(echo "$RES" | grep -o '"id":' | wc -l | tr -d ' ')
if [ "$COUNT" -eq 1 ]; then
  echo -e "${GREEN}PASSED (clamped to 1)${NC}"
else
  echo -e "${RED}FAILED (expected 1, got $COUNT)${NC}" && exit 1
fi

# A.3: Offset pagination (offset=1 skips first result)
echo -n "Test A.3: Offset pagination (offset=1): "
RES_ALL=$(curl -sG "${BROKER_URL}/ngsi-ld/v1/entities" --data-urlencode "limit=2" --data-urlencode "offset=0")
FIRST_ID=$(echo "$RES_ALL" | grep -o '"id":"[^"]*"' | head -n 1)
RES_OFFSET=$(curl -sG "${BROKER_URL}/ngsi-ld/v1/entities" --data-urlencode "limit=1" --data-urlencode "offset=1")
OFFSET_ID=$(echo "$RES_OFFSET" | grep -o '"id":"[^"]*"' | head -n 1)

if [ "$FIRST_ID" != "$OFFSET_ID" ] && [ -n "$OFFSET_ID" ]; then
  echo -e "${GREEN}PASSED (offset changed leading item)${NC}"
else
  echo -e "${RED}FAILED (first=$FIRST_ID, offset=$OFFSET_ID)${NC}" && exit 1
fi

# A.4: Offset beyond total rows returns empty array
echo -n "Test A.4: High offset beyond rows (offset=500): "
RES_HIGH=$(curl -sG "${BROKER_URL}/ngsi-ld/v1/entities" --data-urlencode "offset=500")
if [ "$RES_HIGH" = "[]" ]; then
  echo -e "${GREEN}PASSED (empty array [])${NC}"
else
  echo -e "${RED}FAILED (expected [], got $RES_HIGH)${NC}" && exit 1
fi

# ------------------------------------------------------------------------------
# TEST SUITE B: CONDITION CONCATENATION & PARAMETER CONTINUITY
# ------------------------------------------------------------------------------
echo -e "\n${BLUE}--- Suite B: Condition Concatenation ---${NC}"

# B.1: type + q filter + limit + offset
echo -n "Test B.1: Concatenation type=Sensor + q=co2>400 + limit=2 + offset=0: "
RES=$(curl -sG "${BROKER_URL}/ngsi-ld/v1/entities" \
  --data-urlencode "type=Sensor" \
  --data-urlencode "q=co2>400" \
  --data-urlencode "limit=2" \
  --data-urlencode "offset=0")
COUNT=$(echo "$RES" | grep -o '"id":' | wc -l | tr -d ' ')
if [ "$COUNT" -ge 1 ]; then
  echo -e "${GREEN}PASSED ($COUNT returned matching all criteria)${NC}"
else
  echo -e "${RED}FAILED (expected >=1)${NC}" && exit 1
fi

# B.2: PostGIS geoQ + q + limit
echo -n "Test B.2: Concatenation PostGIS geoQ + q=temperature>20 + limit=1: "
RES=$(curl -sG "${BROKER_URL}/ngsi-ld/v1/entities" \
  --data-urlencode "georel=near;maxDistance==50000" \
  --data-urlencode "geometry=Point" \
  --data-urlencode "coordinates=[12.4924,41.8902]" \
  --data-urlencode "q=temperature>20" \
  --data-urlencode "limit=1")
COUNT=$(echo "$RES" | grep -o '"id":' | wc -l | tr -d ' ')
if [ "$COUNT" -ge 1 ]; then
  echo -e "${GREEN}PASSED ($COUNT returned matching PostGIS and q)${NC}"
else
  echo -e "${RED}FAILED (expected >=1)${NC}" && exit 1
fi

# ------------------------------------------------------------------------------
# TEST SUITE C: TEMPORAL CONDITION CONCATENATION & EDGE CASES
# ------------------------------------------------------------------------------
echo -e "\n${BLUE}--- Suite C: Temporal Conditions & Edge Cases ---${NC}"

# C.1: timerel=between with timeAt and endTimeAt
echo -n "Test C.1: timerel=between with timeAt and endTimeAt: "
RES=$(curl -sG "${BROKER_URL}/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Sensor:Edge01" \
  --data-urlencode "timerel=between" \
  --data-urlencode "timeAt=2026-09-22T08:15:00Z" \
  --data-urlencode "endTimeAt=2026-09-22T09:45:00Z")
POINTS=$(echo "$RES" | grep -o '"observedAt":' | wc -l | tr -d ' ')
if [ "$POINTS" -eq 3 ]; then
  echo -e "${GREEN}PASSED (exactly 3 points in window [08:15, 09:45])${NC}"
else
  echo -e "${RED}FAILED (expected 3 points, got $POINTS)${NC}" && exit 1
fi

# C.2: lastN returns N most recent points in chronological order
echo -n "Test C.2: lastN=2 returns the 2 most recent observations: "
RES=$(curl -sG "${BROKER_URL}/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Sensor:Edge01" \
  --data-urlencode "timerel=after" \
  --data-urlencode "timeAt=2026-09-22T00:00:00Z" \
  --data-urlencode "lastN=2")
POINTS=$(echo "$RES" | grep -o '"observedAt":' | wc -l | tr -d ' ')
LAST_VAL=$(echo "$RES" | grep -o '"value":[0-9.]*' | tail -n 1)
if [ "$POINTS" -eq 2 ] && [ "$LAST_VAL" = '"value":425.0' ]; then
  echo -e "${GREEN}PASSED (2 most recent points, latest=425.0)${NC}"
else
  echo -e "${RED}FAILED (expected 2 points, got $POINTS with last $LAST_VAL)${NC}" && exit 1
fi

# C.3: Inverted time range (timeAt > endTimeAt) must return HTTP 400 Bad Request
echo -n "Test C.3: Inverted timerel=between range validation: "
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" -G "${BROKER_URL}/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Sensor:Edge01" \
  --data-urlencode "timerel=between" \
  --data-urlencode "timeAt=2026-09-25T00:00:00Z" \
  --data-urlencode "endTimeAt=2026-09-20T00:00:00Z")
if [ "$HTTP_CODE" -eq 400 ]; then
  echo -e "${GREEN}PASSED (HTTP 400 Bad Request returned)${NC}"
else
  echo -e "${RED}FAILED (expected HTTP 400, got $HTTP_CODE)${NC}" && exit 1
fi

# C.4: Temporal aggregation aggrMethod=avg
echo -n "Test C.4: Temporal aggregation aggrMethod=avg: "
RES_AVG=$(curl -sG "${BROKER_URL}/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Sensor:Edge01" \
  --data-urlencode "timerel=after" \
  --data-urlencode "timeAt=2026-09-22T00:00:00Z" \
  --data-urlencode "attrs=co2" \
  --data-urlencode "aggrMethod=avg")
if echo "$RES_AVG" | grep -q 'values'; then
  echo -e "${GREEN}PASSED (Aggregated values array returned)${NC}"
else
  echo -e "${RED}FAILED (expected values array, got $RES_AVG)${NC}" && exit 1
fi

# ------------------------------------------------------------------------------
# TEST SUITE D: N+1 TEMPORAL ENTITIES PAGINATION & SQL PUSHDOWN EDGE CASE
# ------------------------------------------------------------------------------
echo -e "\n${BLUE}--- Suite D: N+1 Temporal Entities Pagination (Pushdown Verification) ---${NC}"
RUN_ID=$(date +%s)
TYPE_D="TemporalEdgeDevice_${RUN_ID}"
N=5
TARGET_D="urn:ngsi-ld:${TYPE_D}:${RUN_ID}:Z_TARGET"
FILTER_BEFORE_D="2026-01-15T00:00:00Z"

echo -n "Test D.1: Seeding N+1 entities (target oldest on Page 2): "
# Target entity (oldest: Jan 2026)
curl -s -X POST "${BROKER_URL}/ngsi-ld/v1/entities" \
  -H "Content-Type: application/ld+json" \
  -d "{\"id\":\"${TARGET_D}\",\"type\":\"${TYPE_D}\",\"temperature\":{\"type\":\"Property\",\"value\":15.0},\"@context\":\"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld\"}" > /dev/null

curl -s -X POST "${BROKER_URL}/ngsi-ld/v1/temporal/entities" \
  -H "Content-Type: application/json" \
  -d "{\"id\":\"${TARGET_D}\",\"type\":\"${TYPE_D}\",\"temperature\":[{\"type\":\"Property\",\"value\":15.0,\"observedAt\":\"2026-01-01T10:00:00Z\"}]}" > /dev/null

# 5 newer entities (June 2026)
for i in 1 2 3 4 5; do
  curl -s -X POST "${BROKER_URL}/ngsi-ld/v1/entities" \
    -H "Content-Type: application/ld+json" \
    -d "{\"id\":\"urn:ngsi-ld:${TYPE_D}:${RUN_ID}:A0${i}\",\"type\":\"${TYPE_D}\",\"temperature\":{\"type\":\"Property\",\"value\":20.0},\"@context\":\"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld\"}" > /dev/null

  curl -s -X POST "${BROKER_URL}/ngsi-ld/v1/temporal/entities" \
    -H "Content-Type: application/json" \
    -d "{\"id\":\"urn:ngsi-ld:${TYPE_D}:${RUN_ID}:A0${i}\",\"type\":\"${TYPE_D}\",\"temperature\":[{\"type\":\"Property\",\"value\":20.0,\"observedAt\":\"2026-06-0${i}T10:00:00Z\"}]}" > /dev/null
done
echo -e "${GREEN}OK${NC}"

echo -n "Test D.2: Confirm target entity is outside Page 1 without temporal filter: "
BASE_PAGE1=$(curl -sG "${BROKER_URL}/ngsi-ld/v1/entities" --data-urlencode "type=${TYPE_D}" --data-urlencode "limit=${N}" --data-urlencode "offset=0")
if echo "$BASE_PAGE1" | grep -q "${TARGET_D}"; then
  echo -e "${RED}FAILED (Target was on Page 1)${NC}" && exit 1
else
  echo -e "${GREEN}PASSED (Target is on Page 2 in standard entity query)${NC}"
fi

echo -n "Test D.3: Pushdown returns target entity on Page 1 when temporal filter applied: "
TEMPORAL_RES=$(curl -sG "${BROKER_URL}/ngsi-ld/v1/temporal/entities" \
  --data-urlencode "type=${TYPE_D}" \
  --data-urlencode "timerel=before" \
  --data-urlencode "timeAt=${FILTER_BEFORE_D}" \
  --data-urlencode "limit=${N}" \
  --data-urlencode "offset=0")

if echo "$TEMPORAL_RES" | grep -q "${TARGET_D}"; then
  echo -e "${GREEN}PASSED (Target correctly returned on Page 1 via SQL Pushdown)${NC}"
else
  echo -e "${RED}FAILED (Target not found in temporal query response: $TEMPORAL_RES)${NC}" && exit 1
fi

echo -e "\n${GREEN}=== ALL SQL QUERY & TEMPORAL EDGE CASES PASSED (100%) ===${NC}"
