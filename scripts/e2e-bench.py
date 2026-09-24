#!/usr/bin/env python3
import time
import json
import statistics
import urllib.request
import urllib.error
from concurrent.futures import ThreadPoolExecutor

BROKER_URL = "http://localhost:8080"

def seed_entity():
    payload = {
        "id": "urn:ngsi-ld:Vehicle:BenchPilot",
        "type": "Vehicle",
        "brand": {"type": "Property", "value": "Mercedes"},
        "speed": {"type": "Property", "value": 88.5, "unitCode": "KMH"},
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.4050, 52.5200]}
        },
        "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
    }
    req = urllib.request.Request(
        f"{BROKER_URL}/ngsi-ld/v1/entities",
        data=json.dumps(payload).encode('utf-8'),
        headers={"Content-Type": "application/ld+json"}
    )
    try:
        urllib.request.urlopen(req)
    except Exception:
        pass

def run_bench(name, method, url, body=None, headers=None, total_requests=2000, concurrency=20):
    if headers is None:
        headers = {}

    latencies = []
    errors = 0

    def make_req(req_id):
        nonlocal errors
        data = body(req_id) if callable(body) else body
        t0 = time.perf_counter()
        req = urllib.request.Request(url, data=data, headers=headers, method=method)
        try:
            with urllib.request.urlopen(req, timeout=5) as resp:
                _ = resp.read()
                dur_ms = (time.perf_counter() - t0) * 1000.0
                return dur_ms
        except Exception as e:
            errors += 1
            return None

    # Warmup 50 requests
    for _ in range(50):
        try:
            req = urllib.request.Request(url, data=body(0) if callable(body) else body, headers=headers, method=method)
            with urllib.request.urlopen(req, timeout=5) as resp:
                _ = resp.read()
        except Exception:
            pass

    t_start = time.perf_counter()
    with ThreadPoolExecutor(max_workers=concurrency) as pool:
        results = pool.map(make_req, range(total_requests))
        latencies = [r for r in results if r is not None]
    t_total = time.perf_counter() - t_start

    successful = len(latencies)
    rps = successful / t_total if t_total > 0 else 0
    latencies.sort()

    p50 = statistics.median(latencies) if latencies else 0
    p90 = latencies[int(len(latencies) * 0.90)] if latencies else 0
    p95 = latencies[int(len(latencies) * 0.95)] if latencies else 0
    p99 = latencies[int(len(latencies) * 0.99)] if latencies else 0
    mean = statistics.mean(latencies) if latencies else 0

    print(f"\n[{name}]")
    print(f"  Total: {total_requests} reqs | Concurrency: {concurrency} workers")
    print(f"  Success: {successful}/{total_requests} ({100.0 * successful / total_requests:.1f}%) | Errors: {errors}")
    print(f"  Duration: {t_total:.2f} s | Throughput: {rps:.1f} req/s")
    print(f"  Latency (ms): Mean={mean:.2f}ms | p50={p50:.2f}ms | p90={p90:.2f}ms | p95={p95:.2f}ms | p99={p99:.2f}ms")

    return {
        "name": name,
        "total": total_requests,
        "concurrency": concurrency,
        "rps": rps,
        "mean_ms": mean,
        "p50_ms": p50,
        "p90_ms": p90,
        "p95_ms": p95,
        "p99_ms": p99,
        "errors": errors
    }

def main():
    print("================================================================================")
    print("      ATHENA NGSI-LD BROKER: BENCHMARK PRESTAZIONI END-TO-END HTTP + SQL       ")
    print("================================================================================")
    seed_entity()

    # 1. Healthcheck (raw Tokio/Axum network pipeline)
    run_bench(
        "1. Raw Tokio/Axum HTTP Pipeline (/health)",
        "GET",
        f"{BROKER_URL}/health",
        total_requests=4000,
        concurrency=30
    )

    # 2. Point Read keyValues (PostgreSQL read + simplified JSON projection)
    run_bench(
        "2. Point Read keyValues (/entities/{id}?options=keyValues)",
        "GET",
        f"{BROKER_URL}/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:BenchPilot?options=keyValues",
        headers={"Accept": "application/json"},
        total_requests=2500,
        concurrency=25
    )

    # 3. Point Read Normalized (PostgreSQL read + full ETSI NGSI-LD normalization)
    run_bench(
        "3. Point Read Normalized (/entities/{id})",
        "GET",
        f"{BROKER_URL}/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:BenchPilot",
        headers={"Accept": "application/ld+json"},
        total_requests=2500,
        concurrency=25
    )

    # 4. Complex Query with q filter (Lexer -> AST Parser -> SQL parameterization -> PostgreSql JSONB)
    run_bench(
        "4. Q-Filter Complex Query (/entities?type=Vehicle&q=speed>80;brand=='Mercedes')",
        "GET",
        f"{BROKER_URL}/ngsi-ld/v1/entities?type=Vehicle&q=speed%3E80%3Bbrand%3D%3D%27Mercedes%27&limit=10",
        headers={"Accept": "application/ld+json"},
        total_requests=1500,
        concurrency=20
    )

    # 5. Spatial PostGIS Query (GeoParser -> PostGIS ST_DWithin / ST_Distance)
    run_bench(
        "5. PostGIS Geo-Spatial Query (/entities?georel=near;maxDistance==5000)",
        "GET",
        f"{BROKER_URL}/ngsi-ld/v1/entities?georel=near%3BmaxDistance%3D%3D5000&geometry=Point&coordinates=%5B13.4050%2C52.5200%5D&type=Vehicle&limit=10",
        headers={"Accept": "application/ld+json"},
        total_requests=1500,
        concurrency=20
    )

    # 6. Temporal Pushdown Query (SQL JOIN e.id = t.entity_id + time filter + limit/offset)
    run_bench(
        "6. Temporal Pushdown Query (/temporal/entities?type=Sensor&timerel=after)",
        "GET",
        f"{BROKER_URL}/ngsi-ld/v1/temporal/entities?type=Sensor&timerel=after&timeAt=2026-01-01T00%3A00%3A00Z&limit=10",
        headers={"Accept": "application/json"},
        total_requests=1500,
        concurrency=20
    )

    # 7. Entity Creation Ingestion (Writes to PostgreSQL + PostGIS location indexing)
    def make_body(req_id):
        entity = {
            "id": f"urn:ngsi-ld:Vehicle:LoadTest_{time.time_ns()}_{req_id}",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 70.0 + (req_id % 40)},
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [12.4924 + (req_id % 100) * 0.0001, 41.8902]}
            },
            "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
        }
        return json.dumps(entity).encode('utf-8')

    run_bench(
        "7. Entity Creation Ingestion (POST /entities con PostGIS Indexing)",
        "POST",
        f"{BROKER_URL}/ngsi-ld/v1/entities",
        body=make_body,
        headers={"Content-Type": "application/ld+json"},
        total_requests=1000,
        concurrency=15
    )

    print("\n================================================================================")
    print("                         BENCHMARK COMPLETATO CON SUCCESSO                      ")
    print("================================================================================")

if __name__ == "__main__":
    main()
