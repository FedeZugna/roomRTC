//! Harness de test: dos endpoints de sctp-proto conectados en memoria, sin
//! sockets ni DTLS. Los paquetes de `poll_transmit` de un lado se entregan con
//! `Endpoint::handle` al otro, con un reloj virtual para los timers.
//!
//! Usa la misma configuración de transporte que la app (`dtls_manager.rs`).

use crate::client::types::{SCTP_MAX_FILE_SIZE, SCTP_MAX_USERS};
use bytes::Bytes;
use sctp_proto::{
    Association, AssociationHandle, ClientConfig, DatagramEvent, Endpoint, EndpointConfig, Event,
    Payload, ServerConfig, Transmit, TransportConfig,
};
use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

// Tope de pasos por `drive`, para no iterar para siempre si algo se traba.
const MAX_STEPS: usize = 10_000;
// Hasta dónde se adelanta el reloj virtual en un `drive` sin tráfico.
const TIMER_HORIZON: Duration = Duration::from_secs(10);

fn transport_config() -> TransportConfig {
    TransportConfig::default()
        .with_max_num_inbound_streams(SCTP_MAX_USERS)
        .with_max_num_outbound_streams(SCTP_MAX_USERS)
        .with_max_receive_buffer_size(SCTP_MAX_FILE_SIZE)
        .with_max_message_size(SCTP_MAX_FILE_SIZE)
}

/// Un extremo de la asociación.
pub(crate) struct Peer {
    endpoint: Endpoint,
    assoc: Option<(AssociationHandle, Association)>,
    addr: SocketAddr,
    inbound: VecDeque<Bytes>,
    pub(crate) connected: bool,
}

impl Peer {
    fn new(endpoint: Endpoint, port: u16) -> Self {
        Peer {
            endpoint,
            assoc: None,
            addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
            inbound: VecDeque::new(),
            connected: false,
        }
    }

    pub(crate) fn assoc_mut(&mut self) -> Option<&mut Association> {
        self.assoc.as_mut().map(|(_, assoc)| assoc)
    }

    /// Lee todos los mensajes recibidos, como `(stream_id, payload)`. El orden
    /// se preserva dentro de cada stream.
    pub(crate) fn take_received(&mut self) -> Vec<(u16, Vec<u8>)> {
        let mut out = Vec::new();
        let Some(assoc) = self.assoc_mut() else {
            return out;
        };
        let mut ids = assoc.stream_ids();
        ids.sort_unstable();
        for id in ids {
            let Ok(mut stream) = assoc.stream(id) else {
                continue;
            };
            while let Ok(Some(chunks)) = stream.read_sctp() {
                let mut buf = vec![0u8; chunks.len()];
                if let Ok(n) = chunks.read(&mut buf) {
                    buf.truncate(n);
                    out.push((id, buf));
                }
            }
        }
        out
    }

    // Procesa lo que llegó y devuelve los paquetes a entregar al otro extremo.
    fn step(&mut self, remote: SocketAddr, now: Instant) -> Vec<Bytes> {
        while let Some(packet) = self.inbound.pop_front() {
            match self.endpoint.handle(now, remote, None, None, packet) {
                Some((handle, DatagramEvent::NewAssociation(assoc))) => {
                    self.assoc = Some((handle, assoc));
                }
                Some((handle, DatagramEvent::AssociationEvent(event))) => {
                    if let Some((h, assoc)) = self.assoc.as_mut()
                        && *h == handle
                    {
                        assoc.handle_event(event);
                    }
                }
                None => {}
            }
        }

        let mut outbound = Vec::new();
        if let Some((handle, assoc)) = self.assoc.as_mut() {
            if assoc.poll_timeout().is_some_and(|t| t <= now) {
                assoc.handle_timeout(now);
            }
            while let Some(event) = assoc.poll_endpoint_event() {
                if let Some(event) = self.endpoint.handle_event(*handle, event) {
                    assoc.handle_event(event);
                }
            }
            while let Some(event) = assoc.poll() {
                if let Event::Connected = event {
                    self.connected = true;
                }
            }
            while let Some(transmit) = assoc.poll_transmit(now) {
                push_payload(&mut outbound, transmit);
            }
        }
        while let Some(transmit) = self.endpoint.poll_transmit() {
            push_payload(&mut outbound, transmit);
        }
        outbound
    }

    fn next_timeout(&self) -> Option<Instant> {
        self.assoc
            .as_ref()
            .and_then(|(_, assoc)| assoc.poll_timeout())
    }
}

fn push_payload(outbound: &mut Vec<Bytes>, transmit: Transmit) {
    if let Payload::RawEncode(chunks) = transmit.payload {
        outbound.extend(chunks);
    }
}

/// Dos extremos: `a` inicia la asociación (como el caller) y `b` la acepta
/// (como el callee).
pub(crate) struct SctpPair {
    pub(crate) a: Peer,
    pub(crate) b: Peer,
    now: Instant,
}

impl SctpPair {
    /// Crea los dos extremos y completa el handshake SCTP.
    pub(crate) fn connect() -> Result<SctpPair, String> {
        let ep_config = Arc::new(EndpointConfig::default());
        let mut server_config = ServerConfig::new();
        server_config.transport = Arc::new(transport_config());

        let mut a = Peer::new(Endpoint::new(Arc::clone(&ep_config), None), 5000);
        let b = Peer::new(
            Endpoint::new(ep_config, Some(Arc::new(server_config))),
            5001,
        );

        let client_config = ClientConfig {
            transport: Arc::new(transport_config()),
        };
        let (handle, assoc) = a
            .endpoint
            .connect(client_config, b.addr)
            .map_err(|e| format!("connect falló: {e:?}"))?;
        a.assoc = Some((handle, assoc));

        let mut pair = SctpPair {
            a,
            b,
            now: Instant::now(),
        };
        pair.drive();
        if pair.a.connected && pair.b.connected {
            Ok(pair)
        } else {
            Err(format!(
                "no se conectó: a.connected={}, b.connected={}",
                pair.a.connected, pair.b.connected
            ))
        }
    }

    /// Entrega paquetes en ambos sentidos hasta que no hay más tráfico,
    /// adelantando el reloj virtual a los timers pendientes (retransmisiones,
    /// SACK demorados) dentro de un horizonte acotado.
    pub(crate) fn drive(&mut self) {
        let horizon = self.now + TIMER_HORIZON;
        for _ in 0..MAX_STEPS {
            let to_b = self.a.step(self.b.addr, self.now);
            let to_a = self.b.step(self.a.addr, self.now);
            let moved = !to_b.is_empty() || !to_a.is_empty();
            self.b.inbound.extend(to_b);
            self.a.inbound.extend(to_a);
            if moved {
                continue;
            }
            let next = match (self.a.next_timeout(), self.b.next_timeout()) {
                (Some(x), Some(y)) => Some(x.min(y)),
                (x, y) => x.or(y),
            };
            match next {
                Some(t) if t <= horizon => self.now = self.now.max(t),
                _ => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_conecta_dos_endpoints_en_memoria() -> Result<(), String> {
        let pair = SctpPair::connect()?;

        assert!(pair.a.connected);
        assert!(pair.b.connected);
        Ok(())
    }
}
