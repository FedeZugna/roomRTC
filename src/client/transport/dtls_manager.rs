//! Gestor de DTLS para handshake y conexión segura.
//!
//! Este módulo maneja el proceso de handshake DTLS, verificación de fingerprints,
//! e inicialización de SCTP sobre la conexión DTLS establecida.

use crate::client::types::{SharedDtlsStream, SharedEndpoint, SharedSctpAssociation};
use openssl::hash::MessageDigest;
use openssl::ssl::{SslAcceptor, SslMethod, SslVerifyMode};
use sctp_proto::{
    ClientConfig as SctpClientConfig, Endpoint, EndpointConfig, Payload, ServerConfig,
    TransportConfig,
};
use std::io::Write;
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use udp_dtls::{DtlsAcceptor, DtlsConnector, DtlsStream, Identity, UdpChannel};

/// Lanza el hilo gestor de DTLS que espera confirmación de llamada y realiza el handshake.
///
/// Este hilo espera a recibir el rol (caller/callee) y el fingerprint remoto,
/// luego ejecuta el handshake DTLS, verifica el fingerprint del peer, y inicializa
/// el endpoint SCTP sobre la conexión DTLS establecida.
///
/// # Argumentos
/// * `dtls_stream` - Stream DTLS compartido donde se almacenará la conexión establecida.
/// * `running_call` - Flag atómico que se activa cuando la llamada está establecida.
/// * `socket_dtls` - Socket UDP para la comunicación DTLS.
/// * `sctp_endpoint` - Endpoint SCTP compartido a inicializar.
/// * `sctp_association` - Asociación SCTP compartida a inicializar.
/// * `identity` - Identidad DTLS (certificado y clave privada).
/// * `role_rx` - Canal para recibir el rol (caller=true/callee=false) y fingerprint remoto.
/// * `remote_addr` - Dirección del peer remoto.
/// * `sctp_max_users` - Número máximo de streams SCTP.
/// * `sctp_max_file_size` - Tamaño máximo de archivo para SCTP.
///
/// # Retorna
/// * `JoinHandle<()>` - Handle del hilo spawneado.
pub fn spawn_dtls_handshake_thread(
    dtls_stream: SharedDtlsStream,
    running_call: Arc<AtomicBool>,
    socket_dtls: UdpSocket,
    sctp_endpoint: SharedEndpoint,
    sctp_association: SharedSctpAssociation,
    identity: Identity,
    role_rx: Receiver<(bool, String)>,
    remote_addr: SocketAddr,
    sctp_max_users: u16,
    sctp_max_file_size: u32,
) -> JoinHandle<()> {
    let dtls_mgr_stream = Arc::clone(&dtls_stream);
    let dtls_mgr_running = Arc::clone(&running_call);
    let sctp_mgr_endpoint = Arc::clone(&sctp_endpoint);
    let sctp_mgr_association = Arc::clone(&sctp_association);
    let dtls_stream_for_sctp = Arc::clone(&dtls_mgr_stream);

    thread::spawn(move || {
        // Bloqueamos esperando el rol (esto ocurre cuando la llamada se acepta en Signaling)
        // Usamos recv() bloqueante porque este hilo solo sirve para iniciar DTLS
        println!("[DTLS Manager] Esperando confirmación de llamada...");

        let (am_i_caller, remote_fingerprint) = match role_rx.recv() {
            Ok(role) => role,
            Err(_) => {
                // El canal se cerró (app cerrada), salimos
                return;
            }
        };

        println!(
            "[DTLS] Iniciando Handshake. Rol: {}",
            if am_i_caller { "CLIENTE" } else { "SERVIDOR" }
        );

        // Asegurar que el socket esté en modo bloqueante para el handshake
        if let Err(e) = socket_dtls.set_nonblocking(false) {
            eprintln!("[DTLS] Error setting blocking mode: {}", e);
            return;
        }

        // Configurar timeout para que OpenSSL pueda manejar retransmisiones de DTLS si se pierden paquetes
        if let Err(e) = socket_dtls.set_read_timeout(Some(Duration::from_millis(200))) {
            eprintln!("[DTLS] Error setting read timeout: {}", e);
        }

        let channel = UdpChannel {
            socket: socket_dtls.try_clone().unwrap(),
            remote_addr,
        };

        // Ejecutar Handshake (Bloqueante)
        let mut handshake_result = if am_i_caller {
            DtlsConnector::builder()
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true)
                .build()
                .unwrap()
                .connect("room-rtc", channel)
                .map(|s| s.0)
        } else {
            // Construir SslAcceptor manualmente para deshabilitar verificación
            // Esta version que usamos de openssl no permite usar la nueva ParsedPkcs12_2. Ignorar warnings.
            let mut acceptor = SslAcceptor::mozilla_intermediate(SslMethod::dtls()).unwrap();
            #[allow(deprecated)]
            {
                acceptor.set_private_key(&identity.0.pkey).unwrap();
                acceptor.set_certificate(&identity.0.cert).unwrap();
            }
            acceptor.set_verify(SslVerifyMode::NONE);

            let dtls_acceptor = DtlsAcceptor(acceptor.build());
            dtls_acceptor.accept(channel).map(|s| s.0)
        };

        let stream_inner = loop {
            match handshake_result {
                Ok(s) => break s,
                Err(udp_dtls::HandshakeError::WouldBlock(mid)) => {
                    // Timeout del socket (200ms), reintentar handshake para manejar retransmisiones
                    handshake_result = mid.handshake().map(|s| s.0);
                }
                Err(e) => {
                    eprintln!("[DTLS] Falló Handshake: {:?}", e);
                    return;
                }
            }
        };

        let stream = DtlsStream(stream_inner);

        // VERIFICACIÓN DE FINGERPRINT (Seguridad contra MitM)
        let ssl = stream.0.ssl();
        if let Some(cert) = ssl.peer_certificate() {
            let digest = cert.digest(MessageDigest::sha256()).unwrap();
            let fp = digest
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<String>();
            if fp != remote_fingerprint {
                eprintln!(
                    "[DTLS] ALERTA DE SEGURIDAD: Fingerprint no coincide!\nEsperado: {}\nRecibido: {}",
                    remote_fingerprint, fp
                );
                return;
            }
            println!("[DTLS] Fingerprint verificado correctamente: {}", fp);
        }
        println!("[DTLS] Conexión segura establecida exitosamente.");

        if let Err(e) = socket_dtls.set_nonblocking(true) {
            eprintln!("[DTLS] Error set nonblocking: {}", e);
        }
        // Limpiar timeout al pasar a modo no bloqueante
        let _ = socket_dtls.set_read_timeout(None);

        *dtls_mgr_stream.lock().unwrap() = Some(stream);

        // TransportConfig para setear SCTP en caller y callee
        let transport = TransportConfig::default()
            .with_max_num_inbound_streams(sctp_max_users)
            .with_max_num_outbound_streams(sctp_max_users)
            .with_max_receive_buffer_size(sctp_max_file_size) // 4 MB para archivos grandes
            .with_max_message_size(sctp_max_file_size);

        // Inicializar Endpoint SCTP (Sans-IO)
        if am_i_caller {
            let ep_config = Arc::new(EndpointConfig::default());
            let mut endpoint = Endpoint::new(ep_config, None);

            let config = SctpClientConfig {
                transport: Arc::new(transport),
            };

            let (handle, mut assoc) = endpoint.connect(config, remote_addr).unwrap();

            println!("[SCTP] Endpoint cliente creado. Conectando...");

            // Enviar paquete INIT generado por connect
            while let Some(transmit) = assoc.poll_transmit(Instant::now())
                && let Ok(mut guard) = dtls_stream_for_sctp.lock()
            {
                if let Some(stream) = guard.as_mut()
                    && let Payload::RawEncode(data) = transmit.payload
                {
                    for chunk in data {
                        let _ = stream.write_all(&chunk);
                    }
                }
            }

            *sctp_mgr_endpoint.lock().unwrap() = Some(endpoint);
            *sctp_mgr_association.lock().unwrap() = Some((handle, assoc));
        } else {
            // Callee
            let ep_config = Arc::new(EndpointConfig::default());

            let mut server_config = ServerConfig::new();
            server_config.transport = Arc::new(
                TransportConfig::default()
                    .with_max_num_inbound_streams(sctp_max_users)
                    .with_max_num_outbound_streams(sctp_max_users)
                    .with_max_receive_buffer_size(sctp_max_file_size) // 4 MB para archivos grandes
                    .with_max_message_size(sctp_max_file_size),
            );

            // Endpoint servidor se crea con server_config
            let endpoint = Endpoint::new(ep_config, Some(Arc::new(server_config)));

            *sctp_mgr_endpoint.lock().unwrap() = Some(endpoint);
            println!("[SCTP] Endpoint servidor creado. Esperando conexión...");
        }

        // Activar flags de llamada
        dtls_mgr_running.store(true, Ordering::SeqCst);
    })
}
