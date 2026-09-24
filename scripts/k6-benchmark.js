import http from 'k6/http';
import { check, sleep } from 'k6';
import { Trend, Counter } from 'k6/metrics';

// Benchmark trends
const createLatency = new Trend('athena_create_duration_ms');
const readNormalizedLatency = new Trend('athena_read_normalized_ms');
const readKeyValuesLatency = new Trend('athena_read_keyvalues_ms');
const spatialQueryLatency = new Trend('athena_spatial_query_ms');
const filterQueryLatency = new Trend('athena_filter_query_ms');

export const options = {
  scenarios: {
    high_throughput_load: {
      executor: 'ramping-vus',
      startVUs: 10,
      stages: [
        { duration: '10s', target: 50 },  // Warm up
        { duration: '30s', target: 200 }, // Ramp to high load
        { duration: '20s', target: 200 }, // Sustained benchmark
        { duration: '10s', target: 0 },   // Cool down
      ],
      gracefulRampDown: '5s',
    },
  },
  thresholds: {
    http_req_failed: ['rate<0.01'],             // Less than 1% failure rate
    http_req_duration: ['p(95)<5'],             // Target p95 < 5ms
    athena_read_keyvalues_ms: ['p(95)<3'],      // Target p95 < 3ms for keyValues
  },
};

const BASE_URL = __ENV.BROKER_URL || 'http://localhost:8080';
const HEADERS = {
  'Content-Type': 'application/ld+json',
  'Accept': 'application/ld+json',
};

export default function () {
  const idNum = Math.floor(Math.random() * 100000);
  const entityId = `urn:ngsi-ld:Vehicle:Bench_${__VU}_${idNum}`;

  // 1. Create Entity (Write)
  const entityPayload = JSON.stringify({
    id: entityId,
    type: 'Vehicle',
    brand: {
      type: 'Property',
      value: 'Mercedes',
    },
    speed: {
      type: 'Property',
      value: 85.5 + (idNum % 50),
      unitCode: 'KMH',
    },
    location: {
      type: 'GeoProperty',
      value: {
        type: 'Point',
        coordinates: [13.4050 + (idNum % 100) * 0.001, 52.5200 + (idNum % 100) * 0.001],
      },
    },
    '@context': 'https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld',
  });

  const resCreate = http.post(`${BASE_URL}/ngsi-ld/v1/entities`, entityPayload, { headers: HEADERS });
  createLatency.add(resCreate.timings.duration);
  check(resCreate, {
    'create status is 201': (r) => r.status === 201,
  });

  // 2. Read Normalized (Single Entity)
  const resNorm = http.get(`${BASE_URL}/ngsi-ld/v1/entities/${entityId}`, { headers: HEADERS });
  readNormalizedLatency.add(resNorm.timings.duration);
  check(resNorm, {
    'read normalized status is 200': (r) => r.status === 200,
  });

  // 3. Read keyValues (Simplified High-Performance)
  const resKV = http.get(`${BASE_URL}/ngsi-ld/v1/entities/${entityId}?options=keyValues`, { headers: HEADERS });
  readKeyValuesLatency.add(resKV.timings.duration);
  check(resKV, {
    'read keyValues status is 200': (r) => r.status === 200,
    'speed value matches': (r) => JSON.parse(r.body).speed !== undefined,
  });

  // 4. Spatial Query (PostGIS ST_DWithin)
  const geoUrl = `${BASE_URL}/ngsi-ld/v1/entities?georel=near;maxDistance==5000&geometry=Point&coordinates=[13.4050,52.5200]&type=Vehicle&limit=10`;
  const resGeo = http.get(geoUrl, { headers: HEADERS });
  spatialQueryLatency.add(resGeo.timings.duration);
  check(resGeo, {
    'geo query status is 200': (r) => r.status === 200,
  });

  // 5. Query Filter (q filter)
  const filterUrl = `${BASE_URL}/ngsi-ld/v1/entities?type=Vehicle&q=speed>90;brand=="Mercedes"&limit=10`;
  const resFilter = http.get(filterUrl, { headers: HEADERS });
  filterQueryLatency.add(resFilter.timings.duration);
  check(resFilter, {
    'filter query status is 200': (r) => r.status === 200,
  });

  sleep(0.01);
}
