# Design

## Context

Motivación y alcance: ver `proposal.md`. Requisitos: ver `specs/call-chat/spec.md`. La exploración completa, con el mapa de streams y el ciclo de vida, está en `docs/chat/exploracion.md`. Acá van solo los hechos que condicionan el diseño:

- **Hecho.** La UI y la red se hablan por dos canales mpsc creados una vez por proceso: `SctpCommand` (UI → hilo SCTP TX) y `SctpNotification` (hilo RX → UI) (`src/bin/cliente.rs:96-103`).
- **Hecho.** El hilo RX demultiplexa por `stream_id`: 1 es contenido de archivo (`:367`), 0 es control de archivo (`:424`), y cualquier otro solo se registra (`:521`). Toda esa lógica vive dentro de un closure de `run_media_client`, en el binario, y hoy no se puede testear.
- **Hecho.** El hilo SCTP TX mantiene el lock de la asociación durante su iteración y descarta un comando solo si lo lee con `running_call == false` (`:675-682`).
- **Hecho.** `FrameUpdate` consume una notificación por tick y solo corre en `VideoCall` (`src/media/unified_app.rs:421`, `:1118`).
- **Hecho.** `AppScreen::VideoCall { .. }` se construye en 4 lugares (`unified_app.rs:337`, `:374`, `:442` y `force_video` en `:1231`).
- **Hecho.** `make_app_with_channels` descarta el emisor de notificaciones (`:1191`).
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

El chat se escribe en el stream 2 con `PayloadProtocolIdentifier::String`, y el emisor lo abre cuando lo necesita por primera vez, como el stream 0 (`cliente.rs:708-711`). sctp-proto crea los streams ordenados y confiables por defecto (`stream.rs:419-420`), lo que da 1.9, 2.1 y 2.2 sin código extra.
- *Descartada:* multiplexar en el stream 0 con un prefijo. **Motivo:** el parser de `OFFER ` vería el chat (riesgo para 2.5) y habría que tocar un camino que hoy funciona.
- *Descartada:* un stream no ordenado. **Motivo:** obligaría a reordenar en la aplicación para cumplir el orden por emisor.

### D2. Generación de llamada compartida (`Arc<AtomicU64>`)

`UnifiedApp` incrementa `call_generation` cada vez que entra a `VideoCall` (`unified_app.rs:336`, `:373`, `:441`). El contador llega a los hilos de red por `UnifiedAppConfig` y `MediaClientConfig`, junto a `running_call`.
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

Para testear el Hito 2 sin el binario:
- `route_incoming(stream_id, data, generation, max_bytes) -> Incoming`. `Incoming` es un enum: `Chat(SctpNotification)`, `FileControl(FileControl)`, `FileContent(&[u8])`, `Ignored`.
  - El parseo de `OFFER`, `ACCEPT` y `REJECT` pasa a una función pura `parse_file_control`, con el mismo comportamiento que hoy (`rsplit_once` para el tamaño).
  - Los efectos (mapas de pendientes, envío de la notificación, reensamblado del stream 1) siguen en el binario, que hace un `match` sobre `Incoming` (2.3, 2.4, 2.5).
  - Un payload de chat inválido da `Ignored` y se registra en el log.
- `send_chat(writer, cmd, current_generation) -> Result<(), ChatSendError>` sobre un trait mínimo `ChatWriter`, implementado para la asociación de sctp-proto. Los tests usan un writer que falla para 2.7, y el binario convierte el `Err` en `ChatSendFailed`.
- *Descartada:* dejar la lógica en el binario y testearla con `mod tests` dentro de `cliente.rs`. **Motivo:** los closures dependen de `DtlsStream` y de hilos, así que se terminaría testeando con red real, lo que excluye la DoD.

### D6. Integración en la UI

Mensajes nuevos de `AppMessage`: `ChatDraftChanged(String)`, `ChatSend` (botón y `on_submit` del `text_input`), `ChatReceived { generation, text }` y `ChatSendFailed { id, generation }`. `FrameUpdate` agrega dos ramas al `match` existente (`unified_app.rs:423`) y sigue con una notificación por tick (3.5).

El panel de chat se muestra junto al video con widgets `text`, que muestran texto literal sin interpretar marcado. Los mensajes propios no enviados llevan la etiqueta "no enviado".

**De dónde salen las condiciones de `prepare_send` (D4).** Cuando llega `ChatSend`, la app las calcula así:
- **Llamada activa:** la pantalla actual es `AppScreen::VideoCall`. No se usa `running_call`.
- **Canal listo:** `sctp_tx` está configurado (`Some`), como condición previa, y además `running_call` vale `true`. Si falta cualquiera de las dos, `prepare_send` devuelve `ChannelNotReady` y no sale ningún `SendChat`.

