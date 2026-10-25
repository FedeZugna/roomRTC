## Roadmap y tests de aceptación

Cada hito tiene un **Background** y criterios Given/When/Then. Cada criterio `N.M` tiene un test en Rust nombrado `chat_<N>_<M>_<descripcion>` (por ejemplo `chat_1_3_rechaza_mensaje_sobre_el_limite`). 

**Para avanzar de hito:** (a) todos los tests del hito existen, se vio que fallaban antes de implementar y ahora pasan, (b) `cargo test` en verde sin regresiones respecto de la línea base, (c) `cargo fmt --all -- --check` y `cargo clippy --all-targets -- -D warnings` sin advertencias nuevas, (d) `/opsx:verify` sin CRITICAL. 


### Hito 1 — Modelo y reglas del mensaje (sin red ni UI)

**Background**
```yaml
limite_de_test: 16 bytes        # el test pasa el límite explícito al validador y al decodificador
tope_de_historial_de_test: 3    # idem
produccion:                     # solo se verifica en 1.12
  CHAT_MAX_BYTES: 4096
  CHAT_HISTORY_MAX: 200
sesion: activa, canal de envío listo   # salvo que el criterio diga otra cosa
usuarios: alice (local), bob (par)
```

| #    | Given / When / Then |
|------|---------------------|
| 1.1  | Dado el texto `Hola`, cuando Alice lo envía, entonces se produce un mensaje con contenido exacto `Hola` y el historial de Alice tiene una entrada propia `Hola` |
| 1.2  | Dado el texto vacío, `"   "`, cuando Alice intenta enviarlo, entonces se rechaza con un error identificable, no se genera ningún comando de envío y el historial no cambia |
| 1.3  | Dado un texto de exactamente 16 bytes (`aaaaaaaaaaaaaaaa`), cuando se valida, entonces se acepta. Con 17 bytes se rechaza con un error identificable |
| 1.4  | Dado el texto `"  hola  "`, cuando Alice lo envía, entonces el contenido del mensaje es exactamente `"  hola  "`|
| 1.5  | Dado `¿Qué tal?` (11 bytes), cuando se codifica y se decodifica, entonces el resultado es igual a su original |
| 1.6  | Dados los payloads recibidos `[0xFF, 0xFE]` (UTF-8 inválido), vacío y de 17 bytes, cuando se decodifican, entonces cada uno falla con un error identificable y sin panic |
| 1.7  | Dada una sesión inactiva o terminada, o un canal de envío no listo, cuando Alice intenta enviar `Hola`, entonces se rechaza, no se genera ningún comando de envío y el historial no cambia |
| 1.8  | Dado que Alice envió dos mensajes con el mismo texto `ok` (ids locales 1 y 2), cuando llega un error de envío para el id 2, entonces solo el mensaje 2 queda "no enviado", el 1 queda igual y ambos siguen en el historial |
| 1.9  | Dado que Alice recibe `uno`, `dos` y `tres` de Bob, entonces su historial tiene las tres entradas del par, una sola vez cada una y en ese orden |
| 1.10 | Dado un historial con 3 entradas (`a`, `b`, `c`), cuando llega `d`, entonces sigue habiendo 3 entradas: `b`, `c`, `d` |
| 1.11 | Dado un historial con mensajes y el borrador `hola`, cuando termina la sesión y empieza una nueva, entonces la nueva tiene historial y borrador vacíos |
| 1.12 | Dados los valores de producción, entonces `CHAT_MAX_BYTES` vale 4096 y `CHAT_HISTORY_MAX` vale 200 |

### Hito 2 — Transporte e interoperabilidad con archivos

**Background**
```yaml
extremos: A (alice) y B (bob), con asociaciones sctp-proto conectadas en memoria (sin sockets)
stream_de_chat: el definido en la exploracion previa, leído de su constante
archivo_de_prueba: 64 KiB, byte i = i mod 251
fallos_inyectables: error_al_escribir
mensajes: 001, 002, 003
```

