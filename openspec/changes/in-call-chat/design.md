# Design

## Context

Motivación y alcance: ver `proposal.md`. Requisitos: ver `specs/call-chat/spec.md`. La exploración completa, con el mapa de streams y el ciclo de vida, está en `docs/chat/exploracion.md`. Acá van solo los hechos que condicionan el diseño. Las líneas citadas corresponden al código al cierre del Hito 2:

- **Hecho.** La UI y la red se hablan por dos canales mpsc creados una vez por proceso: `SctpCommand` (UI → hilo SCTP TX) y `SctpNotification` (hilo RX → UI) (`src/bin/cliente.rs:102-108`).
- **Hecho.** Antes del Hito 2, el hilo RX demultiplexaba por `stream_id` con un `if id == 1 / else if id == 0 / else` dentro de un closure de `run_media_client`, y no se podía testear. Desde el Hito 2 hace un `match` sobre `route_incoming` (D5): contenido de archivo (`cliente.rs:380`), control de archivo (`:438`), chat y lo ignorado (`:480`).
- **Hecho.** El hilo SCTP TX mantiene el lock de la asociación durante su iteración y descarta un comando solo si lo lee con `running_call == false` (`cliente.rs:635-642`).
- **Hecho.** `FrameUpdate` consume una notificación por tick y solo corre en `VideoCall` (`src/media/unified_app.rs:453`, `:1153`).
- **Hecho.** `AppScreen::VideoCall { .. }` se construye en 4 lugares (`unified_app.rs:369`, `:406`, `:477` y `force_video` en `:1270`).
- **Hecho.** `make_app_with_channels` descarta el emisor de notificaciones (`unified_app.rs:1228`).
- **Hipótesis (fuera de alcance).** Una segunda llamada en el mismo proceso no vuelve a negociar DTLS ni SCTP (`exploracion.md`, punto 3).

## Goals / Non-Goals

**Goals:**
- Que todas las reglas del chat se puedan testear sin hilos, sockets, cámara ni archivos de configuración. Lo que hoy está en closures del binario pasa a funciones puras de la librería.
- Que el chat no cambie el comportamiento de los streams 0 y 1 (mismos formatos, mismos mensajes, mismas notificaciones).

**Non-Goals:**
- Refactorizar la recepción del contenido de archivos (stream 1) más allá de lo necesario para enrutarla.
- Arreglar el ciclo de vida de DTLS y SCTP entre llamadas.
- Implementar confirmaciones de entrega. El estado "no enviado" refleja solo que la escritura en SCTP falló.

## Decisions

### D1. Stream SCTP dedicado: `CHAT_STREAM_ID = 2`, ordenado y confiable

El chat se escribe en el stream 2 con `PayloadProtocolIdentifier::String`, y el emisor lo abre cuando lo necesita por primera vez, como el stream 0 (`cliente.rs:668-671`). Lo hace `impl ChatWriter for Association` (`src/client/transport/sctp_chat.rs:81`). sctp-proto crea los streams ordenados y confiables por defecto (`stream.rs:419-420`), lo que da 1.9, 2.1 y 2.2 sin código extra.
- *Descartada:* multiplexar en el stream 0 con un prefijo. **Motivo:** el parser de `OFFER ` vería el chat (riesgo para 2.5) y habría que tocar un camino que hoy funciona.
- *Descartada:* un stream no ordenado. **Motivo:** obligaría a reordenar en la aplicación para cumplir el orden por emisor.

### D2. Generación de llamada compartida (`Arc<AtomicU64>`)

