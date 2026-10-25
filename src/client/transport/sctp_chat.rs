//! Enrutamiento de datos SCTP recibidos y envío de chat, como funciones puras
//! que los hilos de `cliente.rs` llaman y que se testean sin red.
//!
//! Streams: 0 control de archivos, 1 contenido de archivos, `CHAT_STREAM_ID`
//! chat (ver `docs/chat/exploracion.md`).

use crate::chat::{self, CHAT_STREAM_ID};
use crate::media::unified_app::{SctpCommand, SctpNotification};
use bytes::Bytes;
use sctp_proto::{Association, PayloadProtocolIdentifier};

/// Mensaje de control de archivos del stream 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileControl {
    Offer { filename: String, size: usize },
    Accept { filename: String },
    Reject { filename: String },
}

/// Qué hacer con un payload recibido.
#[derive(Debug, PartialEq, Eq)]
pub enum Incoming<'a> {
    Chat(SctpNotification),
    FileControl(FileControl),
    FileContent(&'a [u8]),
    Ignored,
}

/// Parsea un mensaje de control de archivos: `OFFER <nombre> <tamaño>`,
/// `ACCEPT <nombre>` o `REJECT <nombre>`.
pub fn parse_file_control(msg: &str) -> Option<FileControl> {
    if let Some(rest) = msg.strip_prefix("OFFER ") {
        // rsplit_once separa el tamaño del final, permitiendo espacios en el nombre
        let (filename, size) = rest.rsplit_once(' ')?;
        let size = size.parse::<usize>().ok()?;
        return Some(FileControl::Offer {
            filename: filename.to_string(),
            size,
        });
    }
    if let Some(filename) = msg.strip_prefix("ACCEPT ") {
        return Some(FileControl::Accept {
            filename: filename.to_string(),
        });
    }
    msg.strip_prefix("REJECT ")
        .map(|filename| FileControl::Reject {
            filename: filename.to_string(),
        })
}

/// Clasifica un payload según su stream. Un chat recibido lleva la generación
/// de la llamada actual.
pub fn route_incoming(
    stream_id: u16,
    data: &[u8],
    generation: u64,
    max_bytes: usize,
) -> Incoming<'_> {
    match stream_id {
        0 => match parse_file_control(&String::from_utf8_lossy(data)) {
            Some(control) => Incoming::FileControl(control),
            None => Incoming::Ignored,
        },
        1 => Incoming::FileContent(data),
        CHAT_STREAM_ID => match chat::decode(data, max_bytes) {
            Ok(text) => Incoming::Chat(SctpNotification::ChatReceived { generation, text }),
            // El receptor no confía en el emisor: lo inválido se descarta
            Err(_) => Incoming::Ignored,
        },
        _ => Incoming::Ignored,
    }
}

/// Destino de escritura del chat. La asociación SCTP lo implementa; los tests
/// usan uno que falla.
pub trait ChatWriter {
    fn write_chat(&mut self, payload: &[u8]) -> Result<(), String>;
}

