//! Bounded, single-publication MQTT sessions using rumqttc's wire codecs.
//! The outbox owns reconnection/retry. DNS is pinned by the outbound policy and
//! TLS validates the original hostname; no unchecked client-side re-resolution.
use athena_http::OutboundClient;
use athena_model::{mqtt::MqttEndpoint, Subscription};
use bytes::BytesMut;
use rumqttc::{
    mqttbytes::{self, v4},
    v5::mqttbytes::{self as mqtt5, v5},
};
use serde_json::{json, Value};
use std::sync::{Arc, OnceLock};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_rustls::{rustls, TlsConnector};

const MAX_ACK: usize = 65536;
const MAX_PACKET: usize = 65 * 1024 * 1024;
trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

pub fn envelope(sub: &Subscription, notification: &Value) -> Result<Value, String> {
    let content_type = sub
        .notification
        .endpoint
        .accept
        .as_deref()
        .unwrap_or("application/json");
    let mut metadata = serde_json::Map::new();
    // Preserve user key spelling: MQTT metadata keys are JSON, not HTTP headers.
    crate::dispatcher::receiver_headers(sub.notification.endpoint.receiver_info.as_ref())?;
    if let Some(info) = &sub.notification.endpoint.receiver_info {
        if let Some(entries) = info.as_array() {
            for entry in entries {
                metadata.insert(
                    entry["key"].as_str().unwrap().into(),
                    entry["value"].clone(),
                );
            }
        } else if let Some(headers) = info.get("headers").and_then(Value::as_object) {
            metadata.extend(headers.clone());
        }
    }
    metadata.insert("Content-Type".into(), json!(content_type));
    let mut body = notification.clone();
    if content_type == "application/ld+json" {
        body["@context"] = json!(athena_model::ETSI_CORE_CONTEXT_URL);
    } else {
        metadata.insert(
            "Link".into(),
            json!(format!(
                "<{}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"",
                athena_model::ETSI_CORE_CONTEXT_URL
            )),
        );
    }
    Ok(json!({"metadata":metadata,"body":body}))
}

#[derive(Clone)]
pub struct MqttClient {
    tls: Arc<rustls::ClientConfig>,
}
impl MqttClient {
    pub fn from_ca_file(path: Option<&str>) -> Result<Self, String> {
        static DEFAULT: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
        if let Some(path) = path {
            let file = std::fs::File::open(path).map_err(|_| "Cannot read MQTT CA file")?;
            if file
                .metadata()
                .map_err(|_| "Cannot inspect MQTT CA file")?
                .len()
                > 1024 * 1024
            {
                return Err("MQTT CA file exceeds 1 MiB".into());
            }
            let mut roots =
                rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let mut count = 0;
            for certificate in rustls_pemfile::certs(&mut std::io::BufReader::new(file)) {
                roots
                    .add(certificate.map_err(|_| "Invalid PEM certificate")?)
                    .map_err(|_| "Invalid MQTT CA certificate")?;
                count += 1;
            }
            if count == 0 {
                return Err("MQTT CA file contains no certificates".into());
            }
            return Ok(Self {
                tls: Self::tls_config(roots),
            });
        }
        Ok(Self {
            tls: DEFAULT
                .get_or_init(|| {
                    Self::tls_config(rustls::RootCertStore::from_iter(
                        webpki_roots::TLS_SERVER_ROOTS.iter().cloned(),
                    ))
                })
                .clone(),
        })
    }
    fn tls_config(roots: rustls::RootCertStore) -> Arc<rustls::ClientConfig> {
        Arc::new(
            rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .expect("Supported TLS versions")
            .with_root_certificates(roots)
            .with_no_client_auth(),
        )
    }
    pub async fn send(
        &self,
        sub: &Subscription,
        notification: &Value,
        client: &OutboundClient,
    ) -> Result<(), String> {
        let options = sub.notification.endpoint.mqtt_options()?;
        let payload = serde_json::to_vec(&envelope(sub, notification)?)
            .map_err(|_| "Invalid MQTT notification")?;
        tokio::time::timeout(
            client.timeout(),
            publish(&options, payload, client, self.tls.clone()),
        )
        .await
        .map_err(|_| "MQTT delivery timed out before protocol completion".to_string())?
    }
}

