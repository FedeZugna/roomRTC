use room_rtc::chat::CHAT_MAX_BYTES;
use room_rtc::client::transport::sctp_chat::{
    ChatSendOutcome, FileControl, Incoming, notify_without_association, route_incoming, send_chat,
};
use room_rtc::client::types::SCTP_MAX_FILE_SIZE;
use room_rtc::client::{
    ClientResult, ClienteICELite, MediaClientConfig, Message, SCTP_MAX_USERS, SharedDtlsStream,
    SharedEndpoint, SharedSctpAssociation, TlsClient, UserStatus, setup_iced_video_client,
    spawn_dtls_handshake_thread,
};

use room_rtc::media::audio;
use room_rtc::media::codec::vp8::EncodedFrame;
use room_rtc::media::constants::{VIDEO_HEIGHT, VIDEO_WIDTH};
use room_rtc::media::jitter_buffer::{JitterBuffer, JitterBufferConfig, JitterFrame};
use room_rtc::rtcp::RtpRecvStats;
use room_rtc::rtp::{RTP_LITE_HEADER_SIZE, RtpLitePacket, RtpLiteReassembler, RtpLiteSender};
use room_rtc::sdp::SessionDescription;

use rand::Rng;
use std::env;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use openssl::asn1::Asn1Time;
use openssl::hash::MessageDigest;
use openssl::pkcs12::Pkcs12;
use openssl::pkey::PKey;
use openssl::rsa::Rsa;
use openssl::x509::X509;
use udp_dtls::{DtlsStream, Identity, UdpChannel};

use bytes::Bytes;
use sctp_proto::{DatagramEvent, Event, Payload, PayloadProtocolIdentifier};

// ==================== PROTOCOLO SEÑALIZACIÓN ====================

/// Genera identidad temporal autofirmada para udp_dtls
fn generate_local_identity() -> Result<(Identity, String), Box<dyn std::error::Error>> {
    let rsa = Rsa::generate(2048)?;
    let pkey = PKey::from_rsa(rsa)?;
    let mut x509 = X509::builder()?;
    x509.set_version(2)?;
    x509.set_pubkey(&pkey)?;
    let not_before = Asn1Time::days_from_now(0)?;
    let not_after = Asn1Time::days_from_now(365)?;
    x509.set_not_before(&not_before)?;
    x509.set_not_after(&not_after)?;
    x509.sign(&pkey, MessageDigest::sha256())?;
    let cert = x509.build();

    let digest = cert.digest(MessageDigest::sha256())?;
    let fingerprint = digest.iter().map(|b| format!("{:02X}", b)).collect();

    let pkcs12 = Pkcs12::builder()
        .name("room-rtc")
        .pkey(&pkey)
        .cert(&cert)
        .build2("password")?;
    let derivation = pkcs12.to_der()?;
    Ok((Identity::from_pkcs12(&derivation, "password")?, fingerprint))
}

// ==================== CLIENTE MEDIA ====================