| #   | Given / When / Then |
|-----|---------------------|
| 2.1 | Dados A y B con el canal listo, cuando A envía `001`, entonces B recibe exactamente una notificación de chat con el contenido `001` |
| 2.2 | Dado que A envía `001`, `002` y `003`, entonces B los recibe en ese orden, una sola vez cada uno |
| 2.3 | Regresión: dado `OFFER foto.png 100` en el stream de control de archivos (el mensaje que hoy usa la app para ofrecer un archivo de 100 bytes), cuando B lo procesa, entonces se genera la notificación de oferta de archivo de siempre y ninguna de chat |
| 2.4 | Regresión: dado un payload que parece texto de chat en el stream de contenido de archivos, cuando B lo procesa, entonces no se genera ninguna notificación de chat |
| 2.5 | Dado que A envía por chat el texto `OFFER foto.png 100`, entonces B recibe una notificación de chat con ese texto y no se genera ninguna oferta de archivo |
| 2.6 | Dada una transferencia del archivo de prueba de A a B en curso, cuando A y B intercambian 20 mensajes de chat, entonces B recibe los 20 de A en orden y el archivo recibido es idéntico byte a byte al original |
| 2.7 | Dado un fallo inyectado al escribir, cuando A envía `001` (id local 1), entonces se emite un error asociado al id 1 y el cliente sigue operativo |
| 2.8 | Dados 2 comandos de envío de chat pendientes en cola, cuando termina la llamada y empieza otra, entonces ninguno se transmite en la llamada nueva |
| 2.9 | Dado un comando de envío de chat (id local 1) y que no hay asociación SCTP, cuando el transporte lo procesa, entonces se emite un error de envío asociado al id 1 y el mensaje no se pierde en silencio |

### Hito 3 — Integración con la app (iced)

**Background**
```yaml
app: helper de test que también devuelve el emisor de notificaciones, pantalla VideoCall con force_video() y running_call en true (salvo en 3.10)
usuario_local: alice
limites: los de producción (4096 bytes, 200 mensajes). El texto largo del test es "a" repetido 4097 veces
tick: AppMessage::FrameUpdate, enviado a mano
notificaciones_pendientes: uno, dos, tres (de bob)
```

| #   | Given / When / Then |
|-----|---------------------|
| 3.1 | Dada la pantalla de videollamada, cuando el usuario escribe `Hola Bob`, entonces el borrador es `Hola Bob` |
| 3.2 | Dado el borrador `Hola Bob`, cuando el usuario pulsa Enviar (o Enter), entonces sale un mensaje de chat con el texto `Hola Bob`, el historial tiene una entrada propia `Hola Bob`, el borrador queda vacío y `view()` no falla |
| 3.3 | Dado un borrador vacío, de solo espacios o de 4097 bytes, cuando el usuario pulsa Enviar, entonces no sale ningún mensaje de chat y el historial no cambia |
| 3.4 | Dado que llega la notificación de chat `Hola Alice` de bob, entonces el historial tiene una entrada del par `Hola Alice` y `view()` no falla |
| 3.5 | Dadas las tres notificaciones pendientes, cuando la app procesa 3 ticks, entonces el historial muestra `uno`, `dos` y `tres`, una vez cada uno y en ese orden  |
| 3.6 | Dados dos mensajes propios en el historial (ids 1 y 2), cuando llega el error de envío del id 2, entonces el 2 se muestra como "no enviado" y el 1 queda igual |
| 3.7 | Dado un historial con mensajes y un borrador, cuando ocurre `EndCall` (o, en otro caso, `RemoteCallEnded`), entonces la app vuelve al lobby y en la llamada siguiente historial y borrador están vacíos |
| 3.8 | Dadas 2 notificaciones de chat del par que llegan con la app ya en el lobby (el par mandó un mensaje justo antes de cortar), cuando empieza una llamada nueva y se procesan 2 ticks, entonces no hay panic y ninguna aparece en el historial |
| 3.9 | Regresión: los tests existentes de `unified_app` (`send_file_sends_sctp_command_with_current_path`, los de `EndCall` y `RemoteCallEnded`, `view_and_subscription_smoke_for_each_screen`) siguen pasando sin cambiar su semántica |
| 3.10 | Dado `running_call` en falso en la pantalla de videollamada y el borrador `Hola`, cuando el usuario pulsa Enviar, entonces no sale ningún comando y el historial no cambia |
| 3.11 | Dado que el canal de comandos hacia el transporte ya no acepta mensajes (el hilo SCTP terminó), cuando el usuario envía `Hola`, entonces el historial tiene la entrada propia `Hola` marcada "no enviado", la vista lo muestra y la app no falla |