`UnifiedApp` incrementa `call_generation` cada vez que entra a `VideoCall` (`unified_app.rs:368`, `:405`, `:476`; se implementa en el Hito 3). El contador sigue el mismo camino que `running_call`:
- **Hecho.** `run_media_client` lo crea con valor 0 junto a `running_call` (`src/bin/cliente.rs:92`). Le pasa un clon a cada hilo que lo lee: el RX al clasificar un chat recibido (`:377`) y el SCTP TX al decidir si transmite (`:751`).
- **Hecho.** A la UI le llega por `UnifiedAppConfig::call_generation` (`src/media/unified_app.rs:195`, construido en `cliente.rs:1089`). `UnifiedApp` empieza a guardarlo y usarlo en la tarea 3.1.
- *Descartada:* llevarlo en `MediaClientConfig`, como decía la versión anterior de este diseño. **Motivo:** `MediaClientConfig` lo arma `main`, que no usa la generación. `running_call` tampoco viaja por ahí: lo crea `run_media_client` (`cliente.rs:90`). Agregarlo habría sido un campo sin uso.
- `SendChat { id, generation, text }`: el hilo TX lo transmite solo si `generation` coincide con la generación actual y `running_call` está activo. Si no, lo descarta sin escribir (2.8).
- `ChatReceived { generation, text }`: el hilo RX le pone la generación actual al recibir. La UI descarta lo que no coincide con su generación (3.8).
- `ChatSendFailed { id, generation }`: se descarta si pertenece a otra llamada.
- *Descartada:* vaciar los canales en `EndCall`. **Motivo:** no cierra la carrera (algo puede llegar justo después de vaciar) y no se puede testear sin hilos.
- *Descartada:* un id de llamada que viaje por la red. **Motivo:** cambia el formato de los mensajes sin necesidad, porque la asociación ya es punto a punto.

### D3. `ChatState` como campo de `UnifiedApp`, no de `AppScreen::VideoCall`

`ChatState` (D4) implementa `Default` y vive en `UnifiedApp`. Se reinicia con `ChatState::reset()` al entrar a `VideoCall` y en `EndCall` y `RemoteCallEnded` (1.11, 3.7). La vista y los mensajes del chat solo actúan si la pantalla actual es `VideoCall`.
- *Descartada:* agregar el campo a `AppScreen::VideoCall`, como decía `exploracion.md` en 5.4. **Motivo:** un campo nuevo en una variante de enum obliga a tocar los 4 literales, y `Default` no lo evita. Esto corrige esa entrada de la exploración.

### D4. Módulo de dominio `room_rtc::chat` (`src/chat.rs`)

Contiene solo funciones puras, sin `unwrap`, `expect` ni `panic!`. No depende de la red ni de la UI: no conoce `SctpCommand` ni la generación de llamada.

- **Constantes:** `CHAT_MAX_BYTES = 4096`, `CHAT_HISTORY_MAX = 200` y `CHAT_STREAM_ID = 2`.
- **`ChatLimits { max_bytes, history_max }`:** agrupa los dos límites. `ChatLimits::PRODUCTION` usa las constantes, y los tests construyen uno con valores chicos (16 bytes, tope 3) (1.3, 1.10, 1.12).
- **`ChatError`:** `Empty`, `TooLong { len, max }`, `InvalidUtf8`, `NotInCall` y `ChannelNotReady`.
  - `NotInCall`: no hay una llamada activa.
  - `ChannelNotReady`: hay llamada pero el canal de envío no está listo.
  - Son dos variantes para que cada caso de 1.7 tenga un error identificable.
- **`validate_outgoing(text, max_bytes) -> Result<(), ChatError>`:** rechaza lo vacío o de solo espacios (`trim().is_empty()`) y lo que supera `max_bytes` en `text.len()`. No modifica el texto (1.2, 1.3, 1.4).
- **`encode(text) -> Vec<u8>` y `decode(bytes, max_bytes) -> Result<String, ChatError>`:** `decode` rechaza el payload vacío, el que supera el límite y el UTF-8 inválido (`String::from_utf8`, sin `lossy`) (1.5, 1.6).
- **Historial:**
  - `ChatEntry { author, text, not_sent }` es una entrada del historial.
  - `ChatAuthor` puede ser `Own { id }` (mensaje propio con su id local) o `Peer` (mensaje del participante, sin id).
  - `not_sent` solo se activa en mensajes propios.
- **`ChatState { history: VecDeque<ChatEntry>, draft: String, next_id }`:** implementa `Default`. `history` y `draft` son públicos, `next_id` es privado.
  - `push_own(text, history_max) -> u64`: asigna el siguiente id local (empieza en 1) y descarta el mensaje más viejo si se supera el tope.
  - `push_peer(text, history_max)`: agrega un mensaje del participante con la misma regla.
  - `mark_failed(id) -> bool`: marca `not_sent` solo en la entrada `Own { id }` y devuelve si la encontró (1.8, 1.9, 1.10).
  - `reset()`: vuelve al estado por defecto, así que vacía el historial y el borrador y reinicia los ids (1.11). Los ids se repiten entre llamadas, pero D2 evita la confusión porque un `ChatSendFailed` de otra llamada se descarta por generación.
