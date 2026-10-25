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

### Hito 3 — Integración con la app (iced)

**Background**
```yaml
app: make_app_with_channels(), pantalla VideoCall con force_video()
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

### Hito 4 — Integración y prueba manual 

- Dos `UnifiedApp` intercambian mensajes en ambos sentidos, orden por emisor, archivo de 64 KiB más 20 mensajes cruzados con el archivo idéntico, y limpieza al cortar.
- Chat con una oferta de archivo pendiente (modal visible): el chat funciona y la oferta no cambia.
- Prueba manual con dos procesos reales (servidor de señalización local, dos `cliente`, audio y video activos): saludo en ambos sentidos, chat durante una transferencia de archivo sin interrupción observable de medios (consignando las limitaciones de la medición), desconexión controlada del par, el chat funciona en la primera llamada tras reiniciar.


### Criterios globales (Definition of Done de la feature)
- [ ] Todos los tests de los Hitos 1 a 3 pasan, cada uno con su nombre `chat_<N>_<M>_...`.
- [ ] Ningún test existente se rompe. Resultado igual o mejor que la línea base.
- [ ] Ningún test del chat depende de red real, cámara ni archivos de configuración.
- [ ] Sin cambios en `src/bin/server.rs` ni en el enum `Message` de señalización.
- [ ] `openspec/specs/` contiene la spec del chat archivada.
