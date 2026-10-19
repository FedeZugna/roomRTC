//! Structs de configuración para el cliente.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use udp_dtls::Identity;

/// Agrupa todos los parámetros necesarios para ejecutar el pipeline de
/// envío/recepción de media.
pub struct MediaClientConfig {
    /// Identidad DTLS (certificado y clave privada) para conexión segura.
    pub identity: Identity,
    /// Frames por segundo para codificación/decodificación de video.
    pub fps: u32,
    /// Pipeline de GStreamer para la fuente de video (ej: "v4l2src device=/dev/video0 ! videoconvert").
    pub source_pipeline: String,
    /// Ancho del video en píxeles.
    pub width: u32,
    /// Alto del video en píxeles.
    pub height: u32,
    /// Estado de autenticación pendiente compartido con la UI.
    pub pending_auth: Arc<Mutex<Option<crate::media::unified_app::PendingAuth>>>,
    /// Resultado de autenticación compartido con la UI.
    pub auth_result: Arc<Mutex<Option<Result<String, String>>>>,
    /// Canal de envío para comandos de señalización desde el lobby.
    pub signaling_tx: Sender<crate::media::unified_app::SignalingCommand>,
    /// Respuesta de señalización compartida con la UI.
    pub signaling_response: Arc<Mutex<Option<crate::media::unified_app::SignalingResponse>>>,
    /// Canal de recepción para determinar el rol (caller/callee) y fingerprint remoto.
    pub role_rx: Receiver<(bool, String)>,
}