- **`OutgoingChat { id, text }`:** el mensaje a transmitir. `text` es exactamente el texto validado.
- **`prepare_send(state, text, in_call, channel_ready, limits) -> Result<OutgoingChat, ChatError>`** (1.1, 1.2, 1.4, 1.7):
  - Comprueba, en este orden, la llamada activa, el canal listo y `validate_outgoing`.
  - Si todo está bien, agrega el mensaje al historial con `push_own` y devuelve `OutgoingChat`.
  - Si algo falla, devuelve el `ChatError` sin tocar el estado.
- **Conversión a `SendChat`:** la hace la app en el Hito 3. Copia `id` y `text` de `OutgoingChat` y agrega la generación actual (D2, D6).
  - *Descartada:* que `prepare_send` devuelva directamente `SctpCommand::SendChat`. **Motivo:** ataría el modelo a `unified_app` y a la generación, y `SendChat` recién existe desde la tarea 2.2. El modelo del Hito 1 se puede testear sin ninguno de los dos.

### D5. Funciones de transporte extraídas a la librería (`src/client/transport/sctp_chat.rs`)

Para testear el Hito 2 sin el binario, `sctp_chat.rs` reúne funciones puras. Los hilos de `cliente.rs` las llaman y conservan los efectos (mapas de pendientes, envío de notificaciones, reensamblado del stream 1).

**Recepción.**
- `route_incoming(stream_id, data, generation, max_bytes) -> Incoming` (`sctp_chat.rs:54`). `Incoming` es un enum (`:22`): `Chat(SctpNotification)`, `FileControl(FileControl)`, `FileContent(&[u8])` o `Ignored`.
  - El stream 0 se parsea con `parse_file_control` (`:31`), que devuelve `FileControl::{Offer, Accept, Reject}` (`:14`). Copia el comportamiento previo: `from_utf8_lossy`, los prefijos `OFFER `, `ACCEPT ` y `REJECT `, y `rsplit_once` para el tamaño. Un mensaje de control que no se reconoce da `Ignored`.
  - El stream 1 da `FileContent` sin interpretarlo.
  - `CHAT_STREAM_ID` se decodifica con `chat::decode`. Si es válido da `Chat(ChatReceived { generation, text })`, con la generación actual. Si es inválido da `Ignored`, y el binario lo registra en la rama `Incoming::Ignored` (`cliente.rs:480`).

**Envío.**
- `send_chat(writer, id, generation, text, current_generation, in_call) -> Result<ChatSendOutcome, ChatSendError>` (`sctp_chat.rs:127`), sobre el trait `ChatWriter` (`:77`), implementado para `Association` (`:81`). Recibe el `id` local para poder reportarlo, y separa dos resultados que no se tratan igual:
  - `Ok(ChatSendOutcome::Discarded)`: el comando es de otra llamada (`generation != current_generation`) o no hay llamada activa (`in_call == false`). No se escribe ni se reporta, porque un mensaje viejo no debe aparecer como "no enviado" en la llamada nueva (2.8). El hilo TX solo lo registra (`cliente.rs:757`).
  - `Err(ChatSendError { id, generation, reason })` (`sctp_chat.rs:110`): la escritura falló. Se reporta: `to_notification()` (`:118`) lo convierte en `ChatSendFailed { id, generation }`, y el hilo TX lo envía a la UI (`cliente.rs:765`) (2.7).
  - `Ok(ChatSendOutcome::Sent)`: quedó encolado en el stream de chat.
- `notify_without_association(cmd) -> Option<SctpNotification>` (`sctp_chat.rs:150`): cuando el hilo TX recibe un comando y no hay asociación SCTP (`cliente.rs:843-848`), un `SendChat` se reporta como `ChatSendFailed { id, generation }`. Los comandos de archivos dan `None` y conservan el comportamiento previo, que es solo registrarlos (2.9, D6).

**Harness de test (`src/client/transport/sctp_harness.rs`).** Es un módulo `#[cfg(test)]` (`transport/mod.rs:5`). Conecta en memoria dos extremos de sctp-proto, sin sockets ni DTLS: `SctpPair::connect()` (`:145`), con `a` como caller y `b` como callee. Usa la misma configuración de transporte que la app (`transport_config`, `:23`, igual a `dtls_manager.rs:166-170`).
- `SctpPair::drive()` (`:184`) entrega los paquetes de `poll_transmit` de un lado con `Endpoint::handle` en el otro, hasta que no hay más tráfico. Adelanta un reloj virtual a los timers pendientes (retransmisiones, SACK demorados) dentro de un horizonte de 10 s, con un tope de pasos (`:19`, `:21`).
- `Peer::assoc_mut()` (`:51`) da acceso a la asociación para escribir.
- `Peer::take_received()` (`:57`) devuelve lo recibido como `(stream_id, payload)`, en orden dentro de cada stream.
- Los tests `chat_2_*` (`sctp_chat.rs:161`) lo usan con `route_incoming` y `send_chat`, como lo harían los hilos RX y TX.

