# Hito 0 — Exploración del chat durante la videollamada

Responde la sección "Antes de empezar" de [`roomRTC_todo.md`](roomRTC_todo.md). Cada punto se marca como **hecho** (verificado en el código, con archivo y línea) o **hipótesis** (no verificado en ejecución).

## 1. Línea base

- **Hecho.** Los resultados reales de `cargo build`, `cargo test`, `cargo fmt --all -- --check` y `cargo clippy --all-targets` están en `docs/baseline/` (`baseline.test.txt`, `baseline.clippy.txt`, `baseline.clippy.new.txt`, `baseline.fmt.txt`, `baseline.build.txt`).
- **Hecho.** Dependencias del sistema: GStreamer, OpenCV/clang (Linux) y OpenSSL.
- **Hecho.** `tests/t_signaling.rs` levanta el binario `server` en el puerto fijo 8443 y genera certificados con OpenSSL. Falla si ya hay un servidor escuchando en ese puerto.

## 2. Mapa de streams SCTP

| Stream | Contenido | Quién lo abre | TX | RX |
|--------|-----------|---------------|----|----|
| 0 | Control de archivos en texto: `OFFER <nombre> <tamaño>`, `ACCEPT <nombre>`, `REJECT <nombre>` (`PayloadProtocolIdentifier::String`) | El emisor, cuando lo necesita por primera vez (`src/bin/cliente.rs:708-711`, `:742-745`, `:764-767`) | Hilo SCTP TX | `src/bin/cliente.rs:424-520` |
| 1 | Contenido de archivo `[len u16][nombre][bytes]` (`Binary`) | `Event::Connected` (`src/bin/cliente.rs:531-536`) o el emisor (`:807-810`) | Hilo SCTP TX, cola `files_to_send` | `src/bin/cliente.rs:367-423` |
| otros | Sin uso. Solo se registran con `println!` | — | — | `src/bin/cliente.rs:521-527` |

- **Hecho.** La cantidad de streams entrantes y salientes está limitada a `SCTP_MAX_USERS = 16` (`src/client/types.rs:43`).
- **Hecho.** Locks: el hilo SCTP TX toma `sctp_association` al principio de cada iteración y lo mantiene hasta el final (`src/bin/cliente.rs:682`). El hilo RX toma el mismo lock para procesar los paquetes entrantes. Enviar un chat dentro de la iteración existente del hilo TX no requiere un lock nuevo.

## 3. Ciclo de vida

- **Hecho.** `spawn_dtls_handshake_thread` hace un único `role_rx.recv()`, negocia DTLS, crea el endpoint y la asociación SCTP, y termina (`src/client/transport/dtls_manager.rs:64-222`).
- **Hecho.** La asociación, `dtls_stream` y los canales `sctp_cmd` y `sctp_notif` se crean una vez por proceso (`src/bin/cliente.rs:90-103`). `EndCall` y `RemoteCallEnded` solo cambian `running_call`, los frames y la pantalla. No reinician ni vacían nada de lo anterior (`src/media/unified_app.rs:450-486`).
- **Hecho.** El hilo SCTP TX descarta un comando solo si lo lee con `running_call == false` (`src/bin/cliente.rs:675-680`). Que un comando pendiente al cortar se descarte o se envíe en la llamada siguiente depende del momento en que el hilo lo lee.
- **Hecho.** Una notificación que llega con la app en el lobby queda en el canal. La consume el primer `FrameUpdate` de la llamada siguiente (ver punto 4).
- **Hecho.** Una segunda llamada en el mismo proceso no vuelve a negociar DTLS ni SCTP: la señalización vuelve a enviar por `role_tx` (`src/bin/cliente.rs:1277`, `:1390`), pero nadie lo recibe.

## 4. Consumo de notificaciones en `FrameUpdate`

- **Hecho.** Cada `FrameUpdate` hace un solo `try_recv()` sobre `sctp_notif_rx` (`src/media/unified_app.rs:421`). Es decir, una notificación por tick.
- **Hecho.** `FrameUpdate` solo se suscribe en la pantalla `VideoCall` (`src/media/unified_app.rs:1118`). En el lobby no se consume nada.

## 5. Decisiones

### 5.1 Transporte: stream SCTP dedicado número 2

- **Decisión.** El chat viaja por el stream 2 (`CHAT_STREAM_ID: u16 = 2`), con `PayloadProtocolIdentifier::String`, ordenado y confiable. Es la configuración por defecto de sctp-proto (`unordered: false`, `ReliabilityType::Reliable` en `sctp-proto-0.7.0/src/association/stream.rs:419-420`). Garantiza el orden por emisor sin duplicados (1.9, 2.2).
- **Descartada:** multiplexar el chat en el stream 0 con un prefijo. **Motivo:** mezcla el chat con el parser de `OFFER `, de modo que un chat con ese texto podría disparar una oferta (2.5), y obliga a tocar un código que hoy funciona.

