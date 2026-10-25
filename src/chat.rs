//! Chat de texto entre los dos participantes de una videollamada.
//!
//! Solo reglas del mensaje e historial: no conoce la red ni la UI.

use std::collections::VecDeque;

/// Tamaño máximo de un mensaje en producción, en bytes UTF-8.
pub const CHAT_MAX_BYTES: usize = 4096;

/// Cantidad máxima de mensajes en el historial en producción.
pub const CHAT_HISTORY_MAX: usize = 200;

/// Stream SCTP dedicado al chat (0 y 1 son de transferencia de archivos).
pub const CHAT_STREAM_ID: u16 = 2;

/// Límites del chat. Los tests inyectan valores chicos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatLimits {
    pub max_bytes: usize,
    pub history_max: usize,
}

impl ChatLimits {
    pub const PRODUCTION: ChatLimits = ChatLimits {
        max_bytes: CHAT_MAX_BYTES,
        history_max: CHAT_HISTORY_MAX,
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatError {
    /// Texto vacío o de solo espacios (o payload vacío al recibir).
    Empty,
    /// Supera el límite de bytes.
    TooLong { len: usize, max: usize },
    /// Payload recibido que no es UTF-8 válido.
    InvalidUtf8,
    /// No hay una llamada activa.
    NotInCall,
    /// Hay llamada pero el canal de envío no está listo.
    ChannelNotReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatAuthor {
    /// Mensaje propio, con su id local.
    Own { id: u64 },
    /// Mensaje del otro participante.
    Peer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatEntry {
    pub author: ChatAuthor,
    pub text: String,
    /// El transporte no pudo escribir el mensaje (solo mensajes propios).
    pub not_sent: bool,
}

/// Mensaje listo para entregar al transporte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingChat {
    pub id: u64,
    pub text: String,
}

/// Estado del chat de una llamada: historial, borrador e ids locales.
#[derive(Debug, Default)]
pub struct ChatState {
    pub history: VecDeque<ChatEntry>,
    pub draft: String,
    next_id: u64,
}

impl ChatState {
    /// Agrega un mensaje propio y devuelve su id local.
    pub fn push_own(&mut self, text: &str, history_max: usize) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.push(ChatAuthor::Own { id }, text, history_max);
        id
    }

    /// Agrega un mensaje recibido del participante.
    pub fn push_peer(&mut self, text: &str, history_max: usize) {
        self.push(ChatAuthor::Peer, text, history_max);
    }

    /// Marca como "no enviado" el mensaje propio con ese id. Devuelve si lo encontró.
    pub fn mark_failed(&mut self, id: u64) -> bool {
        match self
            .history
            .iter_mut()
            .find(|e| e.author == ChatAuthor::Own { id })
        {
            Some(entry) => {
                entry.not_sent = true;
                true
            }
            None => false,
        }
    }

    /// Vacía historial y borrador al terminar la llamada.
    pub fn reset(&mut self) {
        *self = ChatState::default();
    }

    // Si se supera el tope, se descarta el mensaje más viejo.
    fn push(&mut self, author: ChatAuthor, text: &str, history_max: usize) {
        self.history.push_back(ChatEntry {
            author,
            text: text.to_string(),
            not_sent: false,
        });
        while self.history.len() > history_max {
            self.history.pop_front();
        }
    }
}

/// Valida un texto saliente. No lo modifica: el texto válido se envía tal cual.
pub fn validate_outgoing(text: &str, max_bytes: usize) -> Result<(), ChatError> {
    if text.trim().is_empty() {
        return Err(ChatError::Empty);
    }
    check_len(text.len(), max_bytes)
}

pub fn encode(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

/// Decodifica un payload recibido. El receptor no confía en el emisor.
pub fn decode(bytes: &[u8], max_bytes: usize) -> Result<String, ChatError> {
    if bytes.is_empty() {
        return Err(ChatError::Empty);
    }
    check_len(bytes.len(), max_bytes)?;
    String::from_utf8(bytes.to_vec()).map_err(|_| ChatError::InvalidUtf8)
}

// El límite se mide en bytes, no en caracteres.
fn check_len(len: usize, max: usize) -> Result<(), ChatError> {
    if len > max {
        return Err(ChatError::TooLong { len, max });
    }
    Ok(())
}

/// Prepara el envío de un texto: valida, lo agrega al historial y devuelve el
/// mensaje a transmitir. Si falla, el estado no cambia.
pub fn prepare_send(
    state: &mut ChatState,
    text: &str,
    in_call: bool,
    channel_ready: bool,
    limits: ChatLimits,
) -> Result<OutgoingChat, ChatError> {
    if !in_call {
        return Err(ChatError::NotInCall);
    }
    if !channel_ready {
        return Err(ChatError::ChannelNotReady);
    }
    validate_outgoing(text, limits.max_bytes)?;
    let id = state.push_own(text, limits.history_max);
    Ok(OutgoingChat {
        id,
        text: text.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_LIMITS: ChatLimits = ChatLimits {
        max_bytes: 16,
        history_max: 3,
    };

    fn own(id: u64, text: &str) -> ChatEntry {
        ChatEntry {
            author: ChatAuthor::Own { id },
            text: text.to_string(),
            not_sent: false,
        }
    }

    fn peer(text: &str) -> ChatEntry {
        ChatEntry {
            author: ChatAuthor::Peer,
            text: text.to_string(),
            not_sent: false,
        }
    }

    fn send(state: &mut ChatState, text: &str) -> Result<OutgoingChat, ChatError> {
        prepare_send(state, text, true, true, TEST_LIMITS)
    }

    #[test]
    fn chat_1_1_envia_mensaje_valido() {
        let mut state = ChatState::default();

        let out = send(&mut state, "Hola");

        assert_eq!(
            out,
            Ok(OutgoingChat {
                id: 1,
                text: "Hola".to_string()
            })
        );
        assert_eq!(state.history, vec![own(1, "Hola")]);
    }

    #[test]
    fn chat_1_2_rechaza_mensaje_vacio_o_solo_espacios() {
        let mut state = ChatState::default();

        assert_eq!(send(&mut state, ""), Err(ChatError::Empty));
        assert_eq!(send(&mut state, "   "), Err(ChatError::Empty));
        assert!(state.history.is_empty());
    }

    #[test]
    fn chat_1_3_rechaza_mensaje_sobre_el_limite() {
        let dieciseis = "a".repeat(16);
        let diecisiete = "a".repeat(17);

        assert_eq!(validate_outgoing(&dieciseis, 16), Ok(()));
        assert_eq!(
            validate_outgoing(&diecisiete, 16),
            Err(ChatError::TooLong { len: 17, max: 16 })
        );
    }

    #[test]
    fn chat_1_4_conserva_espacios_del_texto() {
        let mut state = ChatState::default();

        let out = send(&mut state, "  hola  ");

        assert_eq!(out.map(|m| m.text), Ok("  hola  ".to_string()));
    }

    #[test]
    fn chat_1_5_codifica_y_decodifica_utf8() {
        let original = "¿Qué tal?";
        assert_eq!(original.len(), 11);

        let bytes = encode(original);

        assert_eq!(bytes.len(), 11);
        assert_eq!(decode(&bytes, 16), Ok(original.to_string()));
    }

    #[test]
    fn chat_1_6_descarta_payload_invalido_sin_panic() {
        assert_eq!(decode(&[0xFF, 0xFE], 16), Err(ChatError::InvalidUtf8));
        assert_eq!(decode(&[], 16), Err(ChatError::Empty));
        assert_eq!(
            decode(&[b'a'; 17], 16),
            Err(ChatError::TooLong { len: 17, max: 16 })
        );
    }

    #[test]
    fn chat_1_7_rechaza_envio_sin_llamada_activa() {
        let mut state = ChatState::default();

        // Sesión inactiva o terminada
        assert_eq!(
            prepare_send(&mut state, "Hola", false, true, TEST_LIMITS),
            Err(ChatError::NotInCall)
        );
        // Llamada activa pero canal de envío no listo
        assert_eq!(
            prepare_send(&mut state, "Hola", true, false, TEST_LIMITS),
            Err(ChatError::ChannelNotReady)
        );
        assert!(state.history.is_empty());
    }

    #[test]
    fn chat_1_8_marca_no_enviado_por_id_local() {
        let mut state = ChatState::default();
        let primero = send(&mut state, "ok").map(|m| m.id);
        let segundo = send(&mut state, "ok").map(|m| m.id);
        assert_eq!((primero, segundo), (Ok(1), Ok(2)));

        assert!(state.mark_failed(2));

        let mut fallido = own(2, "ok");
        fallido.not_sent = true;
        assert_eq!(state.history, vec![own(1, "ok"), fallido]);
    }

    #[test]
    fn chat_1_9_recibe_en_orden_sin_duplicados() {
        let mut state = ChatState::default();

        for text in ["uno", "dos", "tres"] {
            state.push_peer(text, TEST_LIMITS.history_max);
        }

        assert_eq!(state.history, vec![peer("uno"), peer("dos"), peer("tres")]);
    }

    #[test]
    fn chat_1_10_descarta_el_mas_viejo_al_superar_el_tope() {
        let mut state = ChatState::default();
        for text in ["a", "b", "c"] {
            state.push_peer(text, TEST_LIMITS.history_max);
        }

        state.push_peer("d", TEST_LIMITS.history_max);

        assert_eq!(state.history, vec![peer("b"), peer("c"), peer("d")]);
    }

    #[test]
    fn chat_1_11_nueva_sesion_arranca_vacia() {
        let mut state = ChatState::default();
        let _ = send(&mut state, "hola");
        state.push_peer("chau", TEST_LIMITS.history_max);
        state.draft = "hola".to_string();

        state.reset();

        assert!(state.history.is_empty());
        assert!(state.draft.is_empty());
    }

    #[test]
    fn chat_1_12_valores_de_produccion() {
        assert_eq!(CHAT_MAX_BYTES, 4096);
        assert_eq!(CHAT_HISTORY_MAX, 200);
        assert_eq!(
            ChatLimits::PRODUCTION,
            ChatLimits {
                max_bytes: 4096,
                history_max: 200
            }
        );
    }
}