- *Descartada:* dejar la lógica en el binario y testearla con `mod tests` dentro de `cliente.rs`. **Motivo:** los closures dependen de `DtlsStream` y de hilos, así que se terminaría testeando con red real, lo que excluye la DoD.
- *Descartada:* que `send_chat` reciba el `SctpCommand` y devuelva `Result<(), ChatSendError>`, como decía la versión anterior. **Motivo:** sin el `id` no se puede testear que el error de 2.7 queda asociado al id 1, y con `()` el descarte de un mensaje viejo se confundiría con un envío correcto o habría que reportarlo como error.

### D6. Integración en la UI

Mensajes nuevos de `AppMessage`: `ChatDraftChanged(String)`, `ChatSend` (botón y `on_submit` del `text_input`), `ChatReceived { generation, text }` y `ChatSendFailed { id, generation }`. Las líneas citadas en D6 corresponden al código al cierre del Hito 3.

**Procesamiento en el mismo tick.**
- **Hecho.** `FrameUpdate` toma una sola notificación por tick, como antes. Primero la saca del receptor y suelta el lock (`self.sctp_notif_rx.try_lock().ok().and_then(|guard| guard.try_recv().ok())`, `unified_app.rs:466-472`), y recién después la procesa (`:473`).
- **Por qué se suelta el lock antes.** Para procesar el chat, `FrameUpdate` llama a `self.update(...)`, que necesita `&mut self`. Si el guard del receptor siguiera vivo, `self` quedaría prestado y el código no compilaría (E0502).
- **Hecho.** Las notificaciones de archivos siguen como antes: `Task::done(AppMessage::IncomingFileOffer { .. })` y las demás (`:476`).
- **Hecho.** Las de chat se procesan en el mismo tick con `return self.update(AppMessage::ChatReceived { .. })` y `return self.update(AppMessage::ChatSendFailed { .. })` (`:486`, `:489`). Así hay un solo camino por mensaje: los handlers de `AppMessage::ChatReceived` y `ChatSendFailed`, que descartan lo que no es de la llamada actual (`is_current_call`, D2).
- *Descartada:* `Task::done(AppMessage::ChatReceived { .. })`, como las notificaciones de archivos. **Motivo:** los tests llaman a `update` y no ejecutan el `Task` que devuelve, así que 3.5 ("una notificación por tick") no se podría observar después de cada tick.

**Panel de chat.**
- **Hecho.** `view_chat` (`unified_app.rs:1217`) se ubica en la misma fila que los dos videos. El modal de oferta de archivos sigue superpuesto sobre todo el contenido, como antes.
- **Hecho.** Cada línea es un widget `text`, que muestra el texto literal sin interpretar marcado. La arma `chat_line`, que recibe los dos nombres. El método `UnifiedApp::chat_line` se los pasa (3.12, cambio pedido tras la prueba manual):
  - Mensaje propio: `<current_username>: <texto>`. Si no hay usuario logueado, como en algunos tests, se usa `Yo`.
  - Mensaje propio no enviado: `<current_username>: <texto> (no enviado)`, en rojo.
  - Mensaje del participante: `<peer_username>: <texto>`. Si no se conoce el nombre del par, se usa `Participante`.
- **De dónde sale `peer_username`** (campo de `UnifiedApp`):
  - **Hecho.** Si me llamaron, el nombre del caller está en `incoming_call` cuando llega `AcceptCall`, y se guarda al entrar a `VideoCall`.
  - **Hecho.** Si llamé yo, `CallWasAccepted` no trae el nombre, porque `SignalingResponse::CallAccepted` no lo lleva. El nombre del callee solo se conoce en `CallUserByClick`. Por eso se guarda en un campo `pending_callee`, que pasa a `peer_username` en `CallWasAccepted` y se limpia en `CallRejected`. Si se hacen dos llamadas antes de que una se acepte, queda el último callee.
  - **Hecho.** `JoinCall` no trae nombre, así que `peer_username` queda en `None`.
  - Al terminar la llamada (`EndCall`, `RemoteCallEnded`), `peer_username` vuelve a `None`.
  - *Descartada:* agregar el nombre a `SignalingResponse::CallAccepted`. **Motivo:** obliga a tocar la señalización entre el hilo de autenticación y la UI, y el dato ya está disponible en la app.