### Hito 4 — Integración y prueba manual

El Hito 4 tiene dos partes:
- **Automática:** tests `chat_4_*`, sin red real ni cámara.
- **Manual:** con dos procesos reales. El resultado de cada paso se registra en [`evidencia/prueba_manual.md`](evidencia/prueba_manual.md).

**Background (parte automática)**
```yaml
apps: A (alice) y B (bob), dos UnifiedApp en VideoCall con running_call en true (helper del Hito 3)
transporte: asociaciones sctp-proto conectadas en memoria (harness del Hito 2), sin sockets ni DTLS
puente: un helper de test hace lo que hacen los hilos SCTP TX y RX de cliente.rs, con las mismas funciones
  (send_chat, route_incoming). Lleva los comandos de cada app a la asociación y lo recibido al receptor
  de notificaciones de la otra app
tick: AppMessage::FrameUpdate, enviado a mano
limites: los de producción
```

| #   | Given / When / Then |
|-----|---------------------|
| 4.1 | Dadas A y B en llamada, cuando A envía `a1`, `a2`, `a3` y B envía `b1`, `b2` intercalados, y se procesan el puente y los ticks, entonces el historial de B tiene del par `a1`, `a2`, `a3` en ese orden, el de A tiene del par `b1`, `b2` en ese orden, cada mensaje aparece una sola vez y ningún mensaje propio queda "no enviado" |
| 4.2 | Dado que A le ofrece a B el archivo `foto.png` de 100 bytes por el stream de control y B muestra el modal de oferta, cuando A y B intercambian mensajes de chat, entonces los mensajes llegan en ambos sentidos, la oferta pendiente de B sigue siendo `foto.png` de 100 bytes, la vista de B se construye sin fallar y, al aceptar, B emite el mismo comando de aceptación de siempre |

Lo que esta parte no cubre se prueba en la parte manual: el código que une las funciones dentro de los hilos de `cliente.rs`, DTLS y los medios.

Los puntos del Hito 4 de la versión anterior de esta tabla quedan cubiertos así:
- "Archivo de 64 KiB más 20 mensajes cruzados": cubierto por 2.6.
- "Limpieza al cortar": cubierto por 3.7 y 3.8.
- "Llamada nueva sin mensajes viejos": no es un paso manual, porque la prueba reinicia los clientes entre llamadas. La segunda llamada sin reiniciar se registra como hallazgo (M5).

**Parte manual (checklist)**

Dos procesos `cliente` y el servidor de señalización locales, con audio y video activos. Ver la plantilla para los pasos detallados y el registro.

| #  | Paso | Resultado esperado |
|----|------|--------------------|
| M1 | Saludo en ambos sentidos | Cada mensaje aparece una vez del otro lado, en orden |
| M2 | Chat durante la transferencia de un archivo | Los mensajes llegan en orden, y el `shasum` del original y del recibido coincide |
| M3 | Mensaje apenas empieza la llamada | Llega, o queda marcado "no enviado". Nunca se pierde en silencio |
| M4 | Corte controlado | Ambos vuelven al lobby y la app no se cuelga (registrar el comportamiento al cerrar) |
| M5 | Segunda llamada sin reiniciar los clientes | Se registra como **hallazgo** sobre la hipótesis de `exploracion.md` (punto 3), no como fallo del chat |
| M6 | Etiquetas y ancho del panel | Se confirman `Yo:`, `Participante:` y `(no enviado)`, y que el panel entra junto a los videos |

La plantilla también tiene lugar para las limitaciones de la Mac y para la medición de latencia, con sus límites.

### Criterios globales (Definition of Done de la feature)
- [ ] Todos los tests de los Hitos 1 a 4 pasan, cada uno con su nombre `chat_<N>_<M>_...`.
- [ ] La prueba manual del Hito 4 está registrada en `docs/chat/evidencia/prueba_manual.md`.
- [ ] Ningún test existente se rompe. Resultado igual o mejor que la línea base.
- [ ] Ningún test del chat depende de red real, cámara ni archivos de configuración.
- [ ] Sin cambios en `src/bin/server.rs` ni en el enum `Message` de señalización.
- [ ] `openspec/specs/` contiene la spec del chat archivada.
