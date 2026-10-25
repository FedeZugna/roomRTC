# Tasks

Para que un test falle por la funcionalidad ausente y no por un error de compilación, cada hito arranca con un esqueleto compilable: las firmas de `design.md` con cuerpos neutros (`Ok(())`, `Vec::new()`, `Ignored`), nunca `todo!()` ni `panic!`. Después van los tests, que se ven fallar por aserción, y por último la implementación. Los nombres de los tests y sus escenarios están en `specs/call-chat/spec.md`.

## 1. Hito 1 — Modelo y reglas del mensaje (sin red ni UI)

- [ ] 1.1 Crear `src/chat.rs` con las constantes `CHAT_MAX_BYTES`, `CHAT_HISTORY_MAX` y `CHAT_STREAM_ID`, `ChatError`, `ChatState` con `Default` y las firmas de `validate_outgoing`, `encode`, `decode`, `push_own`, `push_peer`, `mark_failed` y la función de preparación de envío (design D4), con cuerpos neutros. Exportarlo en `src/lib.rs`. Verificar: `cargo build` compila.
- [ ] 1.2 Escribir en `src/chat.rs` (`mod tests`) los tests `chat_1_1` a `chat_1_12`, con límite de test 16 bytes y tope 3 inyectados. Verificar: `cargo test chat_1_` compila y los tests que dependen de la lógica fallan por aserción. Guardar la salida.
- [ ] 1.3 Implementar la validación y el codec (`validate_outgoing`, `encode`, `decode`) sin `unwrap`, `expect` ni `panic!`. Verificar: pasan `chat_1_2`, `chat_1_3`, `chat_1_4`, `chat_1_5` y `chat_1_6`.
- [ ] 1.4 Implementar `ChatState` (historial con tope, ids locales, `mark_failed`) y la preparación del envío con las comprobaciones de llamada activa y canal listo. Verificar: pasan `chat_1_1`, `chat_1_7`, `chat_1_8`, `chat_1_9`, `chat_1_10`, `chat_1_11` y `chat_1_12`.
- [ ] 1.5 Gates del Hito 1: `cargo test`, `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings`, comparados con `docs/baseline/`. Correr `/opsx:verify` sin CRITICAL. Mostrar la salida real.

## 2. Hito 2 — Transporte e interoperabilidad con archivos

- [ ] 2.1 Construir el harness de test con dos endpoints de sctp-proto conectados en memoria (intercambio de `poll_transmit` y `handle`, sin sockets ni DTLS). Verificar: un test auxiliar llega a `Event::Connected` en ambos lados. **Si no se puede, detenerse y volver a explorar** (riesgo de `design.md`).
- [ ] 2.2 Agregar las variantes `SctpCommand::SendChat { id, generation, text }`, `SctpNotification::ChatReceived { generation, text }` y `SctpNotification::ChatSendFailed { id, generation }` en `src/media/unified_app.rs`, con ramas que por ahora las ignoran en el `match` de `FrameUpdate`. Crear `src/client/transport/sctp_chat.rs` con las firmas de `route_incoming`, `parse_file_control`, `send_chat` y el trait `ChatWriter` (design D5), con cuerpos neutros. Verificar: `cargo build` compila y los tests existentes pasan.
- [ ] 2.3 Escribir los tests `chat_2_1` a `chat_2_8` usando el harness, un `ChatWriter` que falla para 2.7 y el archivo de 64 KiB con `i mod 251` para 2.6. Verificar: compilan y fallan por la funcionalidad ausente. Guardar la salida.
- [ ] 2.4 Implementar `parse_file_control` copiando el comportamiento actual del stream 0 (`src/bin/cliente.rs:424-520`), y `route_incoming` por `stream_id`. Verificar: pasan `chat_2_3`, `chat_2_4` y `chat_2_5`.
- [ ] 2.5 Implementar `send_chat` (stream 2, ordenado, abierto cuando hace falta, con descarte por generación o por llamada inactiva). Verificar: pasan `chat_2_1`, `chat_2_2`, `chat_2_6`, `chat_2_7` y `chat_2_8`.
- [ ] 2.6 Agregar `call_generation: Arc<AtomicU64>` a `UnifiedAppConfig` y `MediaClientConfig`, y conectarlo en `src/bin/cliente.rs`. Reemplazar el `if id == 1 / else if id == 0 / else` del hilo RX por un `match` sobre `route_incoming`, sin cambiar los efectos de archivos. En el hilo SCTP TX, procesar `SendChat` con `send_chat` y emitir `ChatSendFailed` ante un `Err`. Verificar: `cargo build`, todos los tests en verde y ningún cambio en `src/bin/server.rs` ni en `Message` (`git diff --stat`).
- [ ] 2.7 Gates del Hito 2: `cargo test`, `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings`, sin regresiones respecto de la línea base. Correr `/opsx:verify` sin CRITICAL. Mostrar la salida real.