- **Hecho.** El `text_input` muestra el borrador, lo actualiza con `ChatDraftChanged` y envía con Enter mediante `on_submit(AppMessage::ChatSend)`. Al lado hay un botón "Enviar".
- **Tamaño (ajuste de la prueba manual, sin cambio de requisitos).** El campo de texto era muy chico, así que se agrandó:
  - el panel pasa de 300 a 360 px de ancho;
  - el `text_input` ocupa todo el ancho que deja el botón (`Length::Fill`), con padding 10 y texto de 16 px, lo que da unos 40 px de alto;
  - el botón "Enviar" usa el mismo padding.
- **Layout del panel (corrección de la prueba manual).** En la captura `docs/chat/evidencia/imagen_texto.png`, el `text_input` y el botón aparecen como una barra de pocos píxeles de alto. El ancho estaba bien (360 px); lo que fallaba era el alto.
  - **Hecho.** Los videos no tienen tamaño fijo: en iced 0.13, `center_x(Length::Fill)` y `center_y(Length::Fill)` reemplazan el ancho y el alto del `container` por `Fill` (`iced_widget-0.13.4/src/container.rs:147-154`). Por eso la fila de videos y chat recibe solo el alto que le queda en la ventana.
  - **Hecho.** El layout flex de iced ubica en orden a los hijos que no son `Fill` y descuenta su tamaño del espacio disponible (`iced_core-0.13.2/src/layout/flex.rs:88`, `:117`). El panel pedía un alto fijo: título, más un `scrollable` de `VIDEO_HEIGHT_F32 - 90.0`, más la fila de entrada. Cuando la ventana es baja, la fila de entrada, que es el último hijo, queda casi sin alto.
  - **Decisión.** La columna del panel toma `height(Length::Fill)` y el `scrollable` del historial toma `Length::Fill`. El título y la fila de entrada (`text_input` con `Length::Fill` y el botón "Enviar" al lado, centrados verticalmente) se ubican primero con su alto propio, y el historial ocupa lo que sobra.
  - **Ancho.** Queda fijo en 360 px (al menos 300). Los anchos fijos se reparten antes que los `Fill`, así que el chat siempre recibe sus 360 px y los dos videos se reparten el resto.
  - *Descartada:* `FillPortion` para el panel. **Motivo:** el ancho del chat pasaría a depender del tamaño de la ventana y podría quedar por debajo de 300 px. Con el ancho fijo, el que se achica es el video.
  - *Descartada:* dar a los videos un tamaño fijo (sacar `center_x(Fill)` y `center_y(Fill)`). **Motivo:** cambia el layout de los videos, que no forma parte del chat.
  - **Supuesto.** Es el layout esperado según las reglas de iced, pero no se verificó en pantalla. Se confirma en el paso M6 de la prueba manual.
- **Supuesto.** Las etiquetas, la ubicación y el tamaño del panel se confirman en la prueba manual del Hito 4 (ver Open Questions).

**Fallo al entregar el comando al transporte (3.11).**
- **Hecho.** Si `sctp_tx.send(SendChat { .. })` falla porque el hilo SCTP TX ya no recibe comandos, `ChatSend` marca el mensaje como "no enviado" con `mark_failed(id)` (`unified_app.rs:575-578`). El mensaje ya se agregó al historial, así que no se pierde en silencio. Es el mismo requisito de reporte de fallo de envío de la spec, con su propio escenario (`chat_3_11`).

**De dónde salen las condiciones de `prepare_send` (D4).** Cuando llega `ChatSend`, la app las calcula así:
- **Llamada activa:** la pantalla actual es `AppScreen::VideoCall`. No se usa `running_call`.
- **Canal listo:** `sctp_tx` está configurado (`Some`), como condición previa, y además `running_call` vale `true`. Si falta cualquiera de las dos, `prepare_send` devuelve `ChannelNotReady` y no sale ningún `SendChat`.