impl ChatWriter for Association {
    // El stream de chat se abre la primera vez, como el stream 0. Es ordenado
    // y confiable (default de sctp-proto).
    fn write_chat(&mut self, payload: &[u8]) -> Result<(), String> {
        let mut stream = match self.stream(CHAT_STREAM_ID) {
            Ok(stream) => stream,
            Err(_) => self
                .open_stream(CHAT_STREAM_ID, PayloadProtocolIdentifier::String)
                .map_err(|e| format!("no se pudo abrir el stream de chat: {e:?}"))?,
        };
        stream
            .write_sctp(
                &Bytes::copy_from_slice(payload),
                PayloadProtocolIdentifier::String,
            )
            .map(|_| ())
            .map_err(|e| format!("no se pudo escribir el chat: {e:?}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatSendOutcome {
    Sent,
    /// Es de otra llamada o no hay llamada activa: no se escribe ni se reporta.
    Discarded,
}

/// Fallo al escribir el mensaje propio `id` de la llamada `generation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatSendError {
    pub id: u64,
    pub generation: u64,
    pub reason: String,
}

impl ChatSendError {
    /// Notificación para la UI, que marca el mensaje como "no enviado".
    pub fn to_notification(&self) -> SctpNotification {
        SctpNotification::ChatSendFailed {
            id: self.id,
            generation: self.generation,
        }
    }
}

/// Escribe un mensaje de chat si pertenece a la llamada actual.
pub fn send_chat<W: ChatWriter + ?Sized>(
    writer: &mut W,
    id: u64,
    generation: u64,
    text: &str,
    current_generation: u64,
    in_call: bool,
) -> Result<ChatSendOutcome, ChatSendError> {
    if !in_call || generation != current_generation {
        return Ok(ChatSendOutcome::Discarded);
    }
    writer
        .write_chat(&chat::encode(text))
        .map(|()| ChatSendOutcome::Sent)
        .map_err(|reason| ChatSendError {
            id,
            generation,
            reason,
        })
}

/// Notificación a emitir cuando un comando llega sin asociación SCTP: un
/// `SendChat` se reporta como fallido para que no se pierda en silencio.
pub fn notify_without_association(cmd: &SctpCommand) -> Option<SctpNotification> {
    match cmd {
        SctpCommand::SendChat { id, generation, .. } => Some(SctpNotification::ChatSendFailed {
            id: *id,
            generation: *generation,
        }),
        // Los comandos de archivos mantienen su comportamiento: solo se registran
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::CHAT_MAX_BYTES;
    use crate::client::transport::sctp_harness::{Peer, SctpPair};

    const GEN: u64 = 1;

    struct FailingWriter;

    impl ChatWriter for FailingWriter {
        fn write_chat(&mut self, _payload: &[u8]) -> Result<(), String> {
            Err("fallo inyectado".to_string())
        }
    }

    fn send(peer: &mut Peer, id: u64, text: &str) -> Result<ChatSendOutcome, String> {
        let assoc = peer.assoc_mut().ok_or("sin asociación")?;
        send_chat(assoc, id, GEN, text, GEN, true).map_err(|e| e.reason)
    }

    fn chat(text: &str) -> SctpNotification {
        SctpNotification::ChatReceived {
            generation: GEN,
            text: text.to_string(),
        }
    }

    // Todo lo que recibió `peer`, clasificado como lo haría el hilo RX.
    fn routed(peer: &mut Peer) -> Vec<Incoming<'static>> {
        peer.take_received()
            .into_iter()
            .map(
                |(id, data)| match route_incoming(id, &data, GEN, CHAT_MAX_BYTES) {
                    Incoming::Chat(n) => Incoming::Chat(n),
                    Incoming::FileControl(c) => Incoming::FileControl(c),
                    Incoming::FileContent(_) => Incoming::FileContent(&[]),
                    Incoming::Ignored => Incoming::Ignored,
                },
            )
            .collect()
    }

    fn chats(incoming: &[Incoming<'_>]) -> Vec<SctpNotification> {
        incoming
            .iter()
            .filter_map(|i| match i {
                Incoming::Chat(n) => Some(n.clone()),
                _ => None,
            })
            .collect()
    }

    // Escribe un archivo en el stream 1 con el formato de la app:
    // [len u16][nombre][contenido] (`cliente.rs`, cola `files_to_send`).
    fn write_file(peer: &mut Peer, name: &str, content: &[u8]) -> Result<(), String> {
        let assoc = peer.assoc_mut().ok_or("sin asociación")?;
        let mut payload = Vec::with_capacity(2 + name.len() + content.len());
        payload.extend_from_slice(&(name.len() as u16).to_be_bytes());
        payload.extend_from_slice(name.as_bytes());
        payload.extend_from_slice(content);
        let mut stream = match assoc.stream(1) {
            Ok(s) => s,
            Err(_) => assoc
                .open_stream(1, PayloadProtocolIdentifier::Binary)
                .map_err(|e| format!("{e:?}"))?,
        };
        stream
            .write_sctp(&Bytes::from(payload), PayloadProtocolIdentifier::Binary)
            .map(|_| ())
            .map_err(|e| format!("{e:?}"))
    }

    #[test]
    fn chat_2_1_entrega_un_mensaje() -> Result<(), String> {
        let mut pair = SctpPair::connect()?;

        send(&mut pair.a, 1, "001")?;
        pair.drive();

        assert_eq!(routed(&mut pair.b), vec![Incoming::Chat(chat("001"))]);
        Ok(())
    }

    #[test]
    fn chat_2_2_preserva_orden_de_rafaga() -> Result<(), String> {
        let mut pair = SctpPair::connect()?;

        for (id, text) in [(1, "001"), (2, "002"), (3, "003")] {
            send(&mut pair.a, id, text)?;
        }
        pair.drive();

        assert_eq!(
            chats(&routed(&mut pair.b)),
            vec![chat("001"), chat("002"), chat("003")]
        );
        Ok(())
    }

    #[test]
    fn chat_2_3_oferta_de_archivo_no_genera_chat() {
        let incoming = route_incoming(0, b"OFFER foto.png 100", GEN, CHAT_MAX_BYTES);

        assert_eq!(
            incoming,
            Incoming::FileControl(FileControl::Offer {
                filename: "foto.png".to_string(),
                size: 100
            })
        );
    }

    #[test]
    fn chat_2_4_contenido_de_archivo_no_genera_chat() {
        let data = b"Hola, esto parece un chat";

        let incoming = route_incoming(1, data, GEN, CHAT_MAX_BYTES);

        assert_eq!(incoming, Incoming::FileContent(data));
    }

    #[test]
    fn chat_2_5_chat_con_texto_offer_no_genera_oferta() -> Result<(), String> {
        let mut pair = SctpPair::connect()?;

        send(&mut pair.a, 1, "OFFER foto.png 100")?;
        pair.drive();

        assert_eq!(
            routed(&mut pair.b),
            vec![Incoming::Chat(chat("OFFER foto.png 100"))]
        );
        Ok(())
    }

    #[test]
    fn chat_2_6_chat_durante_transferencia_de_archivo() -> Result<(), String> {
        let mut pair = SctpPair::connect()?;
        let file: Vec<u8> = (0..64 * 1024).map(|i| (i % 251) as u8).collect();
        let de_a: Vec<String> = (0..20).map(|i| format!("a{i:02}")).collect();
        let de_b: Vec<String> = (0..20).map(|i| format!("b{i:02}")).collect();

        write_file(&mut pair.a, "prueba.bin", &file)?;
        for (i, (ta, tb)) in de_a.iter().zip(&de_b).enumerate() {
            send(&mut pair.a, i as u64 + 1, ta)?;
            send(&mut pair.b, i as u64 + 1, tb)?;
        }
        pair.drive();

        let recibido_b = pair.b.take_received();
        let chats_b: Vec<SctpNotification> = recibido_b
            .iter()
            .filter_map(
                |(id, d)| match route_incoming(*id, d, GEN, CHAT_MAX_BYTES) {
                    Incoming::Chat(n) => Some(n),
                    _ => None,
                },
            )
            .collect();
        let archivos_b: Vec<Vec<u8>> = recibido_b
            .iter()
            .filter_map(
                |(id, d)| match route_incoming(*id, d, GEN, CHAT_MAX_BYTES) {
                    Incoming::FileContent(c) => Some(c.to_vec()),
                    _ => None,
                },
            )
            .collect();

        let esperados: Vec<SctpNotification> = de_a.iter().map(|t| chat(t)).collect();
        assert_eq!(chats_b, esperados);
        assert_eq!(archivos_b.len(), 1);
        let header = 2 + "prueba.bin".len();
        assert_eq!(archivos_b[0].get(header..), Some(file.as_slice()));

        let esperados_a: Vec<SctpNotification> = de_b.iter().map(|t| chat(t)).collect();
        assert_eq!(chats(&routed(&mut pair.a)), esperados_a);
        Ok(())
    }

    #[test]
    fn chat_2_7_fallo_de_escritura_reporta_id() -> Result<(), String> {
        let mut pair = SctpPair::connect()?;

        let result = send_chat(&mut FailingWriter, 1, GEN, "001", GEN, true);

        assert_eq!(
            result.map_err(|e| e.to_notification()),
            Err(SctpNotification::ChatSendFailed {
                id: 1,
                generation: GEN
            })
        );
        // El cliente sigue operativo: el envío siguiente llega
        send(&mut pair.a, 2, "002")?;
        pair.drive();
        assert_eq!(chats(&routed(&mut pair.b)), vec![chat("002")]);
        Ok(())
    }

    #[test]
    fn chat_2_8_comandos_pendientes_no_pasan_a_nueva_llamada() -> Result<(), String> {
        let mut pair = SctpPair::connect()?;
        let pendientes = [(1, "001"), (2, "002")];
        let nueva_generacion = GEN + 1;

        let assoc = pair.a.assoc_mut().ok_or("sin asociación")?;
        for (id, text) in pendientes {
            let result = send_chat(assoc, id, GEN, text, nueva_generacion, true);
            assert_eq!(result, Ok(ChatSendOutcome::Discarded));
        }
        pair.drive();

        assert_eq!(chats(&routed(&mut pair.b)), vec![]);
        Ok(())
    }

    #[test]
    fn chat_2_9_envio_sin_asociacion_reporta_fallo() {
        let cmd = SctpCommand::SendChat {
            id: 1,
            generation: GEN,
            text: "001".to_string(),
        };

        assert_eq!(
            notify_without_association(&cmd),
            Some(SctpNotification::ChatSendFailed {
                id: 1,
                generation: GEN
            })
        );
        // Los comandos de archivos mantienen su comportamiento (solo log)
        let file_cmd = SctpCommand::SendFile {
            path: "/tmp/a.bin".to_string(),
        };
        assert_eq!(notify_without_association(&file_cmd), None);
    }
}
