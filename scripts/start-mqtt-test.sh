#!/usr/bin/env bash
# Disposable local fixture; certificates/keys stay in ignored artifacts, never in source.
set -euo pipefail
fixture_dir="$(pwd)/artifacts/mqtt-test"
mkdir -p "$fixture_dir"
chmod 700 "$fixture_dir"
umask 077
cat > "$fixture_dir/openssl.cnf" <<'EOF'
[req]
distinguished_name = dn
[dn]
[ca]
basicConstraints = critical,CA:true
keyUsage = critical,keyCertSign,cRLSign
[server]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature,keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = DNS:localhost
EOF
openssl req -x509 -newkey rsa:2048 -nodes -sha256 -days 2 -subj '/CN=Athena disposable MQTT test CA' \
  -config "$fixture_dir/openssl.cnf" -extensions ca -keyout "$fixture_dir/ca.key" -out "$fixture_dir/ca.crt" 2> "$fixture_dir/openssl.log"
openssl req -new -newkey rsa:2048 -nodes -sha256 -subj '/CN=localhost' \
  -keyout "$fixture_dir/server.key" -out "$fixture_dir/server.csr" 2>> "$fixture_dir/openssl.log"
openssl x509 -req -sha256 -days 2 -in "$fixture_dir/server.csr" -CA "$fixture_dir/ca.crt" -CAkey "$fixture_dir/ca.key" \
  -CAcreateserial -extfile "$fixture_dir/openssl.cnf" -extensions server -out "$fixture_dir/server.crt" 2>> "$fixture_dir/openssl.log"
cat > "$fixture_dir/mosquitto.conf" <<'EOF'
persistence false
allow_anonymous true
listener 1883
listener 8883
certfile /fixtures/server.crt
keyfile /fixtures/server.key
EOF
docker run --rm -d --name athena-mqtt-integration --user "$(id -u):$(id -g)" \
  -p 127.0.0.1:18884:1883 -p 127.0.0.1:18885:8883 \
  -v "$fixture_dir:/fixtures:ro" eclipse-mosquitto:2 mosquitto -c /fixtures/mosquitto.conf
printf 'Fixture started. Run:\nATHENA_TEST_MQTT_CA=%q cargo test --test mqtt_integration -- --ignored --nocapture\nStop: docker stop athena-mqtt-integration\n' "$fixture_dir/ca.crt"