Hechos sobre `running_call` que justifican el criterio:
- **Hecho.** Solo la UI lo pone en `false`: en `EndCall` (`src/media/unified_app.rs:492`), en `RemoteCallEnded` (`:509`) y en `SystemClose` (`:563`). `src/bin/cliente.rs` y `src/client/transport/dtls_manager.rs` solo lo leen (por ejemplo el hilo SCTP TX en `cliente.rs:635`).
- **Hecho.** Lo ponen en `true` el hilo de DTLS, al terminar el handshake y crear el endpoint SCTP (`dtls_manager.rs:221`), y también la UI al entrar a `VideoCall`, antes del handshake: al aceptar (`unified_app.rs:368`), en `CallWasAccepted` (`:405`) y en `JoinCall` (`:476`).
- **Consecuencia.** `running_call` en `true` **no** garantiza que el handshake terminó ni que haya asociación SCTP. Dentro de `VideoCall` casi siempre vale `true`, así que en la práctica "canal listo" depende sobre todo de `sctp_tx`. Se mantiene igual como condición, porque es la misma señal que usa el hilo TX para descartar comandos (`cliente.rs:635-640`).
- **Hecho.** Antes del Hito 2, si el hilo TX recibía un comando sin asociación, solo lo registraba y lo descartaba. Ahora sigue registrándolo (`cliente.rs:843`) y, para que un mensaje enviado antes del handshake no se pierda en silencio, emite `ChatSendFailed` mediante `notify_without_association` (`cliente.rs:845`, `sctp_chat.rs:150`), así que el mensaje queda "no enviado". Es el mismo requisito de reporte de fallo de envío de la spec, con su propio escenario (`chat_2_9`).
- *Descartada:* una señal nueva de "canal listo" activada en `Event::Connected` (`cliente.rs:489`). **Motivo:** agrega estado compartido y trabajo al Hito 2, y el caso sin asociación ya queda visible como "no enviado".

**Tests.** Para los tests del Hito 3 se agrega un helper que también devuelve el emisor de notificaciones, deja `running_call` en `true` y la pantalla en `VideoCall`. `make_app_with_channels` mantiene su firma, así que los tests existentes no cambian (3.9). `chat_3_10` cubre `running_call` en `false` dentro de `VideoCall`: `ChatSend` no emite `SendChat` y el historial no cambia.

### D7. Hito 4 automático: dos apps unidas por un puente de test, sin hilos

**Qué se puede ejecutar de punta a punta sin hilos.**
- **Hecho.** Todo el camino del chat, salvo el código de los hilos: `UnifiedApp` A emite `SendChat`, se escribe en una asociación SCTP real (harness en memoria, D5), `route_incoming` lo clasifica en el otro extremo, la notificación entra al receptor de `UnifiedApp` B y un `FrameUpdate` la muestra. Lo mismo vale para el sentido contrario.
- **Hecho.** Lo que no se puede ejecutar en un test son los bucles de los hilos RX y SCTP TX de `run_media_client` (`src/bin/cliente.rs`). Están dentro de closures del binario y dependen de sockets UDP, DTLS y el pipeline de cámara de GStreamer (`setup_iced_video_client`, `src/client/media/video_pipeline.rs:103`).

**Decisión: puente de test (`ChatBridge`).** Es un helper `#[cfg(test)]` que hace, en un solo hilo, lo que hacen esos bucles, con las mismas funciones de la librería:
- **Lado TX:** saca los comandos del receptor `SctpCommand` de cada app. Un `SendChat` se escribe con `send_chat` en la asociación del harness, con la generación y el `running_call` de esa app. Un `Err` se entrega como `ChatSendFailed` a la propia app, como el hilo TX (`cliente.rs:765`).
- **Mensajes de control:** se escriben en el stream 0, como `OFFER <nombre> <tamaño>`, igual que el hilo TX.
- **Lado RX:** después de `SctpPair::drive()`, toma lo recibido por cada extremo y lo clasifica con `route_incoming`, con la generación de la app receptora. Las notificaciones de `Chat` y la `FileOffer` de un `FileControl::Offer` se entregan al emisor de notificaciones de esa app, como el hilo RX (`cliente.rs:438`, `:476`).
- **Uso desde los tests:** los tests envían los `FrameUpdate` a mano, igual que en el Hito 3.
- **Hecho.** La oferta de archivo ya llega a la app por `IncomingFileOffer`, que guarda `pending_file_offer` (`unified_app.rs:665`). Aceptarla emite `SctpCommand::AcceptFile` y limpia la oferta (`:683-698`), así que 4.2 la verifica sin tocar el flujo de archivos.

