#!/usr/bin/env bash
set -euo pipefail

BROKER_URL="${BROKER_URL:-http://localhost:8080}"
echo "=================================================="
echo " Running Athena NGSI-LD Broker Compliance Tests   "
echo " Target URL: ${BROKER_URL}                        "
echo "=================================================="

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

pass() {
    echo -e "${GREEN}✓ PASS:${NC} $1"
}

fail() {
    echo -e "${RED}✗ FAIL:${NC} $1"
    exit 1
}

# 1. Health check
echo "Checking broker health..."
STATUS=$(curl -s -o /dev/null -w "%{http_code}" "${BROKER_URL}/health")
if [ "$STATUS" -eq 200 ]; then
    pass "Health check returned 200 OK"
else
    fail "Health check failed with status $STATUS"
fi

# 2. Create Entity
ENTITY_ID="urn:ngsi-ld:Building:Test01"
curl -s -X DELETE "${BROKER_URL}/ngsi-ld/v1/entities/${ENTITY_ID}" >/dev/null 2>&1 || true
echo "Creating entity ${ENTITY_ID}..."
CREATE_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X POST "${BROKER_URL}/ngsi-ld/v1/entities" \
    -H "Content-Type: application/ld+json" \
    -d '{
        "id": "'"${ENTITY_ID}"'",
        "type": "Building",
        "temperature": {
            "type": "Property",
            "value": 23.5,
            "unitCode": "CEL"
        },
        "location": {
            "type": "GeoProperty",
            "value": {
                "type": "Point",
                "coordinates": [13.4050, 52.5200]
            }
        },
        "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
    }')

if [ "$CREATE_CODE" -eq 201 ]; then
    pass "Entity created with status 201 Created"
else
    fail "Create entity failed with status $CREATE_CODE"
fi

# 3. Read Entity (Normalized)
echo "Reading entity ${ENTITY_ID} (normalized)..."
NORM_RESP=$(curl -s "${BROKER_URL}/ngsi-ld/v1/entities/${ENTITY_ID}" -H "Accept: application/ld+json")
if echo "$NORM_RESP" | grep -q '"type":"Building"'; then
    pass "Read normalized entity returned valid structure"
else
    fail "Read normalized failed: $NORM_RESP"
fi

# 4. Read Entity (keyValues)
echo "Reading entity ${ENTITY_ID} (keyValues)..."
KV_RESP=$(curl -s "${BROKER_URL}/ngsi-ld/v1/entities/${ENTITY_ID}?options=keyValues" -H "Accept: application/ld+json")
if echo "$KV_RESP" | grep -q '"temperature":23.5'; then
    pass "Read keyValues entity returned simplified attributes"
else
    fail "Read keyValues failed: $KV_RESP"
fi

# 5. Query Entities with filter 'q'
echo "Querying entities with filter q=temperature>20..."
Q_RESP=$(curl -s -G "${BROKER_URL}/ngsi-ld/v1/entities" \
    --data-urlencode "type=Building" \
    --data-urlencode "q=temperature>20" \
    -H "Accept: application/ld+json")
if echo "$Q_RESP" | grep -q "${ENTITY_ID}"; then
    pass "Query with q filter successfully matched entity"
else
    fail "Query with q filter failed: $Q_RESP"
fi

# 6. Query Entities with geoQ
echo "Querying entities with geoQ near;maxDistance==2000..."
GEO_RESP=$(curl -s -G "${BROKER_URL}/ngsi-ld/v1/entities" \
    --data-urlencode "type=Building" \
    --data-urlencode "georel=near;maxDistance==2000" \
    --data-urlencode "geometry=Point" \
    --data-urlencode "coordinates=[13.4050,52.5200]" \
    -H "Accept: application/ld+json")
if echo "$GEO_RESP" | grep -q "${ENTITY_ID}"; then
    pass "Query with geoQ successfully matched entity within 2000m"
else
    fail "Query with geoQ failed: $GEO_RESP"
fi

# 7. Update Entity Attributes
echo "Updating temperature attribute..."
UPDATE_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X PATCH "${BROKER_URL}/ngsi-ld/v1/entities/${ENTITY_ID}/attrs" \
    -H "Content-Type: application/ld+json" \
    -d '{
        "temperature": {
            "type": "Property",
            "value": 26.0
        },
        "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
    }')

if [ "$UPDATE_CODE" -eq 204 ]; then
    pass "Entity attribute updated with status 204 No Content"
else
    fail "Update attribute failed with status $UPDATE_CODE"
fi

# 8. Clean up / Delete Entity
echo "Deleting entity ${ENTITY_ID}..."
DEL_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE "${BROKER_URL}/ngsi-ld/v1/entities/${ENTITY_ID}")
if [ "$DEL_CODE" -eq 204 ]; then
    pass "Entity deleted with status 204 No Content"
else
    fail "Delete entity failed with status $DEL_CODE"
fi

echo "=================================================="
echo -e "${GREEN} All Athena NGSI-LD Compliance Tests Passed! ${NC}"
echo "=================================================="
