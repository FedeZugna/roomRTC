# Tasks

Para que un test falle por la funcionalidad ausente y no por un error de compilación, cada hito arranca con un esqueleto compilable: las firmas de `design.md` con cuerpos neutros (`Ok(())`, `Vec::new()`, `Ignored`), nunca `todo!()` ni `panic!`. Después van los tests, que se ven fallar por aserción, y por último la implementación. Los nombres de los tests y sus escenarios están en `specs/call-chat/spec.md`.

## 1. Hito 1 — Modelo y reglas del mensaje (sin red ni UI)

- [x] 1.1 Crear `src/chat.rs` con las constantes `CHAT_MAX_BYTES`, `CHAT_HISTORY_MAX` y `CHAT_STREAM_ID`, `ChatError`, `ChatState` con `Default` y las firmas de `validate_outgoing`, `encode`, `decode`, `push_own`, `push_peer`, `mark_failed` y la función de preparación de envío (design D4), con cuerpos neutros. Exportarlo en `src/lib.rs`. Verificar: `cargo build` compila.
- [x] 1.2 Escribir en `src/chat.rs` (`mod tests`) los tests `chat_1_1` a `chat_1_12`, con límite de test 16 bytes y tope 3 inyectados. Verificar: `cargo test chat_1_` compila y los tests que dependen de la lógica fallan por aserción. Guardar la salida.
- [x] 1.3 Implementar la validación y el codec (`validate_outgoing`, `encode`, `decode`) sin `unwrap`, `expect` ni `panic!`. Verificar: pasan `chat_1_2`, `chat_1_3`, `chat_1_4`, `chat_1_5` y `chat_1_6`.
- [x] 1.4 Implementar `ChatState` (historial con tope, ids locales, `mark_failed`) y la preparación del envío con las comprobaciones de llamada activa y canal listo. Verificar: pasan `chat_1_1`, `chat_1_7`, `chat_1_8`, `chat_1_9`, `chat_1_10`, `chat_1_11` y `chat_1_12`.
- [x] 1.5 Gates del Hito 1: `cargo test`, `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings`, comparados con `docs/baseline/`. Correr `/opsx:verify` sin CRITICAL. Mostrar la salida real.

## 2. Hito 2 — Transporte e interoperabilidad con archivos

- [x] 2.1 Construir el harness de test con dos endpoints de sctp-proto conectados en memoria (intercambio de `poll_transmit` y `handle`, sin sockets ni DTLS). Verificar: un test auxiliar llega a `Event::Connected` en ambos lados. **Si no se puede, detenerse y volver a explorar** (riesgo de `design.md`).
- [x] 2.2 Agregar las variantes `SctpCommand::SendChat { id, generation, text }`, `SctpNotification::ChatReceived { generation, text }` y `SctpNotification::ChatSendFailed { id, generation }` en `src/media/unified_app.rs`, con ramas que por ahora las ignoran en el `match` de `FrameUpdate`. Crear `src/client/transport/sctp_chat.rs` con las firmas de `route_incoming`, `parse_file_control`, `send_chat`, el trait `ChatWriter` (design D5) y la función que resuelve un `SendChat` sin asociación (design D6, 2.9), con cuerpos neutros. Verificar: `cargo build` compila y los tests existentes pasan.
- [x] 2.3 Escribir los tests `chat_2_1` a `chat_2_9` usando el harness, un `ChatWriter` que falla para 2.7, el archivo de 64 KiB con `i mod 251` para 2.6 y, para 2.9, la función pura de `sctp_chat.rs` que resuelve un `SendChat` sin asociación (con firma y cuerpo neutro creados en 2.2). Verificar: compilan y fallan por la funcionalidad ausente. Guardar la salida.
- [x] 2.4 Implementar `parse_file_control` copiando el comportamiento del stream 0 (`src/bin/cliente.rs:424-520` antes del Hito 2), y `route_incoming` por `stream_id`. Verificar: pasan `chat_2_3`, `chat_2_4` y `chat_2_5`. **Solapamiento con 2.5:** `chat_2_5` envía el chat por el harness, así que depende también de `send_chat` (2.5). Al terminar 2.4 pasaban `chat_2_3` y `chat_2_4`. `chat_2_5` pasó recién con 2.5, y 2.4 se marcó entonces.
- [x] 2.5 Implementar `send_chat` (stream 2, ordenado, abierto cuando hace falta). Recibe el `id` local y separa `Ok(Discarded)` (de otra generación o sin llamada activa: no se escribe ni se reporta) de `Err(ChatSendError { id, generation, reason })` (falló la escritura: se reporta) (design D5). Verificar: pasan `chat_2_1`, `chat_2_2`, `chat_2_6`, `chat_2_7` y `chat_2_8`.
- [x] 2.6 Crear `call_generation: Arc<AtomicU64>` en `run_media_client`, junto a `running_call`, pasarlo a los hilos RX y SCTP TX, y a la UI por `UnifiedAppConfig`. No va en `MediaClientConfig` (design D2). Reemplazar el `if id == 1 / else if id == 0 / else` del hilo RX por un `match` sobre `route_incoming`, sin cambiar los efectos de archivos. En el hilo SCTP TX, procesar `SendChat` con `send_chat` y emitir `ChatSendFailed` ante un `Err`. También emitirlo cuando un `SendChat` llega sin asociación SCTP (rama sin asociación del hilo SCTP TX, `src/bin/cliente.rs:854` antes del Hito 2; design D6), resolviéndolo con `notify_without_association` de `sctp_chat.rs`, que cubre `chat_2_9`. Verificar: pasa `chat_2_9`, `cargo build`, todos los tests en verde y ningún cambio en `src/bin/server.rs` ni en `Message` (`git diff --stat`).
- [x] 2.7 Gates del Hito 2: `cargo test`, `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings`, sin regresiones respecto de la línea base. Correr `/opsx:verify` sin CRITICAL. Mostrar la salida real.

