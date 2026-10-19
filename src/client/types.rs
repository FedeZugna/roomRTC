//! Definiciones de tipos comunes para el cliente

use crate::media::codec::vp8::EncodedFrame;
use sctp_proto::{Association, AssociationHandle, Endpoint};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use udp_dtls::{DtlsStream, UdpChannel};

/// Stream DTLS compartido envuelto en Arc<Mutex<>> para acceso seguro entre hilos.
/// Contiene el stream DTLS opcional usado para comunicación segura.
pub type SharedDtlsStream = Arc<Mutex<Option<DtlsStream<UdpChannel>>>>;

/// Endpoint SCTP compartido envuelto en Arc<Mutex<>> para acceso seguro entre hilos.
/// El endpoint gestiona las asociaciones SCTP.
pub type SharedEndpoint = Arc<Mutex<Option<Endpoint>>>;

/// Handle de asociación SCTP y asociación compartidos envueltos en Arc<Mutex<>>.
/// Usado para gestionar una conexión SCTP activa.
pub type SharedSctpAssociation = Arc<Mutex<Option<(AssociationHandle, Association)>>>;

/// Buffer de frame RGBA compartido para frames de video.
/// Contiene datos de píxeles RGBA opcionales envueltos en Arc<Mutex<>> para acceso seguro entre hilos.
pub type SharedRgba = Arc<Mutex<Option<Vec<u8>>>>;

/// Tipo de resultado del cliente para operaciones que pueden fallar con cualquier error.
/// Este es un alias de conveniencia para Result<T, Box<dyn std::error::Error>>.
pub type ClientResult<T> = Result<T, Box<dyn std::error::Error>>;

/// Canales del cliente para codificación/decodificación de video.
/// Tupla que contiene:
/// - Receiver para frames codificados (desde el encoder)
/// - Sender para frames codificados (hacia el decoder)
/// - Buffer de frame RGBA local (cámara local)
/// - Buffer de frame RGBA remoto (peer remoto)
pub type ClientChannels = (
    Receiver<EncodedFrame>,
    Sender<EncodedFrame>,
    SharedRgba,
    SharedRgba,
);

/// Número máximo de usuarios/streams soportados por SCTP.
pub const SCTP_MAX_USERS: u16 = 16;

/// Tamaño máximo para envio de archivos por SCTP
pub const SCTP_MAX_FILE_SIZE: u32 = 4 * 1024 * 1024; // 4 MB