**Ejecución de los `Task` en los tests del Hito 4 (hallazgo de la tarea 4.2).**
- **Hecho.** `FrameUpdate` no procesa la `FileOffer` en el mismo tick: devuelve `Task::done(AppMessage::IncomingFileOffer { .. })` (`unified_app.rs:486-487`), y el runtime de iced es el que después pasa ese mensaje a `update`. Los tests llaman a `update` y no ejecutan el `Task`, así que sin algo más `pending_file_offer` nunca se guarda y 4.2 no se puede observar.
- **Hecho.** iced 0.13 no tiene una API pública para leer el contenido de un `Task`: `iced::task` solo exporta `Handle` y `Task`. La función que da el stream de acciones es `iced_runtime::task::into_stream` (`iced_runtime-0.13.2/src/task.rs:365`), y `iced_runtime` no era una dependencia directa.
- **Decisión.** `iced_runtime = "0.13"` se agrega como `dev-dependency`, que resuelve a la misma versión que usa `iced`, así que el tipo `Task` es el mismo. Un helper de los tests e2e, `run_task`, hace lo que el runtime: toma el stream del `Task`, lee las acciones listas con `now_or_never` y pasa cada `Action::Output(msg)` a `update`, ejecutando también el `Task` que devuelve. Las acciones que no son `Output` se ignoran. El tick de los tests e2e es `update(FrameUpdate)` más `run_task`.
- **Por qué `now_or_never` y no `block_on`.** Un `Task` que espera al runtime real (operaciones de ventana) no termina nunca en un test. Con `now_or_never` el helper solo toma lo que ya está listo, como `Task::done`, y no se cuelga.
- **No cambia el código de producción** ni el flujo de archivos.
- *Descartada:* procesar la `FileOffer` en el mismo tick, como el chat. **Motivo:** cambia el flujo de archivos, que la spec pide no tocar.
- *Descartada:* poner la oferta pendiente enviando `IncomingFileOffer` a mano. **Motivo:** no probaría que el tick entrega la oferta que llevó el puente.

**Ubicación.**
- **Archivo:** `src/media/unified_app_e2e_tests.rs`, declarado en `unified_app.rs` como módulo hijo `#[cfg(test)]` con `#[path]`. Así ve los campos privados de `UnifiedApp` (`screen`, `chat`), igual que `mod tests`.
- **Helpers:** reutiliza los del Hito 3 (`make_app_with_all_channels`, `force_video`), que pasan a `pub(super)` dentro de `mod tests`, sin cambiar su comportamiento.
- **Dependencias:** usa `SctpPair` del harness, que ya es `pub(crate)`.

**RED de los tests `chat_4_*`.** La funcionalidad del chat ya existe, así que la que falta es el puente. Primero se escribe `ChatBridge` con un `pump` neutro que no mueve nada, y los tests fallan por aserción porque nada llega. Después se implementa `pump`.

**Alternativas.**
- *Descartada:* lanzar los hilos de `run_media_client` en el test. **Motivo:** requiere sockets UDP, DTLS y la cámara, que la DoD excluye.
- *Descartada:* extraer los bucles de los hilos a la librería para testearlos tal cual. **Motivo:** es un refactor grande de `cliente.rs`, fuera del alcance del chat. El código que une las funciones dentro de esos bucles queda cubierto por la prueba manual (D8).

### D8. Hito 4 manual: dos procesos reales

**Entorno.** El servidor de señalización (`cargo run --bin server`) y dos `cliente` corren en la misma máquina, desde la raíz del repo, con `src/config/cliente1_mac.config` y `src/config/cliente2_mac.config` en la Mac. El resultado de cada paso (M1 a M6 de `docs/chat/test_aceptacion.md`) se registra en `docs/chat/evidencia/prueba_manual.md`.

**Hechos que condicionan los pasos.**
- **Hecho.** El archivo recibido se guarda como `received_<nombre>` en el directorio de trabajo del proceso que lo recibe (`cliente.rs:415`). La comparación de M2 se hace con `shasum -a 256` del original y de ese archivo.
- **Hecho.** El tamaño máximo de mensaje SCTP es 4 MB (`SCTP_MAX_FILE_SIZE`, `src/client/types.rs:46`), así que el archivo de M2 tiene que pesar menos.
- **Hipótesis.** La segunda llamada sin reiniciar (M5) no vuelve a negociar DTLS ni SCTP (`exploracion.md`, punto 3). Su resultado confirma o refuta esa hipótesis, y no cuenta como fallo del chat (proposal, Fuera de alcance).