## 3. Hito 3 — Integración con la app (iced)

- [x] 3.1 Agregar en `UnifiedApp` el campo `chat: ChatState`, el uso de `call_generation` y los mensajes `ChatDraftChanged`, `ChatSend`, `ChatReceived` y `ChatSendFailed` (design D3, D6), con handlers vacíos. Agregar un helper de test que devuelva también el emisor de notificaciones, sin cambiar la firma de `make_app_with_channels`. Verificar: `cargo build` compila y los tests existentes de `unified_app` pasan.
- [x] 3.2 Escribir los tests `chat_3_1` a `chat_3_10` en `src/media/unified_app.rs` (`mod tests`), con los límites de producción y los ticks de `FrameUpdate` enviados a mano. Verificar: compilan y fallan por la funcionalidad ausente, salvo la parte de regresión de `chat_3_9`. Guardar la salida.
- [x] 3.3 Implementar el borrador y el envío: actualización del borrador, envío con `prepare_send` del Hito 1 y conversión del `OutgoingChat { id, text }` resultante en `SctpCommand::SendChat { id, generation, text }` con la generación actual (design D4), y borrador vacío al enviar. Si `prepare_send` devuelve `Err`, no se emite nada. Las condiciones salen como dice design D6: "llamada activa" es que la pantalla sea `VideoCall` (no `running_call`), y "canal listo" es `sctp_tx` configurado más `running_call` en `true`. El helper de test nuevo deja `running_call` en `true`, y `chat_3_10` lo pone en `false`. Verificar: pasan `chat_3_1`, `chat_3_2`, `chat_3_3` y `chat_3_10`.
- [x] 3.4 Implementar la recepción: ramas de chat en `FrameUpdate` (una notificación por tick), descarte por generación distinta y `mark_failed` para `ChatSendFailed`. Verificar: pasan `chat_3_4`, `chat_3_5`, `chat_3_6` y `chat_3_8`. **Solapamiento con 3.5 y 3.6:** `chat_3_8` necesita que la generación se incremente al entrar a una llamada (3.5), y la parte de `chat_3_6` que comprueba que se *muestra* "no enviado" necesita `chat_line` (3.6). Al terminar 3.4 pasaban `chat_3_4` y `chat_3_5`. `chat_3_6` y `chat_3_8` pasaron recién con 3.5 y 3.6, y 3.4 se marcó entonces.
- [x] 3.5 Incrementar la generación y reiniciar el chat con `ChatState::reset()` al entrar a `VideoCall` (`unified_app.rs:368`, `:405`, `:476` al cierre del Hito 2), y en `EndCall` y `RemoteCallEnded`. Verificar: pasa `chat_3_7` y `chat_1_11` sigue pasando.
- [x] 3.6 Agregar el panel de chat a la vista de `VideoCall`: lista con widgets `text` literales, etiqueta "no enviado", `text_input` con `on_submit` y botón Enviar. Verificar: pasan `chat_3_2`, `chat_3_4` y `view_and_subscription_smoke_for_each_screen`.
- [x] 3.7 Verificar la regresión: pasan `chat_3_9`, `send_file_sends_sctp_command_with_current_path`, `end_call_sends_end_call_clears_frames_and_returns_to_lobby`, `remote_call_ended_clears_frames_and_returns_to_lobby_without_sending_endcall` y `view_and_subscription_smoke_for_each_screen`, sin cambios en esos tests (`git diff` de sus cuerpos vacío).
- [x] 3.8 Gates del Hito 3 y de la feature: `cargo test chat_` (los 33 tests, de `chat_1_1` a `chat_3_12`), `cargo test`, `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings`, sin regresiones. Verificar con grep que el código nuevo del chat no tiene `unwrap`, `expect` ni `panic!` y que ningún test `chat_` usa red real, cámara ni `.config`. Correr `/opsx:verify` sin CRITICAL. Mostrar la salida real.
- [x] 3.9 Criterio 3.11 (agregado después de cerrar 3.8): escribir `chat_3_11` en `src/media/unified_app.rs`, con el receptor de comandos cerrado, y verlo fallar por aserción con el comportamiento desactivado (salida en `docs/chat/evidencia/`). Después restaurar en `ChatSend` el `mark_failed(id)` que se aplica cuando `sctp_tx.send` falla (design D6). Verificar: pasa `chat_3_11` y se vuelven a correr los gates de 3.8, con 32 tests `chat_`.
- [x] 3.10 Criterio 3.12 (pedido tras la prueba manual):
  - Agregar a `UnifiedApp` los campos `peer_username` y `pending_callee`, y el método `chat_line(&self, entry)`, que pasa los nombres a la función `chat_line` (design D6). Primero con las etiquetas de siempre.
  - Escribir `chat_3_12` y verlo fallar por aserción (salida en `docs/chat/evidencia/`).
  - Después, guardar el callee en `CallUserByClick`, el par al entrar a `VideoCall` (`AcceptCall`, `CallWasAccepted`), y limpiarlo al terminar la llamada y en `CallRejected`.
  - Agrandar el panel y el `text_input`, como dice D6.
  - En `chat_3_6` y `chat_3_11` solo cambia la línea que arma `lines`, que pasa a usar el método. Sus aserciones no cambian.

  Verificar: pasa `chat_3_12`, y se vuelven a correr los gates de 3.8 con 33 tests `chat_`.

