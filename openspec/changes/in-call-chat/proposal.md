# Proposal

## Why

Hoy, durante una videollamada, los dos participantes solo pueden comunicarse por audio, video y transferencia de archivos. Falta un canal de texto para intercambiar mensajes cortos (un enlace, un dato, algo que no se escuchó) sin cortar la llamada. El canal SCTP sobre DTLS ya existe y tiene streams libres, así que se puede agregar el chat sin tocar la señalización.

## What Changes

- Nuevo chat de texto plano entre los dos participantes, disponible solo mientras la llamada está activa. El alcance (MVP) y las reglas están en [`docs/chat/roomRTC_todo.md`](../../../docs/chat/roomRTC_todo.md).
- Nuevo módulo de chat con validación del mensaje saliente, codificación y decodificación UTF-8, historial con tope y error tipado. Los límites (4096 bytes y 200 mensajes) son parámetros, con constantes de producción.
- El chat viaja por un stream SCTP dedicado, separado de los streams de archivos. Las decisiones de transporte, los nombres y el manejo del cambio de llamada se fijaron en [`docs/chat/exploracion.md`](../../../docs/chat/exploracion.md).
- Nuevas variantes en `SctpCommand` (envío de chat) y en `SctpNotification` (chat recibido y fallo de envío). Son aditivas: las variantes existentes no cambian.
- La pantalla de videollamada muestra un panel de chat (historial, campo de texto, botón Enviar y Enter). Al terminar la llamada se descartan historial, borrador y mensajes pendientes.
- Este change cubre los Hitos 1 a 4 de [`docs/chat/test_aceptacion.md`](../../../docs/chat/test_aceptacion.md). Cada criterio automático tiene su test `chat_<hito>_<n>_...`.
- El Hito 4 suma dos partes:
  - **Automática:** dos instancias de la app intercambian mensajes por el transporte en memoria, también con una oferta de archivo pendiente.
  - **Manual:** con dos procesos reales (pasos M1 a M6). Se registra en [`docs/chat/evidencia/prueba_manual.md`](../../../docs/chat/evidencia/prueba_manual.md).

### Fuera de alcance

- Lo que lista la sección "Fuera de alcance" de `docs/chat/roomRTC_todo.md`: chat grupal, persistencia, mensajes offline, búsqueda, edición o borrado, adjuntos desde el chat, confirmación de entrega o lectura, indicador "escribiendo...", relay por el servidor y cifrado adicional.
- Cambios en `src/bin/server.rs` y en el enum `Message` de señalización.
- Arreglar la renegociación de DTLS y SCTP en una segunda llamada dentro del mismo proceso. Es una limitación previa al chat (ver `docs/chat/exploracion.md`, puntos 3 y 5.6). La limpieza del chat entre llamadas se verifica a nivel de modelo y de app.
- Testear automáticamente los hilos RX y SCTP TX de `cliente.rs`, que dependen de sockets, DTLS y la cámara. Los tests usan las mismas funciones que esos hilos, y el código que las une dentro de ellos se cubre con la prueba manual del Hito 4 (design D7 y D8).
- Medir con precisión la latencia del chat. La prueba manual registra la latencia percibida, con los límites de esa medición.

## Capabilities

### New Capabilities

- `call-chat`: mensajería de texto entre los dos participantes de una videollamada activa. Cubre las reglas del mensaje, el historial, el transporte por un canal propio aislado de la transferencia de archivos, el reporte de fallos de envío y el ciclo de vida atado a la llamada.

### Modified Capabilities

(ninguna; no hay specs existentes y la transferencia de archivos no cambia de comportamiento)

## Impact

- **Código nuevo:**
  - `src/chat.rs` (exportado desde `src/lib.rs`): el modelo y las reglas del mensaje.
  - `src/client/transport/sctp_chat.rs`: el enrutamiento de lo recibido y el envío de chat, como funciones puras.
- **Código nuevo, solo de test:**
  - `src/client/transport/sctp_harness.rs`: dos extremos SCTP en memoria.
  - `src/media/unified_app_e2e_tests.rs` (Hito 4): el puente entre dos apps.
- **Código modificado:**
  - `src/media/unified_app.rs`:
    - las variantes de `SctpCommand` y `SctpNotification`, y los mensajes de la app;
    - el estado del chat en `UnifiedApp` (design D3);
    - la generación de llamada en `UnifiedAppConfig`;
    - el panel en la vista y el consumo en `FrameUpdate`.
  - `src/bin/cliente.rs`: los hilos SCTP RX y TX usan las funciones de `sctp_chat.rs` (un `match` sobre lo recibido, y el envío de chat con su reporte de fallos). También crea la generación de llamada.
- **Sin cambios:** `src/bin/server.rs`, el protocolo de señalización, las dependencias de `Cargo.toml` y el formato de los streams de archivos.
- **Tests:** tests nuevos `chat_1_*` a `chat_4_*` (34 en total) y el test del harness. Los tests existentes de `unified_app` y `tests/*.rs` no cambian.
- **Documentación:** `docs/chat/exploracion.md` (Hito 0) y las salidas de RED y de los gates de cada hito en `docs/chat/evidencia/`.