/// Ejecuta el pipeline de envío/recepción + ventana iced (igual para ambos roles)
/// Generico para controlling y controlled asi no se repite codigo
fn run_media_client(cliente: &ClienteICELite, config: MediaClientConfig) -> ClientResult<()> {
    // Extract config fields for use in this function
    let fps = config.fps;
    let width = config.width;
    let height = config.height;
    let pending_auth = config.pending_auth;
    let auth_result = config.auth_result;
    let signaling_tx = config.signaling_tx;
    let signaling_response = config.signaling_response;

    // Setup Video (Encoder/Decoder)
    let (rx_enc, tx_dec, local_frame_buffer, remote_frame_buffer) =
        setup_iced_video_client(fps, &config.source_pipeline, width, height)?;

    // Estado compartido
    let rtp_stats = Arc::new(Mutex::new(RtpRecvStats::default()));
    let quality_state = Arc::new(Mutex::new(0.0));
    let running_call = Arc::new(AtomicBool::new(false));
    // Generación de la llamada actual: la incrementa la UI al entrar a una llamada
    let call_generation = Arc::new(AtomicU64::new(0));
    let shutdown = Arc::new(AtomicBool::new(false));

    // El Stream DTLS compartido protegido por Mutex
    let dtls_stream: SharedDtlsStream = Arc::new(Mutex::new(None));
    // El Endpoint y Handle SCTP compartidos
    let sctp_endpoint: SharedEndpoint = Arc::new(Mutex::new(None));
    let sctp_association: SharedSctpAssociation = Arc::new(Mutex::new(None));

    // Canal para comandos SCTP desde la UI
    let (sctp_cmd_tx, sctp_cmd_rx) =
        std::sync::mpsc::channel::<room_rtc::media::unified_app::SctpCommand>();

    // Canal para notificaciones SCTP hacia la UI
    let (sctp_notif_tx, sctp_notif_rx) =
        std::sync::mpsc::channel::<room_rtc::media::unified_app::SctpNotification>();
    let sctp_notif_rx = Arc::new(Mutex::new(sctp_notif_rx));

    // Estado compartido de archivos pendientes (filename -> path)
    let pending_files: Arc<Mutex<std::collections::HashMap<String, String>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));

    // Cola de archivos que necesitan ser enviados (después de aceptación)
    let files_to_send: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));

    // ================================================================
    //               HILO GESTOR DTLS (HANDSHAKE)
    //               + INICIALIZACIÓN SCTP
    // ================================================================
    // Espera la señal de ROL para iniciar la conexión segura
    let socket_dtls = cliente.socket_rx.try_clone()?; // Clonamos socket UDP original
    let remote_addr = cliente.direccion_remota;
    let identity = config.identity;
    let role_rx = config.role_rx;

    // Lanzar hilo gestor DTLS/SCTP
    let _dtls_thread = spawn_dtls_handshake_thread(
        Arc::clone(&dtls_stream),
        Arc::clone(&running_call),
        socket_dtls,
        Arc::clone(&sctp_endpoint),
        Arc::clone(&sctp_association),
        identity,
        role_rx,
        remote_addr,
        SCTP_MAX_USERS,
        SCTP_MAX_FILE_SIZE,
    );

    // ================================================================
    //               HILO TX: VIDEO -> DTLS
    // ================================================================
    let tx_dtls: Arc<Mutex<Option<DtlsStream<UdpChannel>>>> = Arc::clone(&dtls_stream);
    let tx_running = Arc::clone(&running_call);
    let tx_shutdown = Arc::clone(&shutdown);
    let stats_for_tx = Arc::clone(&rtp_stats);
    let qs_for_tx = Arc::clone(&quality_state);

    let tx_thread = thread::spawn(move || {
        let mut rtp_sender = RtpLiteSender::new(96, rand::rng().random::<u32>(), 1200);
        let mut ts: u32 = rand::rng().random();
        let mut adaptive_drop_factor: i32;
        let mut frame_cnt = 0;

        for frame in rx_enc {
            if tx_shutdown.load(Ordering::Relaxed) {
                break;
            }
            if !tx_running.load(Ordering::Relaxed) {
                continue;
            }

            // Lógica Adaptativa simple
            let loss = if let Ok(s) = stats_for_tx.lock() {
                s.snapshot_report().fraction_lost
            } else {
                0.0
            };
            if let Ok(mut q) = qs_for_tx.lock() {
                *q = loss;
            }
            adaptive_drop_factor = if loss > 0.10 { 2 } else { 1 };

            if frame_cnt % adaptive_drop_factor != 0 {
                frame_cnt += 1;
                continue;
            }
            frame_cnt += 1;

            let pkts = rtp_sender.packetize_frame(&frame.data, ts);

            // Enviar por DTLS
            // Intentamos bloquear el mutex brevemente
            if let Ok(mut guard) = tx_dtls.lock()
                && let Some(stream) = guard.as_mut()
            {
                for pkt in pkts {
                    if let Err(e) = stream.write_all(&pkt) {
                        // Ignoramos WouldBlock en escritura UDP
                        if e.kind() != std::io::ErrorKind::WouldBlock {
                            eprintln!("[TX] Error: {}", e);
                        }
                    }
                }
            }
            ts = ts.wrapping_add(3000);
        }
    });

    // ================================================================
    //               HILO RX: DTLS -> VIDEO + RTCP
    // ================================================================
    let rx_dtls: Arc<Mutex<Option<DtlsStream<UdpChannel>>>> = Arc::clone(&dtls_stream);
    let rx_running = Arc::clone(&running_call);
    let rx_shutdown = Arc::clone(&shutdown);
    let stats_for_rx = Arc::clone(&rtp_stats);

    let sctp_endpoint_for_rx = Arc::clone(&sctp_endpoint);
    let sctp_association_for_rx = Arc::clone(&sctp_association);
    let remote_addr_for_rx = remote_addr;

    // Jitter Buffer
    let jitter_buf = Arc::new(Mutex::new(JitterBuffer::new(JitterBufferConfig {
        target_delay: Duration::from_millis(80),
        max_delay: Duration::from_millis(200),
    })));
    let jitter_for_cons = Arc::clone(&jitter_buf);

    let sctp_notif_tx_for_rx = sctp_notif_tx.clone();
    let rx_call_generation = Arc::clone(&call_generation);
    let pending_files_rx = Arc::clone(&pending_files);
    let files_to_send_rx = Arc::clone(&files_to_send);
    let rx_thread = thread::spawn(move || {
        let mut reasm = RtpLiteReassembler::new();
        let mut buf = vec![0u8; 1500];
        let mut last_sctp_tick = Instant::now();

        // Buffers internos para transferencia de archivos
        let mut file_buffer: Vec<u8> = Vec::new();
        let mut current_file_name = String::new();
        let mut current_file_size = 0usize;
        let mut file_offers: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        loop {
            if rx_shutdown.load(Ordering::Relaxed) {
                break;
            }
            if !rx_running.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(50));
                continue;
            }

            // Lectura con Mutex
            let res = {
                if let Ok(mut guard) = rx_dtls.lock() {
                    if let Some(stream) = guard.as_mut() {
                        stream.read(&mut buf)
                    } else {
                        Err(std::io::Error::new(
                            std::io::ErrorKind::NotConnected,
                            "No stream",
                        ))
                    }
                } else {
                    Err(std::io::Error::other("Lock fail"))
                }
            };

            match res {
                Ok(n) => {
                    let data = &buf[..n];
                    let arrival = Instant::now();

                    // Discriminar tipo de paquete
                    // IMPORTANTE: Estamos usando RtpLite, no RTP estándar
                    // RtpLite: primer byte = (marker << 7) | payload_type
                    // Para payload_type=96 (0x60):
                    //   - Sin marker: 0x60 (0b01100000)
                    //   - Con marker: 0xE0 (0b11100000)

                    enum PacketType {
                        RtpLite,
                        Sctp,
                    }

                    let packet_type = if !data.is_empty() {
                        let first_byte = data[0];
                        let payload_type = first_byte & 0x7F;

                        // Verificar RtpLite (payload_type 96)
                        if payload_type == 96 && data.len() >= RTP_LITE_HEADER_SIZE {
                            Some(PacketType::RtpLite)
                        }
                        // Si no es RTP y tiene tamaño mínimo SCTP
                        else if data.len() >= 12 {
                            Some(PacketType::Sctp)
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    match packet_type {
                        Some(PacketType::Sctp) => {
                            let mut endpoint_guard = sctp_endpoint_for_rx.lock().unwrap();
                            if let Some(endpoint) = endpoint_guard.as_mut() {
                                let res = endpoint.handle(
                                    Instant::now(),
                                    remote_addr_for_rx,
                                    None,
                                    None,
                                    Bytes::copy_from_slice(data),
                                );

                                if let Some((handle, event)) = res {
                                    match event {
                                        DatagramEvent::NewAssociation(mut assoc) => {
                                            println!(
                                                "[SCTP RX] Nueva asociación aceptada (Handle {:?})",
                                                handle
                                            );
                                            // Sondear paquetes inmediatos (ej. INIT ACK)
                                            while let Some(transmit) =
                                                assoc.poll_transmit(Instant::now())
                                            {
                                                if let Ok(mut guard) = rx_dtls.lock()
                                                    && let Some(stream) = guard.as_mut()
                                                    && let Payload::RawEncode(data) =
                                                        transmit.payload
                                                {
                                                    for chunk in data {
                                                        let _ = stream.write_all(&chunk);
                                                    }
                                                }
                                            }
                                            // Guardar la asociación
                                            *sctp_association_for_rx.lock().unwrap() =
                                                Some((handle, assoc));
                                        }
                                        DatagramEvent::AssociationEvent(ae) => {
                                            // Buscar la asociación existente y procesar el evento
                                            let mut assoc_guard =
                                                sctp_association_for_rx.lock().unwrap();
                                            if let Some((h, assoc)) = assoc_guard.as_mut()
                                                && *h == handle
                                            {
                                                assoc.handle_event(ae);

                                                // 2. Procesar eventos de la asociación (DATA, Connected, etc.)
                                                while let Some(event) = assoc.poll() {
                                                    match event {
                                                        Event::DatagramReceived => {
                                                            for id in assoc.stream_ids() {
                                                                let mut stream =
                                                                    assoc.stream(id).unwrap();
                                                                while let Ok(Some(d)) =
                                                                    stream.read_sctp()
                                                                {
                                                                    // Get the total length of data
                                                                    let data_len = d.len();

                                                                    // Create a buffer to read the data into
                                                                    let mut buffer =
                                                                        vec![0u8; data_len];

                                                                    if let Ok(bytes_read) =
                                                                        d.read(&mut buffer)
                                                                    {
                                                                        let data =
                                                                            &buffer[..bytes_read];

                                                                        println!(
                                                                            "{} bytes",
                                                                            d.len()
                                                                        );
                                                                        println!(
                                                                            "[SCTP RX] Datos recibidos en stream {}: {:?}",
                                                                            id, d
                                                                        );

                                                                        match route_incoming(
                                                                            id,
                                                                            data,
                                                                            rx_call_generation.load(Ordering::SeqCst),
                                                                            CHAT_MAX_BYTES,
                                                                    ) {
                                                                        Incoming::FileContent(data) => {
                                                                                // Stream 1: Archivos
                                                                                // Si el buffer está vacío, esperamos cabecera [Len(2)][Name][Content]
                                                                                if file_buffer
                                                                                    .is_empty()
                                                                                {
                                                                                    println!(
                                                                                        "[SCTP RX] Inicio de transferencia de archivo: {} bytes. Esperando cabecera...\n",
                                                                                        data.len()
                                                                                    );
                                                                                    if data.len() > 2 {
                                                                                        let name_len = u16::from_be_bytes([data[0], data[1]]) as usize;
                                                                                        if data.len() >= 2 + name_len {
                                                                                            let name_bytes = &data[2..2+name_len];
                                                                                            let content = &data[2+name_len..];
                                                                                            let filename = String::from_utf8_lossy(name_bytes).to_string();
                                                                                            current_file_name = filename.clone();
                                                                                            current_file_size = *file_offers.get(&filename).unwrap_or(&0);
                                                                                            file_buffer.extend_from_slice(content);
                                                                                        }
                                                                                    }
                                                                                } else {
                                                                                    // Continuación de archivo (chunks sin cabecera)
                                                                                    file_buffer.extend_from_slice(data);
                                                                                }

                                                                                println!(
                                                                                    "[SCTP RX] Buffer progress: {} / {} bytes for file '{}'",
                                                                                    file_buffer.len(),
                                                                                    current_file_size,
                                                                                    current_file_name
                                                                                );

                                                                                // Verificar si se completó la descarga
                                                                                if !current_file_name.is_empty() && file_buffer.len() >= current_file_size {
                                                                                    let save_name = format!("received_{}", current_file_name);
                                                                                    if std::fs::write(&save_name, &file_buffer).is_ok() {
                                                                                        println!("[SCTP RX] Archivo guardado: {} ({} bytes)", save_name, file_buffer.len());
                                                                                        let _ = sctp_notif_tx_for_rx.send(
                                                                                            room_rtc::media::unified_app::SctpNotification::FileTransferComplete {
                                                                                                filename: current_file_name.clone(),
                                                                                            }
                                                                                        );
                                                                                    } else {
                                                                                        eprintln!("[SCTP RX] Error guardando archivo");
                                                                                        let _ = sctp_notif_tx_for_rx.send(
                                                                                            room_rtc::media::unified_app::SctpNotification::FileTransferFailed {
                                                                                                filename: current_file_name.clone(),
                                                                                                error: "Failed to save file".to_string(),
                                                                                            }
                                                                                        );
                                                                                    }
                                                                                    // Resetear estado
                                                                                    file_buffer.clear();
                                                                                    current_file_name.clear();
                                                                                    current_file_size = 0;
                                                                                }
                                                                        }
                                                                        Incoming::FileControl(control) => {
                                                                            // Stream 0: Mensajes de control de archivos
                                                                            println!("[SCTP RX] Control message: {:?}", control);
                                                                            match control {
                                                                                FileControl::Offer { filename, size } => {
                                                                                    println!("[SCTP RX] File offer received: {} ({} bytes)", filename, size);
                                                                                    file_offers.insert(filename.clone(), size);
                                                                                    let _ = sctp_notif_tx_for_rx.send(
                                                                                        room_rtc::media::unified_app::SctpNotification::FileOffer {
                                                                                            filename,
                                                                                            size,
                                                                                        },
                                                                                    );
                                                                                }
                                                                                FileControl::Accept { filename } => {
                                                                                    println!("[SCTP RX] File acceptance received for: {}", filename);
                                                                                    // Buscar el archivo pendiente y agregarlo a la cola de envío
                                                                                    if let Ok(mut pending) = pending_files_rx.lock() {
                                                                                        if let Some(path) = pending.remove(&filename) {
                                                                                            println!("[SCTP RX] Queuing file for transmission: {}", path);
                                                                                            // Agregar a la cola que el TX thread procesará
                                                                                            if let Ok(mut queue) = files_to_send_rx.lock() {
                                                                                                queue.push((filename, path));
                                                                                            }
                                                                                        } else {
                                                                                            println!("[SCTP RX] No pending file found for: {}", filename);
                                                                                        }
                                                                                    }
                                                                                }
                                                                                FileControl::Reject { filename } => {
                                                                                    println!("[SCTP RX] File rejected: {}", filename);
                                                                                    // Remover archivo pendiente
                                                                                    if let Ok(mut pending) = pending_files_rx.lock() {
                                                                                        pending.remove(&filename);
                                                                                    }
                                                                                }
                                                                            }
                                                                        }
                                                                        Incoming::Chat(notif) => {
                                                                            // Stream de chat: la UI descarta lo que no es de la llamada actual
                                                                            let _ = sctp_notif_tx_for_rx.send(notif);
                                                                        }
                                                                        Incoming::Ignored => {
                                                                            // Otros streams, control desconocido o chat inválido: Log
                                                                            println!("[SCTP RX] Mensaje en stream {}: {:?}", id, d);
                                                                        }
                                                                    }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                        Event::Connected => {
                                                            println!(
                                                                "[SCTP RX] Asociación conectada!"
                                                            );
                                                            // Abrir stream 1 para transferencia de archivos (bidireccional)
                                                            if let Err(e) = assoc.open_stream(
                                                                1,
                                                                PayloadProtocolIdentifier::Binary,
                                                            ) {
                                                                eprintln!(
                                                                    "[SCTP RX] Error abriendo stream 1: {:?}",
                                                                    e
                                                                );
                                                            } else {
                                                                println!(
                                                                    "[SCTP RX] Stream 1 abierto para transferencia de archivos"
                                                                );
                                                            }
                                                        }

                                                        _ => {}
                                                    }
                                                }

                                                // 3. Sondear paquetes de respuesta (SACKs, etc.)
                                                while let Some(transmit) =
                                                    assoc.poll_transmit(Instant::now())
                                                {
                                                    if let Ok(mut guard) = rx_dtls.lock()
                                                        && let Some(stream) = guard.as_mut()
                                                        && let Payload::RawEncode(data) =
                                                            transmit.payload
                                                    {
                                                        for chunk in data {
                                                            let _ = stream.write_all(&chunk);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                // 4. Sondear paquetes del Endpoint (ej. ABORTs para paquetes malformados/desconocidos)
                                while let Some(transmit) = endpoint.poll_transmit()
                                    && let Ok(mut guard) = rx_dtls.lock()
                                    && let Some(stream) = guard.as_mut()
                                {
                                    if let Payload::RawEncode(data) = transmit.payload {
                                        for chunk in data {
                                            let _ = stream.write_all(&chunk);
                                        }
                                    }
                                }
                            }
                        }

                        Some(PacketType::RtpLite) => {
                            if let Some(pkt) = RtpLitePacket::parse(data) {
                                // [RTP] Paquete de video recibido
                                if let Ok(mut s) = stats_for_rx.lock() {
                                    s.on_rtp_packet(
                                        pkt.header.ssrc,
                                        pkt.header.sequence_number,
                                        pkt.header.timestamp,
                                        arrival,
                                    );
                                }
                                if let Some(vp8) = reasm.push_packet(&pkt)
                                    && let Ok(mut jb) = jitter_buf.lock()
                                {
                                    jb.push(JitterFrame {
                                        timestamp_90khz: pkt.header.timestamp,
                                        arrival,
                                        data: vp8,
                                    });
                                }
                            }
                        }

                        // Otros tipos de paquetes (RTCP, Unknown): ignorar
                        None => {}
                    }
                }
                Err(e) => {
                    // Manejo CRÍTICO de WouldBlock para no congelar la app
                    if e.kind() == std::io::ErrorKind::WouldBlock {
                        // Cedemos el turno para que el TX thread pueda obtener el lock
                        thread::sleep(Duration::from_millis(1));
                    } else if e.kind() != std::io::ErrorKind::NotConnected {
                        eprintln!("[RX] Error lectura: {}", e);
                    } else {
                        thread::sleep(Duration::from_millis(20));
                    }
                }
            }

            // Mantenimiento periódico de SCTP (Retransmisiones, Heartbeats)
            // Es vital llamar a poll_transmit periódicamente aunque no lleguen datos
            if last_sctp_tick.elapsed() >= Duration::from_millis(100) {
                last_sctp_tick = Instant::now();
                if let Ok(mut assoc_guard) = sctp_association_for_rx.lock()
                    && let Some((_, assoc)) = assoc_guard.as_mut()
                {
                    while let Some(transmit) = assoc.poll_transmit(Instant::now()) {
                        if let Ok(mut guard) = rx_dtls.lock()
                            && let Some(stream) = guard.as_mut()
                            && let Payload::RawEncode(data) = transmit.payload
                        {
                            for chunk in data {
                                let _ = stream.write_all(&chunk);
                            }
                        }
                    }
                }
            }
        }
    });

    // ================================================================
    //               HILO TX SCTP: ENVIAR DATOS (EJ. ARCHIVO)
    // ================================================================
    let sctp_tx_association = Arc::clone(&sctp_association);
    let sctp_tx_dtls = Arc::clone(&dtls_stream);
    let sctp_tx_running = Arc::clone(&running_call);
    let sctp_tx_shutdown = Arc::clone(&shutdown);
    let pending_files_tx = Arc::clone(&pending_files);
    let files_to_send_tx = Arc::clone(&files_to_send);
    let sctp_notif_tx_for_tx = sctp_notif_tx.clone();
    let tx_call_generation = Arc::clone(&call_generation);

    thread::spawn(move || {
        use room_rtc::media::unified_app::SctpCommand;

        loop {
            if sctp_tx_shutdown.load(Ordering::Relaxed) {
                break;
            }

            // Esperar comando de la UI con timeout corto para polling periódico
            let cmd_opt = match sctp_cmd_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(cmd) => Some(cmd),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            };

            if !sctp_tx_running.load(Ordering::Relaxed) {
                if cmd_opt.is_some() {
                    eprintln!("[SCTP TX] Ignorando comando: llamada no activa");
                }
                continue;
            }

            let mut assoc_guard = sctp_tx_association.lock().unwrap();
            if let Some((_, assoc)) = assoc_guard.as_mut() {
                // Procesar comando si existe
                if let Some(cmd) = cmd_opt {
                    match cmd {
                        SctpCommand::SendFile { path } => {
                            println!("[SCTP TX] Enviando oferta de archivo: {}", path);
                            match std::fs::metadata(&path) {
                                Ok(metadata) => {
                                    // 1. Obtener nombre de archivo y tamaño
                                    let path_obj = std::path::Path::new(&path);
                                    let filename = path_obj
                                        .file_name()
                                        .and_then(|s| s.to_str())
                                        .unwrap_or("unknown.bin");
                                    let file_size = metadata.len() as usize;

                                    // 2. Guardar el path para cuando acepten
                                    if let Ok(mut pending) = pending_files_tx.lock() {
                                        pending.insert(filename.to_string(), path.clone());
                                    }

                                    // 3. Enviar oferta en stream 0
                                    let offer_msg = format!("OFFER {} {}", filename, file_size);
                                    let offer_bytes = Bytes::from(offer_msg.as_bytes().to_vec());

                                    let stream_res = if let Ok(stream) = assoc.stream(0) {
                                        Ok(stream)
                                    } else {
                                        assoc.open_stream(0, PayloadProtocolIdentifier::String)
                                    };

                                    if let Ok(mut stream) = stream_res {
                                        if let Err(e) = stream.write_sctp(
                                            &offer_bytes,
                                            PayloadProtocolIdentifier::String,
                                        ) {
                                            eprintln!("[SCTP TX] Error enviando oferta: {:?}", e);
                                        } else {
                                            println!(
                                                "[SCTP TX] Oferta enviada: {} ({} bytes)",
                                                filename, file_size
                                            );
                                        }
                                    } else {
                                        eprintln!(
                                            "[SCTP TX] Error abriendo/obteniendo stream 0: {:?}",
                                            stream_res.err()
                                        );
                                    }
                                }
                                Err(e) => eprintln!("[SCTP TX] Error accediendo archivo: {}", e),
                            }
                        }
                        SctpCommand::AcceptFile { filename } => {
                            println!("[SCTP TX] Aceptando archivo: {}", filename);
                            // Enviar mensaje de aceptación en stream 0
                            let accept_msg = format!("ACCEPT {}", filename);
                            let accept_bytes = Bytes::from(accept_msg.as_bytes().to_vec());

                            let stream_res = if let Ok(stream) = assoc.stream(0) {
                                Ok(stream)
                            } else {
                                assoc.open_stream(0, PayloadProtocolIdentifier::String)
                            };

                            if let Ok(mut stream) = stream_res {
                                if let Err(e) = stream
                                    .write_sctp(&accept_bytes, PayloadProtocolIdentifier::String)
                                {
                                    eprintln!("[SCTP TX] Error enviando aceptación: {:?}", e);
                                } else {
                                    println!("[SCTP TX] Aceptación enviada: {}", filename);
                                }
                            }
                        }
                        SctpCommand::RejectFile { filename } => {
                            println!("[SCTP TX] Rechazando archivo: {}", filename);
                            // Enviar mensaje de rechazo en stream 0
                            let reject_msg = format!("REJECT {}", filename);
                            let reject_bytes = Bytes::from(reject_msg.as_bytes().to_vec());

                            let stream_res = if let Ok(stream) = assoc.stream(0) {
                                Ok(stream)
                            } else {
                                assoc.open_stream(0, PayloadProtocolIdentifier::String)
                            };

                            if let Ok(mut stream) = stream_res {
                                if let Err(e) = stream
                                    .write_sctp(&reject_bytes, PayloadProtocolIdentifier::String)
                                {
                                    eprintln!("[SCTP TX] Error enviando rechazo: {:?}", e);
                                } else {
                                    println!("[SCTP TX] Rechazo enviado: {}", filename);
                                }
                            }
                        }
                        SctpCommand::SendChat {
                            id,
                            generation,
                            text,
                        } => {
                            // Enviar chat por su stream dedicado
                            match send_chat(
                                assoc,
                                id,
                                generation,
                                &text,
                                tx_call_generation.load(Ordering::SeqCst),
                                sctp_tx_running.load(Ordering::Relaxed),
                            ) {
                                Ok(ChatSendOutcome::Sent) => {
                                    println!("[SCTP TX] Chat enviado (id {})", id);
                                }
                                Ok(ChatSendOutcome::Discarded) => {
                                    println!(
                                        "[SCTP TX] Chat de otra llamada descartado (id {})",
                                        id
                                    );
                                }
                                Err(e) => {
                                    eprintln!("[SCTP TX] Error enviando chat: {}", e.reason);
                                    let _ = sctp_notif_tx_for_tx.send(e.to_notification());
                                }
                            }
                        }
                    }
                }

                // Procesar cola de archivos aceptados para enviar
                if let Ok(mut queue) = files_to_send_tx.try_lock() {
                    while let Some((filename, path)) = queue.pop() {
                        println!("[SCTP TX] Processing accepted file: {}", path);
                        match std::fs::read(&path) {
                            Ok(file_data) => {
                                let filename_bytes = filename.as_bytes();
                                let name_len = filename_bytes.len() as u16;

                                // Construir payload: [Len(2)][Name][Content]
                                let mut payload_vec =
                                    Vec::with_capacity(2 + filename_bytes.len() + file_data.len());
                                payload_vec.extend_from_slice(&name_len.to_be_bytes());
                                payload_vec.extend_from_slice(filename_bytes);
                                payload_vec.extend_from_slice(&file_data);
                                let payload = Bytes::from(payload_vec);

                                println!(
                                    "[SCTP TX] Sending file: {} ({} bytes)",
                                    filename,
                                    payload.len()
                                );

                                // Enviar por stream 1
                                let stream_res = if let Ok(stream) = assoc.stream(1) {
                                    Ok(stream)
                                } else {
                                    assoc.open_stream(1, PayloadProtocolIdentifier::Binary)
                                };

                                if let Ok(mut stream) = stream_res {
                                    if let Err(e) = stream
                                        .write_sctp(&payload, PayloadProtocolIdentifier::Binary)
                                    {
                                        eprintln!("[SCTP TX] Error enqueueing file: {:?}", e);
                                    } else {
                                        println!(
                                            "[SCTP TX] File enqueued in SCTP: {} bytes",
                                            payload.len()
                                        );
                                    }
                                } else {
                                    eprintln!("[SCTP TX] Error opening stream 1");
                                }
                            }
                            Err(e) => {
                                eprintln!("[SCTP TX] Error reading file {}: {}", path, e);
                            }
                        }
                    }
                }

                // Procesar timeouts de SCTP (importante para retransmisiones y ACKs)
                assoc.handle_timeout(Instant::now());

                // Sondear paquetes para enviar (DATA chunks, SACKs, etc.)
                while let Some(transmit) = assoc.poll_transmit(Instant::now()) {
                    let mut dtls_guard = sctp_tx_dtls.lock().unwrap();
                    if let Some(dtls) = dtls_guard.as_mut()
                        && let Payload::RawEncode(data) = transmit.payload
                    {
                        for chunk in data {
                            // Solo logear paquetes grandes (DATA), no control (SACK/HB)
                            if chunk.len() > 100 {
                                println!("[SCTP TX] Enviando DATA de {} bytes", chunk.len());
                            }
                            let _ = dtls.write_all(&chunk);
                        }
                    }
                }
            } else if let Some(cmd) = cmd_opt {
                eprintln!("[SCTP TX] Error: No hay asociación SCTP activa.");
                // Un chat sin asociación se reporta para que no se pierda en silencio
                if let Some(notif) = notify_without_association(&cmd) {
                    let _ = sctp_notif_tx_for_tx.send(notif);
                }
            }
        }
    });

    // ================================================================
    //                              AUDIO
    // ================================================================
    // Audio usa UDP en puertos +2
    // Local audio bind:  local_video_port + 2
    // Remote audio dst : remote_video_port + 2
    let audio_port_offset: u16 = 2;

    let audio_local = SocketAddr::new(
        cliente.direccion_local.ip(),
        cliente
            .direccion_local
            .port()
            .saturating_add(audio_port_offset),
    );
    let audio_remote = SocketAddr::new(
        cliente.direccion_remota.ip(),
        cliente
            .direccion_remota
            .port()
            .saturating_add(audio_port_offset),
    );

    // Mute/unmute
    let audio_muted = Arc::new(AtomicBool::new(false));

    {
        let muted_flag = Arc::clone(&audio_muted);
        let shut = Arc::clone(&shutdown);
        std::thread::spawn(move || {
            // Loop simple: lee stdin pero es un hilo aparte.
            let mut buf = String::new();
            while !shut.load(Ordering::Relaxed) {
                buf.clear();
                if std::io::stdin().read_line(&mut buf).is_err() {
                    break;
                }
                if buf.trim().eq_ignore_ascii_case("m") {
                    let new_val = !muted_flag.load(Ordering::Relaxed);
                    muted_flag.store(new_val, Ordering::Relaxed);
                    //aviso por terminal si esta muteado o no
                    println!("[AUDIO] muted={}", new_val);
                }
            }
        });
    }

    let audio_shut = Arc::clone(&shutdown);
    // Encoder mic -> opus (bytes)
    let audio_rx_opus = match audio::start_audio_encoder(
        Arc::clone(&audio_muted),
        Arc::clone(&audio_shut),
    ) {
        Ok(rx) => {
            println!(
                "[AUDIO] Encoder iniciado (mic -> opus). Local={audio_local} Remote={audio_remote}"
            );
            Some(rx)
        }
        Err(e) => {
            eprintln!("[AUDIO] No se pudo iniciar encoder (mic). Continuo sin audio TX. Err={e}");
            None
        }
    };

    // Player opus -> speakers
    let audio_tx_player = match audio::start_audio_player(Arc::clone(&audio_shut)) {
        Ok(tx) => {
            println!("[AUDIO] Player iniciado (opus -> speakers).");
            Some(tx)
        }
        Err(e) => {
            eprintln!("[AUDIO] No se pudo iniciar player. Continuo sin audio RX. Err={e}");
            None
        }
    };

    // Socket UDP audio RX/TX
    let audio_sock = {
        // Intento bindear local; si falla (puerto ocupado), uso ephemeral para no frenar todo.
        match UdpSocket::bind(audio_local) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[AUDIO] bind({audio_local}) falló ({e}), uso puerto efímero.");
                UdpSocket::bind(("0.0.0.0", 0))?
            }
        }
    };
    audio_sock.set_nonblocking(true)?;
    let audio_sock_tx = audio_sock.try_clone()?;

    // --- AUDIO TX THREAD (mic->opus bytes -> udp send) ---
    let audio_tx_thread = {
        let shut = Arc::clone(&shutdown);
        let call = Arc::clone(&running_call);
        let mut maybe_rx = audio_rx_opus;
        thread::spawn(move || {
            // Si no hay encoder, no hacemos nada
            let Some(rx) = maybe_rx.take() else {
                return;
            };

            // Enviamos solo si hay llamada activa
            for pkt in rx {
                if shut.load(Ordering::Relaxed) {
                    break;
                }
                if !call.load(Ordering::Relaxed) {
                    continue;
                }

                if let Err(e) = audio_sock_tx.send_to(&pkt, audio_remote)
                    && e.kind() != ErrorKind::WouldBlock
                {
                    eprintln!("[AUDIO TX] send_to error: {e}");
                }
            }
        })
    };

    // --- AUDIO RX THREAD (udp recv -> opus bytes -> player) ---
    let audio_rx_thread = {
        let shut = Arc::clone(&shutdown);
        let call = Arc::clone(&running_call);
        let mut maybe_player = audio_tx_player;
        thread::spawn(move || {
            let Some(player_tx) = maybe_player.take() else {
                return;
            };

            let mut buf = vec![0u8; 2048];
            loop {
                if shut.load(Ordering::Relaxed) {
                    break;
                }
                if !call.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }

                match audio_sock.recv_from(&mut buf) {
                    Ok((n, _src)) => {
                        if n == 0 {
                            continue;
                        }
                        // mandamos bytes Opus al pipeline de reproducción
                        if player_tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(e) => {
                        eprintln!("[AUDIO RX] recv error: {e}");
                        break;
                    }
                }
            }
        })
    };

    // ================================================================
    //               HILO RTCP TX (Reportes)
    // ================================================================
    let rtcp_dtls: Arc<Mutex<Option<DtlsStream<UdpChannel>>>> = Arc::clone(&dtls_stream);
    let rtcp_running = Arc::clone(&running_call);
    let rtcp_shutdown = Arc::clone(&shutdown);
    let rtcp_interval = Duration::from_millis(cliente.rtcp_interval_ms);
    let rtcp_enabled = cliente.rtcp_enabled;

    let rtcp_tx_thread = thread::spawn(move || {
        if !rtcp_enabled {
            return;
        }
        loop {
            if rtcp_shutdown.load(Ordering::Relaxed) {
                break;
            }
            thread::sleep(rtcp_interval);

            if !rtcp_running.load(Ordering::Relaxed) {
                continue;
            }

            // Construir reporte dummy (o usar librería real)
            let report = vec![128, 201, 0, 1]; // Header RTCP RR genérico

            if let Ok(mut guard) = rtcp_dtls.lock()
                && let Some(stream) = guard.as_mut()
            {
                let _ = stream.write_all(&report);
            }
        }
    });

    // ================================================================
    //               CONSUMIDOR JITTER
    // ================================================================
    let jitter_shut = Arc::clone(&shutdown);
    let jitter_thread = thread::spawn(move || {
        loop {
            if jitter_shut.load(Ordering::Relaxed) {
                break;
            }
            let frame = if let Ok(mut jb) = jitter_for_cons.lock() {
                jb.pop_ready(Instant::now())
            } else {
                None
            };

            if let Some(data) = frame {
                if tx_dec.send(EncodedFrame { data }).is_err() {
                    break;
                }
            } else {
                thread::sleep(Duration::from_millis(1));
            }
        }
    });

    println!("[Client] Iniciando GUI...");

    // UI Bloqueante
    let _ = room_rtc::media::unified_app::run_unified_app(
        room_rtc::media::unified_app::UnifiedAppConfig {
            pending_auth,
            auth_result,
            signaling_tx: Some(signaling_tx),
            signaling_response,
            sctp_tx: Some(sctp_cmd_tx),
            sctp_notif_rx,
            local_frame: local_frame_buffer,
            remote_frame: remote_frame_buffer,
            width,
            height,
            running_call: Arc::clone(&running_call),
            call_generation: Arc::clone(&call_generation),
            shutdown: Arc::clone(&shutdown),
            audio_muted_signal: Arc::clone(&audio_muted),
        },
    );

    // Cleanup
    shutdown.store(true, Ordering::SeqCst);
    let _ = tx_thread.join();
    let _ = rx_thread.join();
    let _ = jitter_thread.join();
    let _ = rtcp_tx_thread.join();
    let _ = audio_tx_thread.join();
    let _ = audio_rx_thread.join();

    Ok(())
}

// ==================== MAIN  ====================

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("--- Iniciando Cliente ---");

    // Preparar estado compartido para autenticación
    let pending_auth: Arc<Mutex<Option<room_rtc::media::unified_app::PendingAuth>>> =
        Arc::new(Mutex::new(None));
    let auth_result: Arc<Mutex<Option<Result<String, String>>>> = Arc::new(Mutex::new(None));

    // Canal para comandos de señalización desde el lobby
    let (signaling_tx, signaling_rx) =
        std::sync::mpsc::channel::<room_rtc::media::unified_app::SignalingCommand>();
    let signaling_response: Arc<Mutex<Option<room_rtc::media::unified_app::SignalingResponse>>> =
        Arc::new(Mutex::new(None));

    // Flag de shutdown global
    let global_shutdown = Arc::new(AtomicBool::new(false));

    let pending_auth_thread = Arc::clone(&pending_auth);
    let auth_result_thread = Arc::clone(&auth_result);
    let signaling_response_thread = Arc::clone(&signaling_response);
    let auth_shutdown = Arc::clone(&global_shutdown);

    let (role_tx, role_rx) = std::sync::mpsc::channel::<(bool, String)>();

    // Generar identidad y fingerprint
    let (identity, local_fingerprint) = generate_local_identity().unwrap();
    let local_fp_for_auth = local_fingerprint.clone();

    // Hilo de autenticación y señalización en background
    let auth_thread = thread::spawn(move || {
        let mut client = match TlsClient::connect("127.0.0.1:8443", "certs/rootCA.pem", "localhost")
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[Auth Thread] Error conectando: {}", e);
                *auth_result_thread.lock().unwrap() =
                    Some(Err(format!("Error de conexión: {}", e)));
                return;
            }
        };
        println!("[Auth Thread] Conexión segura establecida.");

        // Configurar timeout para lectura no bloqueante
        if let Err(e) = client.set_read_timeout(Some(Duration::from_millis(100))) {
            eprintln!("[Auth Thread] Error configurando timeout: {}", e);
        }

        let mut authenticated = false;
        let mut pending_remote_fingerprint: Option<String> = None;

        loop {
            // Verificar si el programa se está cerrando
            if auth_shutdown.load(Ordering::Relaxed) {
                println!("[Auth Thread] Shutdown detectado. Terminando hilo...");
                break;
            }

            // Manejar autenticación
            if !authenticated {
                if let Some(auth_req) = pending_auth_thread.lock().unwrap().take() {
                    println!("[Auth Thread] Procesando autenticación...");

                    let result = if auth_req.is_register {
                        client
                            .send(&Message::Register {
                                username: auth_req.username.clone(),
                                password: auth_req.password.clone(),
                                email: auth_req.email.clone(),
                            })
                            .and_then(|_| client.recv())
                    } else {
                        client
                            .send(&Message::Login {
                                username: auth_req.username.clone(),
                                password: auth_req.password.clone(),
                            })
                            .and_then(|_| client.recv())
                    };

                    match result {
                        Ok(Message::Success { message }) => {
                            println!("[Auth Thread] Éxito: {}", message);
                            *auth_result_thread.lock().unwrap() = Some(Ok(message));
                            authenticated = true;
                        }
                        Ok(Message::Error { message }) => {
                            println!("[Auth Thread] Error: {}", message);
                            *auth_result_thread.lock().unwrap() = Some(Err(message));
                        }
                        Err(e) => {
                            *auth_result_thread.lock().unwrap() =
                                Some(Err(format!("Error: {}", e)));
                        }
                        _ => {
                            *auth_result_thread.lock().unwrap() =
                                Some(Err("Respuesta inesperada".to_string()));
                        }
                    }
                }
            } else {
                // Manejar comandos de señalización después de autenticar
                if let Ok(cmd) = signaling_rx.try_recv() {
                    use room_rtc::media::unified_app::SignalingCommand;

                    match cmd {
                        SignalingCommand::GetUserList => {
                            println!("[Signaling] Solicitando lista de usuarios...");
                            if let Err(e) = client.send(&Message::GetUserList) {
                                eprintln!("[Signaling] Error enviando GetUserList: {}", e);
                                *signaling_response_thread.lock().unwrap() =
                                    Some(room_rtc::media::unified_app::SignalingResponse::Error(
                                        format!("Error enviando solicitud: {}", e),
                                    ));
                            }
                            // No esperamos respuesta aquí, llegará por el loop asíncrono
                        }
                        SignalingCommand::CallRequest { callee } => {
                            println!("[Signaling] Llamando a {}...", callee);
                            if let Err(e) = client.send(&Message::CallRequest {
                                callee,
                                fingerprint: local_fp_for_auth.clone(),
                            }) {
                                eprintln!("[Signaling] Error enviando CallRequest: {}", e);
                                continue;
                            }

                            // Ahora esperamos la respuesta inmediata del servidor (Success o Error)
                            match client.recv() {
                                Ok(Message::Success { message }) => {
                                    println!("[Signaling] {}", message);
                                    // La respuesta CallAccept/CallReject llegará después de forma asíncrona
                                }
                                Ok(Message::Error { message }) => {
                                    eprintln!("[Signaling] Error: {}", message);
                                    *signaling_response_thread.lock().unwrap() = Some(
                                        room_rtc::media::unified_app::SignalingResponse::Error(
                                            message,
                                        ),
                                    );
                                }
                                Err(e) => {
                                    eprintln!("[Signaling] Error recibiendo respuesta: {}", e);
                                }
                                _ => {
                                    eprintln!("[Signaling] Respuesta inesperada");
                                }
                            }
                        }
                        SignalingCommand::AcceptCall { caller } => {
                            println!("[Signaling] Aceptando llamada de {}...", caller);
                            if let Err(e) = client.send(&Message::CallAccept {
                                caller,
                                fingerprint: local_fp_for_auth.clone(),
                            }) {
                                eprintln!("[Signaling] Error enviando CallAccept: {}", e);
                                continue;
                            }

                            match client.recv() {
                                Ok(Message::Success { message }) => {
                                    println!("[Signaling] {}", message);
                                    if let Some(fp) = pending_remote_fingerprint.clone() {
                                        let _ = role_tx.send((false, fp));
                                    }
                                }
                                Ok(Message::Error { message }) => {
                                    eprintln!("[Signaling] Error: {}", message);
                                }
                                Err(e) => {
                                    eprintln!("[Signaling] Error recibiendo respuesta: {}", e);
                                }
                                _ => {
                                    eprintln!("[Signaling] Respuesta inesperada");
                                }
                            }
                        }
                        SignalingCommand::RejectCall { caller } => {
                            println!("[Signaling] Rechazando llamada de {}...", caller);
                            if let Err(e) = client.send(&Message::CallReject { caller }) {
                                eprintln!("[Signaling] Error enviando CallReject: {}", e);
                                continue;
                            }
                            pending_remote_fingerprint = None;

                            match client.recv() {
                                Ok(Message::Success { message }) => {
                                    println!("[Signaling] {}", message);
                                }
                                Ok(Message::Error { message }) => {
                                    eprintln!("[Signaling] Error: {}", message);
                                }
                                Err(e) => {
                                    eprintln!("[Signaling] Error recibiendo respuesta: {}", e);
                                }
                                _ => {
                                    eprintln!("[Signaling] Respuesta inesperada");
                                }
                            }
                        }
                        SignalingCommand::EndCall => {
                            println!("[Signaling] Finalizando llamada...");
                            if let Err(e) = client.send(&Message::CallEnd) {
                                eprintln!("[Signaling] Error enviando CallEnd: {}", e);
                                continue;
                            }

                            match client.recv() {
                                Ok(Message::Success { message }) => {
                                    println!("[Signaling] {}", message);
                                }
                                Ok(Message::Error { message }) => {
                                    eprintln!("[Signaling] Error: {}", message);
                                }
                                Err(e) => {
                                    eprintln!("[Signaling] Error recibiendo respuesta: {}", e);
                                }
                                _ => {
                                    eprintln!("[Signaling] Respuesta inesperada");
                                }
                            }
                        }
                    }
                }

                // Verificar si hay mensajes entrantes del servidor (non-blocking)
                match client.try_recv() {
                    Ok(Message::UserListResponse { users }) => {
                        println!("[Signaling] Recibida lista de {} usuarios", users.len());
                        // Convertir UserStatus a unified_app::UserStatus
                        let converted_users: Vec<(
                            String,
                            room_rtc::media::unified_app::UserStatus,
                        )> = users
                            .into_iter()
                            .map(|(name, status)| {
                                let converted_status = match status {
                                    UserStatus::Available => {
                                        room_rtc::media::unified_app::UserStatus::Available
                                    }
                                    UserStatus::Busy => {
                                        room_rtc::media::unified_app::UserStatus::Busy
                                    }
                                    UserStatus::Disconnected => {
                                        room_rtc::media::unified_app::UserStatus::Disconnected
                                    }
                                };
                                (name, converted_status)
                            })
                            .collect();

                        *signaling_response_thread.lock().unwrap() =
                            Some(room_rtc::media::unified_app::SignalingResponse::UserList(
                                converted_users,
                            ));
                    }
                    Ok(Message::CallIncoming {
                        caller,
                        fingerprint,
                    }) => {
                        println!("[Signaling] Llamada entrante de {}", caller);
                        pending_remote_fingerprint = Some(fingerprint);
                        *signaling_response_thread.lock().unwrap() = Some(
                            room_rtc::media::unified_app::SignalingResponse::IncomingCall {
                                caller,
                            },
                        );
                    }
                    Ok(Message::CallAccept {
                        caller,
                        fingerprint,
                    }) => {
                        println!("[Signaling] {} aceptó la llamada", caller);
                        *signaling_response_thread.lock().unwrap() =
                            Some(room_rtc::media::unified_app::SignalingResponse::CallAccepted);
                        // Soy Caller (true), paso el fingerprint del callee
                        let _ = role_tx.send((true, fingerprint));
                    }
                    Ok(Message::CallReject { caller }) => {
                        println!("[Signaling] {} rechazó la llamada", caller);
                        *signaling_response_thread.lock().unwrap() =
                            Some(room_rtc::media::unified_app::SignalingResponse::CallRejected);
                    }
                    Ok(Message::CallEnd) => {
                        println!("[Signaling] El otro usuario finalizó la llamada");
                        pending_remote_fingerprint = None;
                        *signaling_response_thread.lock().unwrap() =
                            Some(room_rtc::media::unified_app::SignalingResponse::CallEnded);
                    }
                    Ok(msg) => {
                        println!("[Signaling] Mensaje inesperado recibido: {:?}", msg);
                    }
                    Err(e) => {
                        // Si es timeout o WouldBlock, es normal (no hay mensajes)
                        if e.kind() != std::io::ErrorKind::WouldBlock
                            && e.kind() != std::io::ErrorKind::TimedOut
                        {
                            eprintln!("[Signaling] Error leyendo mensaje: {}", e);
                        }
                    }
                }
            }

            thread::sleep(Duration::from_millis(50));
        }
    });

    // Leer configuración
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Uso: {} <archivo_config>", args[0]);
        return Ok(());
    }
    let cfg_path = &args[1];

    // Parse SDP-like .config
    let cfg_text = std::fs::read_to_string(cfg_path)?;
    let mut sdp = SessionDescription::new();
    sdp = sdp.parse(&cfg_text)?;

    let ip_local = sdp.origin.unicast_address.clone();

    // Tomamos primera media "video"
    let media = &sdp.media[0];
    let puerto_local = media.port;

    // IP remota (si hay c= a nivel session; si no, usamos la local)
    let ip_remota = media
        .connection_info
        .as_ref()
        .unwrap()
        .connection_address
        .clone();

    // Puerto remoto: convención +/-1
    // uso el session_id para romper la simetria:
    // si es impar -> puerto_local , remoto +1
    // si es par -> puerto local, remoto -1

    let puerto_remoto = if sdp.origin.session_id % 2 == 1 {
        puerto_local + 1
    } else {
        puerto_local - 1
    };

    // Atributos: source / framerate
    let mut source_attr: Option<String> = None;
    let mut fps: u32 = 30;

    let mut rtcp_enabled = false;
    let mut rtcp_interval_ms: u64 = 1000;
    let mut rtcp_port_offset: u16 = 1;

    for a in &media.attributes {
        if a.name == "source" {
            source_attr = a.value.clone();
        } else if a.name == "framerate"
            && let Some(v) = &a.value
            && let Ok(n) = v.trim().parse::<u32>()
        {
            fps = n.max(1);
        } else if a.name == "rtcp_enabled"
            && let Some(v) = &a.value
        {
            rtcp_enabled = v.trim().eq_ignore_ascii_case("true");
        } else if a.name == "rtcp_interval_ms"
            && let Some(v) = &a.value
            && let Ok(n) = v.trim().parse::<u64>()
        {
            rtcp_interval_ms = n.max(1);
        } else if a.name == "rtcp_port_offset"
            && let Some(v) = &a.value
            && let Ok(n) = v.trim().parse::<u16>()
        {
            rtcp_port_offset = n.max(1);
        }
    }

    let source_pipeline = if let Some(s) = source_attr {
        if s.starts_with("/dev/video") {
            format!("v4l2src device={} ! videoconvert", s)
        } else {
            s
        }
    } else {
        "v4l2src device=/dev/video0 ! videoconvert".to_string()
    };

    let mut cliente = ClienteICELite::new(&ip_local, puerto_local, &ip_remota, puerto_remoto)?;

    println!(
        "[Session {}] Local={}  Remoto={}",
        sdp.session_name, cliente.direccion_local, cliente.direccion_remota
    );
    println!(
        "[Session {}] Fuente='{}'  FPS={}",
        sdp.session_name, source_pipeline, fps
    );

    // --- Asignar valores RTCP leídos del SDP al cliente ---
    cliente.rtcp_enabled = rtcp_enabled;
    cliente.rtcp_interval_ms = rtcp_interval_ms;
    cliente.rtcp_port_offset = rtcp_port_offset;

    println!(
        "[RTCP] enabled={} interval={}ms offset={}",
        cliente.rtcp_enabled, cliente.rtcp_interval_ms, cliente.rtcp_port_offset
    );

    // Resolución de video
    let width = VIDEO_WIDTH;
    let height = VIDEO_HEIGHT;

    // Iniciar cliente de video con autenticación integrada
    run_media_client(
        &cliente,
        MediaClientConfig {
            identity,
            fps,
            source_pipeline: source_pipeline.to_string(),
            width,
            height,
            pending_auth,
            auth_result,
            signaling_tx,
            signaling_response,
            role_rx,
        },
    )?;

    // Después de que la ventana se cierre, esperar el hilo de auth
    println!("[Main] Esperando hilo de autenticación...");
    global_shutdown.store(true, Ordering::SeqCst);
    let _ = auth_thread.join();
    println!("[Main] Todos los hilos terminados. Saliendo...");

    Ok(())
}

// ======================== TESTS ===============================

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;
    use std::str::FromStr;
    use std::time::Duration;

    fn compute_source_pipeline(source_attr: Option<String>) -> String {
        if let Some(s) = source_attr {
            if s.starts_with("/dev/video") {
                format!("v4l2src device={} ! videoconvert", s)
            } else {
                s
            }
        } else {
            "v4l2src device=/dev/video0 ! videoconvert".to_string()
        }
    }

    fn extract_video_settings_from_media(
        media: &room_rtc::sdp::MediaDescription,
    ) -> (Option<String>, u32, bool, u64, u16) {
        let mut source_attr: Option<String> = None;
        let mut fps: u32 = 30;

        let mut rtcp_enabled = false;
        let mut rtcp_interval_ms: u64 = 1000;
        let mut rtcp_port_offset: u16 = 1;

        for a in &media.attributes {
            if a.name == "source" {
                source_attr = a.value.clone();
            } else if a.name == "framerate"
                && let Some(v) = &a.value
                && let Ok(n) = v.trim().parse::<u32>()
            {
                fps = n.max(1);
            } else if a.name == "rtcp_enabled"
                && let Some(v) = &a.value
            {
                rtcp_enabled = v.trim().eq_ignore_ascii_case("true");
            } else if a.name == "rtcp_interval_ms"
                && let Some(v) = &a.value
                && let Ok(n) = v.trim().parse::<u64>()
            {
                rtcp_interval_ms = n.max(1);
            } else if a.name == "rtcp_port_offset"
                && let Some(v) = &a.value
                && let Ok(n) = v.trim().parse::<u16>()
            {
                rtcp_port_offset = n.max(1);
            }
        }

        (
            source_attr,
            fps,
            rtcp_enabled,
            rtcp_interval_ms,
            rtcp_port_offset,
        )
    }

    fn compute_remote_port(session_id: u64, local_port: u16) -> u16 {
        if session_id % 2 == 1 {
            local_port + 1
        } else {
            local_port - 1
        }
    }

    #[test]
    fn generate_local_identity_fingerprint_is_upper_hex_64() {
        let (_identity, fp) = generate_local_identity().expect("generate_local_identity failed");

        assert_eq!(fp.len(), 64, "SHA-256 fingerprint debe tener 64 hex chars");
        assert!(
            fp.chars().all(|c| matches!(c, '0'..='9' | 'A'..='F')),
            "Fingerprint debe ser HEX uppercase sin separadores"
        );
    }

    #[test]
    fn generate_local_identity_fingerprint_matches_identity_certificate() {
        let (identity, fp) = generate_local_identity().expect("generate_local_identity failed");

        #[allow(deprecated)]
        {
            let digest = identity
                .0
                .cert
                .digest(MessageDigest::sha256())
                .expect("cert.digest sha256 failed");

            let computed = digest
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<String>();
            assert_eq!(
                computed, fp,
                "Fingerprint devuelto debe coincidir con el cert"
            );
        }
    }

    #[test]
    fn sdp_extracts_source_pipeline_fps_and_rtcp_settings() {
        let sdp_text = r#"v=0
        o=Node 3 1 IN IP4 192.168.1.10
        s=TestSession
        t=0 0
        m=video 5000 RTP/AVP 96
        c=IN IP4 192.168.1.20
        a=source:/dev/video2
        a=framerate:15
        a=rtcp_enabled:true
        a=rtcp_interval_ms:250
        a=rtcp_port_offset:3
        "#;

        let sdp = SessionDescription::new()
            .parse(sdp_text)
            .expect("SDP parse failed");
        assert_eq!(sdp.media.len(), 1);
        let media = &sdp.media[0];

        let (source_attr, fps, rtcp_enabled, rtcp_interval_ms, rtcp_port_offset) =
            extract_video_settings_from_media(media);

        let source_pipeline = compute_source_pipeline(source_attr);

        assert_eq!(source_pipeline, "v4l2src device=/dev/video2 ! videoconvert");
        assert_eq!(fps, 15);
        assert!(rtcp_enabled);
        assert_eq!(rtcp_interval_ms, 250);
        assert_eq!(rtcp_port_offset, 3);

        let local_port = media.port;
        let remote_port = compute_remote_port(sdp.origin.session_id, local_port);
        assert_eq!(local_port, 5000);
        assert_eq!(sdp.origin.session_id, 3);
        assert_eq!(remote_port, 5001);
    }

    #[test]
    fn sdp_source_pipeline_keeps_custom_pipeline_if_not_dev_video() {
        let sdp_text = r#"v=0
            o=Node 4 1 IN IP4 10.0.0.1
            s=TestSession
            t=0 0
            m=video 6000 RTP/AVP 96
            c=IN IP4 10.0.0.2
            a=source:videotestsrc ! videoconvert
            a=framerate:30
            "#;

        let sdp = SessionDescription::new()
            .parse(sdp_text)
            .expect("SDP parse failed");
        let media = &sdp.media[0];

        let (source_attr, fps, rtcp_enabled, rtcp_interval_ms, rtcp_port_offset) =
            extract_video_settings_from_media(media);

        let source_pipeline = compute_source_pipeline(source_attr);

        assert_eq!(source_pipeline, "videotestsrc ! videoconvert");
        assert_eq!(fps, 30);

        assert!(!rtcp_enabled);
        assert_eq!(rtcp_interval_ms, 1000);
        assert_eq!(rtcp_port_offset, 1);

        let remote_port = compute_remote_port(sdp.origin.session_id, media.port);
        assert_eq!(sdp.origin.session_id, 4);
        assert_eq!(remote_port, 5999);
    }

    #[test]
    fn sdp_defaults_when_missing_attributes_and_clamps_minimums() {
        let sdp_text = r#"v=0
        o=Node 5 1 IN IP4 10.10.10.1
        s=TestSession
        t=0 0
        m=video 7000 RTP/AVP 96
        c=IN IP4 10.10.10.2
        a=framerate:0
        a=rtcp_enabled:false
        a=rtcp_interval_ms:0
        a=rtcp_port_offset:0
        "#;

        let sdp = SessionDescription::new()
            .parse(sdp_text)
            .expect("SDP parse failed");
        let media = &sdp.media[0];

        let (source_attr, fps, rtcp_enabled, rtcp_interval_ms, rtcp_port_offset) =
            extract_video_settings_from_media(media);

        let source_pipeline = compute_source_pipeline(source_attr);
        assert_eq!(source_pipeline, "v4l2src device=/dev/video0 ! videoconvert");

        assert_eq!(fps, 1);
        assert!(!rtcp_enabled);
        assert_eq!(rtcp_interval_ms, 1);
        assert_eq!(rtcp_port_offset, 1);

        let remote_port = compute_remote_port(sdp.origin.session_id, media.port);
        assert_eq!(remote_port, 7001);
    }

    #[test]
    fn audio_ports_are_video_port_plus_two_saturating() {
        let video_local = SocketAddr::new(IpAddr::from_str("192.168.0.10").unwrap(), 5000);
        let video_remote = SocketAddr::new(IpAddr::from_str("192.168.0.20").unwrap(), 5001);

        let audio_port_offset: u16 = 2;

        let audio_local = SocketAddr::new(
            video_local.ip(),
            video_local.port().saturating_add(audio_port_offset),
        );

        let audio_remote = SocketAddr::new(
            video_remote.ip(),
            video_remote.port().saturating_add(audio_port_offset),
        );

        assert_eq!(audio_local.port(), 5002);
        assert_eq!(audio_remote.port(), 5003);
    }

    #[test]
    fn jitter_buffer_config_values_used_in_client_are_reasonable() {
        let cfg = JitterBufferConfig {
            target_delay: Duration::from_millis(80),
            max_delay: Duration::from_millis(200),
        };

        assert!(cfg.target_delay < cfg.max_delay);
        assert!(cfg.target_delay >= Duration::from_millis(1));
    }
}