Hechos sobre `running_call` que justifican el criterio:
- **Hecho.** Solo la UI lo pone en `false`: en `EndCall` (`src/media/unified_app.rs:457`), en `RemoteCallEnded` (`:474`) y en `SystemClose` (`:528`). `src/bin/cliente.rs` y `src/client/transport/dtls_manager.rs` solo lo leen (por ejemplo el hilo SCTP TX en `cliente.rs:675`).
- **Hecho.** Lo ponen en `true` el hilo de DTLS, al terminar el handshake y crear el endpoint SCTP (`dtls_manager.rs:221`), y también la UI al entrar a `VideoCall`, antes del handshake: al aceptar (`unified_app.rs:336`), en `CallWasAccepted` (`:373`) y en `JoinCall` (`:441`).
- **Consecuencia.** `running_call` en `true` **no** garantiza que el handshake terminó ni que haya asociación SCTP. Dentro de `VideoCall` casi siempre vale `true`, así que en la práctica "canal listo" depende sobre todo de `sctp_tx`. Se mantiene igual como condición, porque es la misma señal que usa el hilo TX para descartar comandos (`cliente.rs:675-680`).
- **Hecho.** Si el hilo TX recibe un comando sin asociación, solo lo registra y lo descarta (`cliente.rs:854`). Para que un mensaje enviado antes del handshake no se pierda en silencio, la tarea 2.6 emite `ChatSendFailed` también en ese caso, y el mensaje queda "no enviado". Es el mismo requisito de reporte de fallo de envío de la spec, con su propio escenario (`chat_2_9`).
- *Descartada:* una señal nueva de "canal listo" activada en `Event::Connected` (`cliente.rs:531`). **Motivo:** agrega estado compartido y trabajo al Hito 2, y el caso sin asociación ya queda visible como "no enviado".

**Tests.** Para los tests del Hito 3 se agrega un helper que también devuelve el emisor de notificaciones, deja `running_call` en `true` y la pantalla en `VideoCall`. `make_app_with_channels` mantiene su firma, así que los tests existentes no cambian (3.9). `chat_3_10` cubre `running_call` en `false` dentro de `VideoCall`: `ChatSend` no emite `SendChat` y el historial no cambia.

## Supuestos

Los escenarios del Hito 2 en `specs/call-chat/spec.md` dependen de dos supuestos que no se verificaron en ejecución:
- **Supuesto:** dos asociaciones de sctp-proto se pueden conectar en memoria, intercambiando los paquetes de `poll_transmit` sin sockets ni DTLS (2.1, 2.2, 2.5, 2.6, 2.8). La tarea 2.1 lo confirma.
- **Supuesto:** el fallo de escritura de 2.7 se puede inyectar en `send_chat` mediante `ChatWriter`, sin un transporte real.

## Risks / Trade-offs

- **[Riesgo] Las asociaciones de sctp-proto en memoria (Background del Hito 2) podrían no conectarse sin un transporte real** (supuesto de la spec). → Mitigación: la primera tarea del Hito 2 es un harness de dos endpoints que intercambian `poll_transmit`. Si no funciona, se vuelve a explorar antes de seguir, sin bajar los criterios.
- **[Riesgo] Contención del lock de la asociación entre el hilo TX y el hilo RX durante una transferencia de 4 MB.** → El chat se escribe dentro de la iteración que ya existe (sin locks nuevos), y la escritura de sctp-proto solo encola. En el Hito 4 se mide la latencia percibida en la prueba manual.
- **[Trade-off] "No enviado" solo cubre los errores de `write_sctp`.** Un mensaje encolado que nunca llega no se detecta, porque el MVP no tiene confirmación de entrega (fuera de alcance).
- **[Trade-off] El contador de generación agrega un `Arc` a `UnifiedAppConfig` y `MediaClientConfig`.** → Es un cambio aditivo. Solo los constructores de configuración (`cliente.rs` y el helper de tests) lo pasan.
- **[Riesgo] Mover el parseo del stream 0 a `parse_file_control` podría cambiar el comportamiento de archivos.** → 2.3 y los tests existentes cubren la regresión. La función copia la lógica actual tal cual.

## Open Questions

- El ancho y la ubicación exactos del panel de chat dentro de la vista de la llamada. No cambian los requisitos ni las tareas, y se ajustan en la prueba manual del Hito 4.