async fn publish(
    options: &MqttEndpoint,
    payload: Vec<u8>,
    client: &OutboundClient,
    tls: Arc<rustls::ClientConfig>,
) -> Result<(), String> {
    let tcp = client
        .policy()
        .connect_tcp(&options.host, options.port)
        .await?;
    tcp.set_nodelay(true)
        .map_err(|_| "Cannot configure MQTT socket")?;
    let mut stream: Box<dyn Stream> = if options.tls {
        let name = rustls::pki_types::ServerName::try_from(options.host.clone())
            .map_err(|_| "Invalid TLS server name")?;
        Box::new(
            TlsConnector::from(tls)
                .connect(name, tcp)
                .await
                .map_err(|_| "MQTT TLS handshake or certificate validation failed")?,
        )
    } else {
        Box::new(tcp)
    };
    if options.v5 {
        exchange5(&mut *stream, options, payload).await
    } else {
        exchange4(&mut *stream, options, payload).await
    }
}

async fn write(stream: &mut dyn Stream, buffer: &[u8]) -> Result<(), String> {
    stream
        .write_all(buffer)
        .await
        .map_err(|_| "MQTT socket write failed")?;
    stream
        .flush()
        .await
        .map_err(|_| "MQTT socket flush failed".into())
}
async fn read_more(stream: &mut dyn Stream, buffer: &mut BytesMut) -> Result<(), String> {
    let mut chunk = [0_u8; 4096];
    let n = stream
        .read(&mut chunk)
        .await
        .map_err(|_| "MQTT socket read failed")?;
    if n == 0 {
        return Err("MQTT peer disconnected before acknowledgement".into());
    }
    if buffer.len() + n > MAX_ACK + 5 {
        return Err("MQTT acknowledgement exceeds limit".into());
    }
    buffer.extend_from_slice(&chunk[..n]);
    Ok(())
}

async fn read4(stream: &mut dyn Stream, buffer: &mut BytesMut) -> Result<v4::Packet, String> {
    loop {
        match v4::Packet::read(buffer, MAX_ACK) {
            Ok(packet) => return Ok(packet),
            Err(mqttbytes::Error::InsufficientBytes(_)) => read_more(stream, buffer).await?,
            Err(_) => return Err("Invalid MQTT 3.1.1 response".into()),
        }
    }
}
async fn read5(stream: &mut dyn Stream, buffer: &mut BytesMut) -> Result<v5::Packet, String> {
    loop {
        match v5::Packet::read(buffer, Some(MAX_ACK as u32)) {
            Ok(packet) => return Ok(packet),
            Err(mqtt5::Error::InsufficientBytes(_)) => read_more(stream, buffer).await?,
            Err(_) => return Err("Invalid MQTT 5 response".into()),
        }
    }
}
async fn write4(stream: &mut dyn Stream, packet: v4::Packet) -> Result<(), String> {
    let mut bytes = BytesMut::new();
    packet
        .write(&mut bytes, MAX_PACKET)
        .map_err(|_| "Cannot encode MQTT 3.1.1 packet")?;
    write(stream, &bytes).await
}
async fn write5(stream: &mut dyn Stream, packet: v5::Packet, max_size: u32) -> Result<(), String> {
    let mut bytes = BytesMut::new();
    packet
        .write(&mut bytes, Some(max_size))
        .map_err(|_| "Cannot encode MQTT 5 packet within peer size limit")?;
    write(stream, &bytes).await
}
fn client_id() -> String {
    format!("athena{}", &uuid::Uuid::new_v4().simple().to_string()[..17])
}

