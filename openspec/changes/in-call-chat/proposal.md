# Proposal

## Why

Hoy, durante una videollamada, los dos participantes solo pueden comunicarse por audio, video y transferencia de archivos. Falta un canal de texto para intercambiar mensajes cortos (un enlace, un dato, algo que no se escuchó) sin cortar la llamada. El canal SCTP sobre DTLS ya existe y tiene streams libres, así que se puede agregar el chat sin tocar la señalización.

## What Changes

- Nuevo chat de texto plano entre los dos participantes, disponible solo mientras la llamada está activa. El alcance (MVP) y las reglas están en [`docs/chat/roomRTC_todo.md`](../../../docs/chat/roomRTC_todo.md).
- Nuevo módulo de chat con validación del mensaje saliente, codificación y decodificación UTF-8, historial con tope y error tipado. Los límites (4096 bytes y 200 mensajes) son parámetros, con constantes de producción.
- El chat viaja por un stream SCTP dedicado, separado de los streams de archivos. Las decisiones de transporte, los nombres y el manejo del cambio de llamada se fijaron en [`docs/chat/exploracion.md`](../../../docs/chat/exploracion.md).
- Nuevas variantes en `SctpCommand` (envío de chat) y en `SctpNotification` (chat recibido y fallo de envío). Son aditivas: las variantes existentes no cambian.
- La pantalla de videollamada muestra un panel de chat (historial, campo de texto, botón Enviar y Enter). Al terminar la llamada se descartan historial, borrador y mensajes pendientes.
- Este change cubre los Hitos 1 a 3 de [`docs/chat/test_aceptacion.md`](../../../docs/chat/test_aceptacion.md). Cada criterio de esos hitos tiene su test `chat_<hito>_<n>_...`. El Hito 4 se especifica al cerrar el Hito 3.

### Fuera de alcance

- Lo que lista la sección "Fuera de alcance" de `docs/chat/roomRTC_todo.md`: chat grupal, persistencia, mensajes offline, búsqueda, edición o borrado, adjuntos desde el chat, confirmación de entrega o lectura, indicador "escribiendo...", relay por el servidor y cifrado adicional.
- Cambios en `src/bin/server.rs` y en el enum `Message` de señalización.
- Arreglar la renegociación de DTLS y SCTP en una segunda llamada dentro del mismo proceso. Es una limitación previa al chat (ver `docs/chat/exploracion.md`, puntos 3 y 5.6). La limpieza del chat entre llamadas se verifica a nivel de modelo y de app.
- El Hito 4 (integración entre dos apps y prueba manual), que se especifica en un paso posterior.

## Capabilities

### New Capabilities

- `call-chat`: mensajería de texto entre los dos participantes de una videollamada activa. Cubre las reglas del mensaje, el historial, el transporte por un canal propio aislado de la transferencia de archivos, el reporte de fallos de envío y el ciclo de vida atado a la llamada.

### Modified Capabilities

(ninguna; no hay specs existentes y la transferencia de archivos no cambia de comportamiento)

## Impact

- **Código nuevo:** `src/chat.rs` (exportado desde `src/lib.rs`).
- **Código modificado:**
  - `src/media/unified_app.rs`: variantes de `SctpCommand` y `SctpNotification`, mensajes de la app, estado del chat en `AppScreen::VideoCall`, la vista y el consumo en `FrameUpdate`.
  - `src/bin/cliente.rs`: se extraen funciones puras de `run_media_client` para la recepción y el envío, y se agrega la rama del stream de chat en los hilos SCTP RX y TX.
- **Sin cambios:** `src/bin/server.rs`, el protocolo de señalización, las dependencias de `Cargo.toml` y el formato de los streams de archivos.
- **Tests:** tests nuevos `chat_1_*` a `chat_3_*`. Los tests existentes de `unified_app` y `tests/*.rs` no cambian.
