# Periodic and MQTT notifications

Implemented contracts refer to ETSI NGSI-LD [periodic notification behaviour](https://cim.etsi.org/NGSI-LD/official/clause-5.html#notification-behaviour)
and the [MQTT notification binding](https://cim.etsi.org/NGSI-LD/official/clause-7.html).
These features do not establish full broker conformance.

## Create a periodic MQTT subscription

```http
POST /ngsi-ld/v1/subscriptions
Content-Type: application/json

{
  "id": "urn:ngsi-ld:Subscription:temperature-summary",
  "type": "Subscription",
  "entities": [{"type": "Sensor"}],
  "q": "temperature>20",
  "timeInterval": 10,
  "notification": {
    "attributes": ["temperature"],
    "endpoint": {
      "uri": "mqtts://mqtt.example.org/athena/temperatures",
      "accept": "application/ld+json",
      "notifierInfo": [
        {"key": "MQTT-Version", "value": "mqtt5.0"},
        {"key": "MQTT-QoS", "value": "1"}
      ],
      "receiverInfo": [{"key": "X-Source", "value": "athena"}]
    }
  }
}
```

Use an HTTP(S) URI for periodic HTTP delivery. Omit `timeInterval` for mutation-driven
delivery over either transport. MQTT ingestion and entity CRUD over MQTT are not
implemented; this is the notification binding, with subscription management over HTTP.

## Scheduling guarantees and limits

`timeInterval` accepts positive seconds, including fractions, up to one year. It cannot
be combined with `watchedAttributes` or `throttling`, and requires entity selectors.
The first deadline is one interval after subscription creation or PATCH. A PATCH
resets the schedule; notification counters do not. Removing `timeInterval` with
`urn:ngsi-ld:null` switches back to mutation-driven notifications.

Each run queries the current **local** entity set, applying type/id/idPattern, `q`,
`geoQ` and notification attribute projection. Overlapping selectors do not duplicate
entities. One database SELECT provides a consistent snapshot. Entities need not have
changed. An empty result does not produce a notification. Unsupported filters such
as `scopeQ` remain rejected; remote registered entities are not queried by this scheduler.

Schedule selection uses row locks with `SKIP LOCKED`. The complete snapshot, durable
job and next deadline commit together. A transaction failure leaves the deadline
recoverable by another worker or after restart. Pause, expiry and deletion prevent
new runs, and the delivery worker rechecks lifecycle before sending. An already
materialized job retains its original body/endpoint when a subscription is patched.

There is at most one pending periodic job per subscription. During a receiver outage,
the existing snapshot is retried; further ticks wait. On recovery, missed ticks are
coalesced into one current snapshot rather than replaying artificial historical states.
The next interval starts after snapshot generation, so this is not a wall-clock/crontab
scheduler. Effective timer resolution is `poll_interval_ms` (250 ms by default), and
generation/delivery time can add delay.

`[subscriptions]` controls:

| Setting | Default | Effect |
| --- | --- | --- |
| `scheduler_max_entities` | 10000 | Maximum complete matching set per notification |
| `scheduler_max_payload_bytes` | 4194304 | Bound on the generated notification body |
| `scheduler_pending_job_limit` | 100000 | Soft global pending-job threshold for scheduling |
| `poll_interval_ms` | 250 | Poll frequency and minimum effective schedule interval |
| `request_timeout_sec` | 10 | Complete HTTP or MQTT connect/TLS/publish deadline |
| `mqtt_ca_file` | unset | PEM file with additional trusted MQTT server CAs |

Results exceeding a cap are **not truncated**. The schedule retains its deadline and
records `last_error`, `attempts` and `retry_at` in `subscription_schedules`; retries
back off from five seconds to five minutes. Adjust the subscription filters or limits.
`athena_schedules_failed` and `athena_schedules_due` expose these conditions to monitoring.
Query statement timeout and the existing notification retry/lease settings also apply.
The queue threshold is backpressure, not a strict quota across replicas.

## MQTT transport

Endpoints use `mqtt://[username:password@]host[:1883]/topic` or
`mqtts://[username:password@]host[:8883]/topic`. Credentials/topic components are
percent-decoded; publish topics cannot contain `+` or `#`. Query parameters and URI
fragments are rejected. Secrets in subscription endpoints must be protected along
with the database and API access.

`notifierInfo` values are strings: MQTT-QoS `0`, `1`, `2`; MQTT-Version `mqtt3.1.1`
or `mqtt5.0`. Defaults are QoS 0 and MQTT 5.0. Invalid/duplicate/unknown settings are
rejected. The message is a JSON envelope with `metadata` and `body`. Metadata includes
`Content-Type`, `Link` for JSON, and receiver information; JSON-LD includes `@context`
in the body. Notification rendering currently uses the ETSI core context and canonical
attribute IRIs; custom context compaction/`jsonldContext` remain open work.

QoS 0 completion means bytes were written to the socket; it has no broker acknowledgement.
QoS 1 waits for the matching successful PUBACK. QoS 2 waits through PUBREC/PUBREL/PUBCOMP.
MQTT 5 rejection codes and peer packet-size/QoS restrictions are checked. These
acknowledgements indicate broker acceptance, not processing by downstream applications.
No matching MQTT subscriber is still a valid broker acceptance.

The outbox retains stable notification IDs across retries. Each attempt uses a fresh,
clean MQTT session and does not retain the publication. A process crash after broker
acceptance but before database completion can duplicate a notification, including at
QoS 2: consumers must deduplicate by notification ID. Retry/dead-letter policy applies.

TCP connections use the exact DNS addresses checked by the outbound policy. Private
addresses remain blocked unless `ALLOW_INTERNAL_ENDPOINTS=true` is explicitly configured.
TLS verifies certificate trust and the original hostname. Private CAs can be mounted
read-only and selected with `mqtt_ca_file`; there is no insecure verification switch.
CA changes require restart. Client certificates/mTLS and persistent connection pooling
are not implemented. Per-attempt connection setup is a current throughput limitation;
no MQTT production throughput claim is made by this release.

## Verification

```bash
bash scripts/test-integration.sh
bash scripts/start-mqtt-test.sh
ATHENA_TEST_MQTT_CA="$PWD/artifacts/mqtt-test/ca.crt" \
  cargo test --locked --test mqtt_integration -- --ignored --nocapture
docker stop athena-mqtt-integration
```

The fixture binds only loopback, generates short-lived test certificates, and writes
them under ignored `artifacts/mqtt-test`. Integration databases must be named
`athena_test`. Run suites and benchmarks sequentially on that isolated database.
