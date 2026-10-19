//! Definiciones de mensajes para el protocolo de señalización.
//!
//! Este módulo contiene las estructuras de mensajes usadas para la comunicación
//! de señalización entre el cliente y el servidor, incluyendo autenticación,
//! gestión de llamadas, e intercambio de información de sesión.

/// Estado de un usuario en el sistema.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum UserStatus {
    /// Usuario desconectado del sistema.
    Disconnected,
    /// Usuario disponible para recibir llamadas.
    Available,
    /// Usuario ocupado en una llamada.
    Busy,
}

/// Mensajes del protocolo de señalización.
///
/// Este enum define todos los tipos de mensajes que pueden ser intercambiados
/// entre el cliente y el servidor para gestionar autenticación, estado de usuarios,
/// y establecimiento de llamadas WebRTC.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Message {
    /// Solicitud de registro de nuevo usuario.
    Register {
        /// Nombre de usuario único.
        username: String,
        /// Contraseña del usuario.
        password: String,
        /// Email opcional del usuario.
        email: Option<String>,
    },
    /// Solicitud de inicio de sesión.
    Login {
        /// Nombre de usuario.
        username: String,
        /// Contraseña del usuario.
        password: String,
    },
    /// Solicitud de cierre de sesión.
    Logout,
    /// Solicitud de lista de usuarios conectados.
    GetUserList,
    /// Respuesta con lista de usuarios y sus estados.
    UserListResponse {
        /// Lista de tuplas (nombre_usuario, estado).
        users: Vec<(String, UserStatus)>,
    },
    /// Actualización de estado de un usuario.
    StatusUpdate {
        /// Nombre del usuario cuyo estado cambió.
        username: String,
        /// Nuevo estado del usuario.
        status: UserStatus,
    },
    /// Solicitud de llamada a otro usuario.
    CallRequest {
        /// Usuario al que se quiere llamar.
        callee: String,
        /// Fingerprint del certificado DTLS local.
        fingerprint: String,
    },
    /// Notificación de llamada entrante.
    CallIncoming {
        /// Usuario que está llamando.
        caller: String,
        /// Fingerprint del certificado DTLS del llamador.
        fingerprint: String,
    },
    /// Aceptación de una llamada.
    CallAccept {
        /// Usuario que inició la llamada.
        caller: String,
        /// Fingerprint del certificado DTLS local.
        fingerprint: String,
    },
    /// Rechazo de una llamada.
    CallReject {
        /// Usuario que inició la llamada.
        caller: String,
    },
    /// Finalización de llamada.
    CallEnd,
    /// Oferta SDP para establecer sesión WebRTC.
    Offer {
        /// Descripción de sesión en formato SDP.
        sdp: String,
        /// Usuario destinatario de la oferta.
        target: String,
    },
    /// Respuesta SDP para establecer sesión WebRTC.
    Answer {
        /// Descripción de sesión en formato SDP.
        sdp: String,
        /// Usuario destinatario de la respuesta.
        target: String,
    },
    /// Candidato ICE para establecer conectividad.
    IceCandidate {
        /// Información del candidato ICE.
        candidate: String,
        /// Usuario destinatario del candidato.
        target: String,
    },
    /// Respuesta exitosa del servidor.
    Success {
        /// Mensaje descriptivo del éxito.
        message: String,
    },
    /// Respuesta de error del servidor.
    Error {
        /// Mensaje descriptivo del error.
        message: String,
    },
}