async fn exchange4(
    stream: &mut dyn Stream,
    options: &MqttEndpoint,
    payload: Vec<u8>,
) -> Result<(), String> {
    let mut connect = v4::Connect::new(client_id());
    connect.keep_alive = 0;
    if !options.username.is_empty() || options.password.is_some() {
        connect.set_login(&options.username, options.password.as_deref().unwrap_or(""));
    }
    write4(stream, v4::Packet::Connect(connect)).await?;
    let mut buffer = BytesMut::new();
    match read4(stream, &mut buffer).await? {
        v4::Packet::ConnAck(ack)
            if ack.code == v4::ConnectReturnCode::Success && !ack.session_present => {}
        _ => return Err("MQTT connection was not accepted".into()),
    }
    let qos = match options.qos {
        0 => mqttbytes::QoS::AtMostOnce,
        1 => mqttbytes::QoS::AtLeastOnce,
        _ => mqttbytes::QoS::ExactlyOnce,
    };
    let mut publish = v4::Publish::new(&options.topic, qos, payload);
    if options.qos > 0 {
        publish.pkid = 1;
    }
    write4(stream, v4::Packet::Publish(publish)).await?;
    match options.qos {
        1 => {
            if !matches!(read4(stream,&mut buffer).await?,v4::Packet::PubAck(ack) if ack.pkid==1) {
                return Err("Expected MQTT PUBACK".into());
            }
        }
        2 => {
            if !matches!(read4(stream,&mut buffer).await?,v4::Packet::PubRec(ack) if ack.pkid==1) {
                return Err("Expected MQTT PUBREC".into());
            }
            write4(stream, v4::Packet::PubRel(v4::PubRel::new(1))).await?;
            if !matches!(read4(stream,&mut buffer).await?,v4::Packet::PubComp(ack) if ack.pkid==1) {
                return Err("Expected MQTT PUBCOMP".into());
            }
        }
        _ => {}
    }
    // Completion is already known. Failure of the courtesy DISCONNECT must not
    // turn a confirmed publish into a retry.
    let _ = write4(stream, v4::Packet::Disconnect).await;
    Ok(())
}
async fn exchange5(
    stream: &mut dyn Stream,
    options: &MqttEndpoint,
    payload: Vec<u8>,
) -> Result<(), String> {
    let connect = v5::Connect {
        keep_alive: 0,
        client_id: client_id(),
        clean_start: true,
        properties: Some(v5::ConnectProperties {
            max_packet_size: Some(MAX_ACK as u32),
            ..Default::default()
        }),
    };
    let login = (!options.username.is_empty() || options.password.is_some())
        .then(|| v5::Login::new(&options.username, options.password.as_deref().unwrap_or("")));
    write5(
        stream,
        v5::Packet::Connect(connect, None, login),
        MAX_PACKET as u32,
    )
    .await?;
    let mut buffer = BytesMut::new();
    let max_size = match read5(stream, &mut buffer).await? {
        v5::Packet::ConnAck(ack)
            if ack.code == v5::ConnectReturnCode::Success && !ack.session_present =>
        {
            let props = ack.properties;
            if props
                .as_ref()
                .and_then(|p| p.max_qos)
                .is_some_and(|max| options.qos > max)
            {
                return Err("MQTT peer does not support requested QoS".into());
            }
            props
                .and_then(|p| p.max_packet_size)
                .unwrap_or(MAX_PACKET as u32)
                .min(MAX_PACKET as u32)
        }
        _ => return Err("MQTT connection was not accepted".into()),
    };
    let qos = match options.qos {
        0 => mqtt5::QoS::AtMostOnce,
        1 => mqtt5::QoS::AtLeastOnce,
        _ => mqtt5::QoS::ExactlyOnce,
    };
    let mut publish = v5::Publish::new(&options.topic, qos, payload, None);
    if options.qos > 0 {
        publish.pkid = 1;
    }
    write5(stream, v5::Packet::Publish(publish), max_size).await?;
    match options.qos {
        1 => {
            if !matches!(read5(stream,&mut buffer).await?,v5::Packet::PubAck(ack) if ack.pkid==1 && matches!(ack.reason,v5::PubAckReason::Success | v5::PubAckReason::NoMatchingSubscribers))
            {
                return Err("MQTT PUBACK missing, mismatched or rejected".into());
            }
        }
        2 => {
            if !matches!(read5(stream,&mut buffer).await?,v5::Packet::PubRec(ack) if ack.pkid==1 && matches!(ack.reason,v5::PubRecReason::Success | v5::PubRecReason::NoMatchingSubscribers))
            {
                return Err("MQTT PUBREC missing, mismatched or rejected".into());
            }
            write5(
                stream,
                v5::Packet::PubRel(v5::PubRel::new(1, None)),
                max_size,
            )
            .await?;
            if !matches!(read5(stream,&mut buffer).await?,v5::Packet::PubComp(ack) if ack.pkid==1 && ack.reason==v5::PubCompReason::Success)
            {
                return Err("MQTT PUBCOMP missing, mismatched or rejected".into());
            }
        }
        _ => {}
    }
    let _ = write5(
        stream,
        v5::Packet::Disconnect(v5::Disconnect::new(
            v5::DisconnectReasonCode::NormalDisconnection,
        )),
        max_size,
    )
    .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn qos_waits_for_matching_successful_acknowledgements() {
        // Independent wire fixtures, fragmented CONNACK and deliberately missing,
        // negative and mismatched acknowledgements. No external broker required.
        for (v5, qos, ack, succeeds) in [
            (false, 1, vec![0x40, 2, 0, 1], true),
            (false, 1, vec![0x40, 2, 0, 2], false),
            (false, 1, vec![], false),
            (true, 1, vec![0x40, 2, 0, 1], true),
            (true, 1, vec![0x40, 3, 0, 1, 0x87], false),
            (true, 1, vec![0x40, 2, 0, 2], false),
            (true, 2, vec![0x50, 3, 0, 1, 0x87], false),
        ] {
            let (mut client, mut peer) = tokio::io::duplex(8192);
            let options = MqttEndpoint {
                host: "example.org".into(),
                port: 1883,
                tls: false,
                topic: "sensor/events".into(),
                username: String::new(),
                password: None,
                qos,
                v5,
            };
            let exchange = tokio::spawn(async move {
                if v5 {
                    exchange5(&mut client, &options, b"notification".to_vec()).await
                } else {
                    exchange4(&mut client, &options, b"notification".to_vec()).await
                }
            });
            let mut buf = BytesMut::new();
            if v5 {
                assert!(matches!(
                    read5(&mut peer, &mut buf).await.unwrap(),
                    v5::Packet::Connect(..)
                ));
            } else {
                assert!(matches!(
                    read4(&mut peer, &mut buf).await.unwrap(),
                    v4::Packet::Connect(_)
                ));
            }
            let connack = if v5 {
                vec![0x20, 3, 0, 0, 0]
            } else {
                vec![0x20, 2, 0, 0]
            };
            for byte in connack {
                peer.write_all(&[byte]).await.unwrap();
                tokio::task::yield_now().await;
            }
            if v5 {
                assert!(matches!(
                    read5(&mut peer, &mut buf).await.unwrap(),
                    v5::Packet::Publish(_)
                ));
            } else {
                assert!(matches!(
                    read4(&mut peer, &mut buf).await.unwrap(),
                    v4::Packet::Publish(_)
                ));
            }
            tokio::task::yield_now().await;
            assert!(
                !exchange.is_finished(),
                "a socket write is not a QoS acknowledgement"
            );
            peer.write_all(&ack).await.unwrap();
            if !succeeds {
                drop(peer);
            }
            let result = tokio::time::timeout(std::time::Duration::from_secs(1), exchange)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(result.is_ok(), succeeds, "version5={v5}, qos={qos}");
        }
    }
}
