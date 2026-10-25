# TODO: Chat durante la videollamada


## Nueva feature

Feature para sistema de videollamadas: Si se inicia una nueva videollamada, está la posibilidad de chatear con el otro participante.

Feature: durante una videollamada entre dos clientes, cada usuario puede escribir mensajes de texto y verlos junto al video; el otro usuario los recibe en tiempo real. Al cortar la llamada el chat desaparece.

## Alcance (MVP)

Solo texto plano, entre los dos participantes de una llamada activa. El historial vive en memoria, solo durante la llamada.

| Caso             | Cuándo ocurre                                       | Cómo viaja                                                           | Qué ve el usuario                                                |
| ---------------- | --------------------------------------------------- | -------------------------------------------------------------------- | ---------------------------------------------------------------- |
| Mensaje saliente | El usuario envía un texto válido durante la llamada | Un mensaje SCTP por el canal de chat  | Su mensaje en el historial                       |
| Mensaje entrante | El participante envía un texto                               | Mismo canal                                                          | El mensaje del participante en el historial                               |
| Fallo de envío   | El transporte no puede escribir el mensaje          | No llega al participante                                                      | El mensaje propio marcado como "no enviado"                      |
| Fin de llamada   | Cualquiera de los dos corta              | Sin viaje                                                          | Historial y borrador vacíos. La siguiente llamada arranca limpia |

Items:

- [ ] Módulo de chat con: validación de mensaje saliente, codificación/decodificación UTF-8 y error tipado.
- [ ] Validación con el límite y el tope de historial como parámetros (los tests inyectan valores chicos), y constantes públicas con los valores de producción (4096 bytes y 200 mensajes).
- [ ] Extraer de `run_media_client` (`src/bin/cliente.rs`) funciones puras para procesar un payload recibido y para preparar un envío, de modo que se puedan testear sin hilos. La decodificación también recibe el límite como parámetro.
- [ ] Nuevos `SctpCommand::SendChat` y `SctpNotification` de chat recibido y de fallo de envío, con identificador local del mensaje.
- [ ] Estado del chat en la app (historial + borrador), agrupado en un tipo con `Default` para no tocar cada literal de `AppScreen::VideoCall { .. }`.
- [ ] Mensajes de `iced` para borrador, envío y recepción, y la vista del panel de chat (lista, campo de texto, botón Enviar y Enter).


Reglas a especificar:

- [ ] Se rechaza el mensaje vacío o de solo espacios. El texto válido se envía tal cual, sin recortar.
- [ ] Máximo 4096 bytes UTF-8 por mensaje, medido en bytes, no en caracteres.
- [ ] El receptor no confía en el emisor: bytes inválidos, vacíos o sobre el límite se descartan sin romper la app.
- [ ] El chat viaja por un stream SCTP dedicado y no interfiere con el control de archivos ni con el contenido de archivos. Un chat con el texto `OFFER ...` no dispara una oferta de archivo.
- [ ] El texto se muestra tal cual, sin interpretarlo como marcado y se conserva al codificar y decodificar.
- [ ] Orden preservado por emisor, sin duplicados ni pérdidas, incluso con ráfagas.
- [ ] Historial con tope de 200 mensajes, si se supera el tope, se descarta el mensaje más viejo.
- [ ] Solo se puede enviar durante una llamada activa; lo que llega fuera de llamada se ignora.
- [ ] Si el envío falla, el mensaje propio queda marcado "no enviado" y los demás no se pierden. Se identifica por id local, no por texto.
- [ ] Al terminar la llamada se borran historial y borrador. Los mensajes que quedaron en la cola no aparecen ni se envían en la llamada siguiente, se descartan.
- [ ] El chat no afecta el modal de oferta de archivos ni la transferencia en curso.
- [ ] Sin cambios en `src/bin/server.rs` ni en el enum `Message` de señalización.


## Pruebas de aceptación

Son el criterio de terminado. Los escenarios están en `test_aceptacion.md` y cada uno tiene su test con el nombre `chat_<hito>_<n>_<descripcion>` (por ejemplo `chat_1_3_rechaza_mensaje_sobre_el_limite`). La feature está terminada cuando pasan todos:

```bash
cargo test chat_
cargo test
cargo fmt --all -- --check
cargo clippy --all-targets
```


## Fuera de alcance

- Chat grupal, persistencia del historial, mensajes offline, búsqueda, edición o borrado.
- Adjuntos desde el chat (los archivos siguen con su propio flujo).
- Confirmación de entrega o lectura, e indicador "escribiendo...". El MVP solo distingue "se pudo escribir" de "no se pudo".
- Relay por el servidor de señalización y cualquier cambio de protocolo de señalización.
- Cifrado adicional: el canal ya va sobre DTLS.

## Antes de empezar

- [ ] Línea base: `cargo build`, `cargo test`, `cargo fmt --all -- --check`, `cargo clippy --all-targets`. Registrar resultados reales, dependencias del sistema (GStreamer/GTK) y fallas preexistentes.
- [ ] Mapa de streams SCTP: IDs, quién los abre, productores y consumidores, locks.
- [ ] Ciclo de vida: si la asociación SCTP y el receptor `sctp_notif_rx` son por llamada o por proceso, y una notificación vieja no puede aparecer en la llamada siguiente.
- [ ] Verificar que `FrameUpdate` consume una sola notificación por tick.
- [ ] Transporte: stream SCTP dedicado.
- [ ] Límites: 4096 bytes y 200 mensajes(se confirman o se ajustan acá mismo, revisando el tamaño máximo de mensaje de sctp-proto). El límite debe ser inyectable también al decodificar, y los nombres de tipos, módulo y stream.