## 4. Hito 4 — Integración y prueba manual

- [x] 4.1 Crear `src/media/unified_app_e2e_tests.rs`, declarado en `unified_app.rs` como módulo hijo `#[cfg(test)]` con `#[path]` (design D7). Pasar a `pub(super)` los helpers `make_app_with_all_channels` y `force_video` de `mod tests`, sin cambiar su comportamiento. Escribir el `ChatBridge` de test sobre `SctpPair` con un `pump` neutro, que no mueve nada, y el helper `run_task`, que ejecuta los `Task` de cada tick como el runtime de iced (`iced_runtime` como `dev-dependency`, design D7). Verificar: `cargo test` compila, los tests existentes pasan y los cuerpos de los tests existentes no cambian.
- [x] 4.2 Escribir los tests `chat_4_1` y `chat_4_2` (`docs/chat/test_aceptacion.md`, Hito 4) usando `ChatBridge`, el helper del Hito 3 y ticks de `FrameUpdate` enviados a mano. Verificar: compilan y fallan por aserción porque el puente todavía no mueve nada. Guardar la salida en `docs/chat/evidencia/`.
- [x] 4.3 Implementar `ChatBridge::pump` como en design D7:
  - Lado TX: `SendChat` con `send_chat`, con la generación y el `running_call` de cada app, y un `Err` entregado como `ChatSendFailed` a la propia app.
  - Mensajes de control en el stream 0.
  - Lado RX: `route_incoming` con la generación de la app receptora, entregando el chat y la `FileOffer`.

  Verificar: pasan `chat_4_1` y `chat_4_2`.
- [x] 4.4 Gates automáticos del Hito 4: `cargo test chat_` (los 35 tests, de `chat_1_1` a `chat_4_2`), `cargo test`, `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings`, sin regresiones. Verificar con grep que el código nuevo no tiene `unwrap`, `expect` ni `panic!` y que ningún test `chat_` usa red real, cámara ni `.config`. Mostrar la salida real.
- [ ] 4.5 Ejecutar la prueba manual M1 a M6 con dos procesos reales (design D8), completando `docs/chat/evidencia/prueba_manual.md`:
  - el resultado de cada paso;
  - el `shasum -a 256` del original y del recibido en M2;
  - la medición de latencia, con método y límites;
  - las limitaciones de la Mac;
  - el hallazgo de M5.

  Verificar: ningún paso de la plantilla queda sin resultado ("pasó", "falló" o "no se pudo ejecutar", con el motivo).
- [ ] 4.6 Verificación final del Hito 4:
  - Registrar el hallazgo de M5 en `docs/chat/exploracion.md` (punto 3) como hecho o hipótesis refutada.
  - Si M6 cambia las etiquetas o el ancho del panel, actualizar primero design D6 y la spec, y después el código y los tests.
  - Si algún paso de M1 a M4 falla por el chat, abrir la corrección con su criterio y su test antes de seguir.

  Verificar: `/opsx:verify` sin CRITICAL y `openspec validate in-call-chat --strict` en verde.

## Workflow follow-up

- Archivar el change con `/opsx:archive` cuando se cumplan todos los gates y quede registrada la prueba manual.