## 3. Hito 3 — Integración con la app (iced)

- [ ] 3.1 Agregar en `UnifiedApp` el campo `chat: ChatState`, el uso de `call_generation` y los mensajes `ChatDraftChanged`, `ChatSend`, `ChatReceived` y `ChatSendFailed` (design D3, D6), con handlers vacíos. Agregar un helper de test que devuelva también el emisor de notificaciones, sin cambiar la firma de `make_app_with_channels`. Verificar: `cargo build` compila y los tests existentes de `unified_app` pasan.
- [ ] 3.2 Escribir los tests `chat_3_1` a `chat_3_9` en `src/media/unified_app.rs` (`mod tests`), con los límites de producción y los ticks de `FrameUpdate` enviados a mano. Verificar: compilan y fallan por la funcionalidad ausente, salvo la parte de regresión de `chat_3_9`. Guardar la salida.
- [ ] 3.3 Implementar el borrador y el envío: actualización del borrador, envío con la preparación del Hito 1, emisión de `SendChat` con la generación actual y borrador vacío al enviar. Verificar: pasan `chat_3_1`, `chat_3_2` y `chat_3_3`.
- [ ] 3.4 Implementar la recepción: ramas de chat en `FrameUpdate` (una notificación por tick), descarte por generación distinta y `mark_failed` para `ChatSendFailed`. Verificar: pasan `chat_3_4`, `chat_3_5`, `chat_3_6` y `chat_3_8`.
- [ ] 3.5 Incrementar la generación y reiniciar `ChatState` al entrar a `VideoCall` (`unified_app.rs:336`, `:373`, `:441`), y en `EndCall` y `RemoteCallEnded`. Verificar: pasa `chat_3_7` y `chat_1_11` sigue pasando.
- [ ] 3.6 Agregar el panel de chat a la vista de `VideoCall`: lista con widgets `text` literales, etiqueta "no enviado", `text_input` con `on_submit` y botón Enviar. Verificar: pasan `chat_3_2`, `chat_3_4` y `view_and_subscription_smoke_for_each_screen`.
- [ ] 3.7 Verificar la regresión: pasan `chat_3_9`, `send_file_sends_sctp_command_with_current_path`, `end_call_sends_end_call_clears_frames_and_returns_to_lobby`, `remote_call_ended_clears_frames_and_returns_to_lobby_without_sending_endcall` y `view_and_subscription_smoke_for_each_screen`, sin cambios en esos tests (`git diff` de sus cuerpos vacío).
- [ ] 3.8 Gates del Hito 3 y de la feature: `cargo test chat_` (los 29 tests, de `chat_1_1` a `chat_3_9`), `cargo test`, `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings`, sin regresiones. Verificar con grep que el código nuevo del chat no tiene `unwrap`, `expect` ni `panic!` y que ningún test `chat_` usa red real, cámara ni `.config`. Correr `/opsx:verify` sin CRITICAL. Mostrar la salida real.

## Workflow follow-up

- Especificar el Hito 4 (integración entre dos `UnifiedApp`, chat con el modal de oferta visible y prueba manual con dos procesos reales) al cerrar el Hito 3, actualizando esta spec. La prueba manual reinicia ambos clientes entre llamadas (`docs/chat/exploracion.md`, 5.6).
- Archivar el change con `/opsx:archive` cuando se cumplan todos los gates y quede registrada la prueba manual.