### 5.2 Limpieza entre llamadas: generación de llamada

- **Decisión.** La app lleva una generación de llamada (`u64`) que se incrementa al empezar cada llamada. `SendChat`, `ChatReceived` y `ChatSendFailed` llevan la generación. Lo que llega con una generación distinta de la actual, o fuera de una llamada activa, se descarta. Cubre 1.11, 2.8, 3.7 y 3.8 sin depender del momento en que actúan los hilos.
- **Descartada:** vaciar los canales en `EndCall`. **Motivo:** no elimina la carrera (un mensaje puede llegar justo después de vaciar), depende de los hilos y no se puede testear con funciones puras.

### 5.3 Límites

- **Hecho.** El tamaño máximo de mensaje del transporte es 4 MB (`with_max_message_size(SCTP_MAX_FILE_SIZE)`, `src/client/transport/dtls_manager.rs:170`, `:210`).
- **Decisión.** Se confirman `CHAT_MAX_BYTES = 4096` (medido en bytes UTF-8) y `CHAT_HISTORY_MAX = 200`. Ambos son parámetros de la validación, la decodificación y el historial, para que los tests puedan pasar valores chicos.

### 5.4 Nombres

| Elemento | Nombre |
|----------|--------|
| Módulo | `room_rtc::chat` (`src/chat.rs`) |
| Error tipado | `ChatError`: `Empty`, `TooLong { len, max }`, `InvalidUtf8`, `NotInCall` (no hay llamada activa) y `ChannelNotReady` (hay llamada pero el canal de envío no está listo) |
| Límites | `ChatLimits { max_bytes, history_max }`, con `ChatLimits::PRODUCTION` armado con las constantes. Los tests inyectan valores chicos |
| Entrada del historial | `ChatEntry { author, text, not_sent }`, con `ChatAuthor::Own { id }` (propio, con id local) o `ChatAuthor::Peer` (del participante) |
| Mensaje saliente | `OutgoingChat { id, text }`, lo que devuelve `prepare_send`. La app lo convierte en `SendChat` en el Hito 3 |
| Estado en la app | `ChatState { history, draft, next_id }`, con `Default`, como campo de `UnifiedApp`. `ChatState::reset()` lo reinicia al empezar y al terminar cada llamada. Un campo en `AppScreen::VideoCall` obligaría a tocar los 4 literales de la variante; ver D3 en `openspec/changes/in-call-chat/design.md` |
| Comando | `SctpCommand::SendChat { id, generation, text }` |
| Notificaciones | `SctpNotification::ChatReceived { generation, text }`, `SctpNotification::ChatSendFailed { id, generation }` |
| Stream | `CHAT_STREAM_ID: u16 = 2` |
| Constantes | `CHAT_MAX_BYTES = 4096`, `CHAT_HISTORY_MAX = 200` |

### 5.5 Reporte de errores de envío

- **Hecho.** Hoy los errores de `write_sctp` solo se imprimen con `eprintln!` (por ejemplo `src/bin/cliente.rs:719`). La UI no se entera.
- **Decisión.** El chat agrega el primer camino de reporte: si falla la escritura de un `SendChat`, el hilo TX emite `ChatSendFailed` con el id local (2.7, 3.6).

### 5.6 Nueva llamada en el mismo proceso: fuera de alcance

- **Decisión.** No se corrige la renegociación de DTLS y SCTP entre llamadas. Es una limitación previa al chat. La limpieza del chat entre llamadas se prueba a nivel de modelo y de app (Hitos 1 a 3). En la prueba manual del Hito 4 se reinician ambos clientes entre llamadas, y la limitación queda registrada.
- **Descartada:** hacer que la configuración DTLS/SCTP corra una vez por llamada. **Motivo:** es un refactor del transporte más grande que el chat. Merece un change propio.

## 6. Puntos de integración para la implementación

- Extraer de `run_media_client` (`src/bin/cliente.rs:69`) dos funciones puras: una procesa un payload recibido según su `stream_id` y otra prepara un envío de chat. La rama "Otros streams" (`:521`) pasa a ser el camino de `CHAT_STREAM_ID`.
- Los tests del Hito 3 reutilizan `make_app_with_channels()` y `force_video()` (`src/media/unified_app.rs:1178`, `:1230`).