**Medición de latencia (M2).** Los logs actuales (`println!`) no llevan marca de tiempo, así que se mide desde afuera:
- **Método:** grabación de pantalla, contando cuadros entre el Enter y la aparición del mensaje. Si no se puede, cronómetro.
- **Límites:**
  - Resolución de ±1 cuadro (unos 17 ms a 60 fps), o la reacción humana (unos 200 ms) con cronómetro.
  - Los dos procesos comparten máquina y loopback, así que la medición no representa una red real.
  - Mide la percepción del usuario, no la latencia del transporte.
- **Interrupción de medios:** se registra cualitativamente (cortes de audio o congelamiento del video), con las mismas limitaciones.

**Limitaciones de la Mac a registrar.**
- Permiso de cámara para `avfvideosrc`.
- El cliente puede quedar colgado al cerrar la ventana. Es una limitación reportada, que no se verificó en el código ni tiene causa identificada. M4 registra si ocurre y en qué condiciones.
- La plantilla tiene una sección para estas y otras limitaciones que aparezcan.

## Supuestos

Los escenarios del Hito 2 en `specs/call-chat/spec.md` dependían de dos supuestos. Los dos quedaron verificados en el Hito 2:
- **Verificado.** Dos asociaciones de sctp-proto se conectan en memoria intercambiando los paquetes de `poll_transmit`, sin sockets ni DTLS. Ambos extremos llegan a `Event::Connected` (`harness_conecta_dos_endpoints_en_memoria`, en `sctp_harness.rs`), y sobre eso pasan 2.1, 2.2, 2.5, 2.6, 2.7 y 2.8.
- **Verificado.** El fallo de escritura de 2.7 se inyecta en `send_chat` con un `ChatWriter` que falla (`FailingWriter`, en los tests de `sctp_chat.rs`), sin un transporte real.

## Risks / Trade-offs

- **[Riesgo, cerrado en el Hito 2] Las asociaciones de sctp-proto en memoria podrían no conectarse sin un transporte real.** → La tarea 2.1 construyó el harness (`sctp_harness.rs`, D5) y ambos extremos se conectan.
- **[Riesgo] Contención del lock de la asociación entre el hilo TX y el hilo RX durante una transferencia de 4 MB.** → El chat se escribe dentro de la iteración que ya existe (sin locks nuevos), y la escritura de sctp-proto solo encola. En el Hito 4 se mide la latencia percibida en la prueba manual (M2, D8), con los límites de esa medición.
- **[Riesgo] El puente de test (D7) podría no reflejar lo que hacen los hilos de `cliente.rs`.** → Usa las mismas funciones y el mismo orden (`send_chat`, `route_incoming`, notificación). Lo que el puente no ejercita, que es el código que une todo dentro de los hilos, lo cubre la prueba manual (D8).
- **[Trade-off] "No enviado" solo cubre los errores de `write_sctp`.** Un mensaje encolado que nunca llega no se detecta, porque el MVP no tiene confirmación de entrega (fuera de alcance).
- **[Trade-off] El contador de generación agrega un `Arc<AtomicU64>` a `UnifiedAppConfig`, no a `MediaClientConfig` (D2).** → Es un cambio aditivo. Solo lo construyen `run_media_client` (`cliente.rs:92`, `:1089`), `run_unified_app` y el helper de tests `make_app_with_channels`.
- **[Riesgo] Mover el parseo del stream 0 a `parse_file_control` podría cambiar el comportamiento de archivos.** → 2.3 y los tests existentes cubren la regresión, y la función copia la lógica previa. Se verificó además que el cuerpo del stream 1 en el hilo RX quedó idéntico, salvo espacios. Único cambio observable: un mensaje de control que no se reconoce ahora se registra como `Mensaje en stream 0: …` en vez de `Control message: …` (solo el log).

## Open Questions

- El ancho y la ubicación exactos del panel de chat dentro de la vista de la llamada (D6). No cambian los requisitos ni las tareas. Tras la primera prueba manual, el panel pasó a 360 px y el campo de texto se agrandó, porque era muy chico. Se vuelve a confirmar en el paso M6 del Hito 4. Las etiquetas ya no son una pregunta abierta: las fija el criterio 3.12 (nombres de usuario).
