# Spec Delta

## Purpose

Permite que los dos participantes de una videollamada activa intercambien mensajes de texto plano en tiempo real, sin afectar la transferencia de archivos y sin que el chat sobreviva al fin de la llamada.

## ADDED Requirements

### Requirement: Envío de un mensaje válido

Durante una llamada activa con el canal de envío listo, el sistema SHALL enviar al participante el texto válido que escribe el usuario, exactamente como lo escribió y sin recortarlo, y SHALL agregarlo al historial local como mensaje propio. Al enviar desde la pantalla de la llamada, el borrador SHALL quedar vacío.

#### Scenario: chat_1_1_envia_mensaje_valido
- **GIVEN** una sesión activa con el canal de envío listo y el texto `Hola`
- **WHEN** Alice lo envía
- **THEN** se produce un mensaje con contenido exacto `Hola`
- **AND** el historial de Alice tiene una entrada propia `Hola`

#### Scenario: chat_1_4_conserva_espacios_del_texto
- **GIVEN** el texto `"  hola  "`
- **WHEN** Alice lo envía
- **THEN** el contenido del mensaje es exactamente `"  hola  "`

#### Scenario: chat_3_1_actualiza_borrador
- **GIVEN** la pantalla de videollamada
- **WHEN** el usuario escribe `Hola Bob`
- **THEN** el borrador es `Hola Bob`

#### Scenario: chat_3_2_enviar_agrega_al_historial_y_limpia_borrador
- **GIVEN** el borrador `Hola Bob` en la pantalla de videollamada
- **WHEN** el usuario pulsa Enviar o Enter
- **THEN** sale un mensaje de chat con el texto `Hola Bob`
- **AND** el historial tiene una entrada propia `Hola Bob`
- **AND** el borrador queda vacío
- **AND** la vista se puede construir sin fallar

### Requirement: Validación del mensaje saliente

El sistema SHALL rechazar con un error identificable el texto vacío o de solo espacios en blanco, y el texto que supere el límite de bytes UTF-8 configurado. El límite se mide en bytes, no en caracteres. Un mensaje rechazado MUST NOT generar un envío ni modificar el historial.

#### Scenario: chat_1_2_rechaza_mensaje_vacio_o_solo_espacios
- **GIVEN** el texto vacío o `"   "`
- **WHEN** Alice intenta enviarlo
- **THEN** se rechaza con un error identificable
- **AND** no se genera ningún comando de envío
- **AND** el historial no cambia

#### Scenario: chat_1_3_rechaza_mensaje_sobre_el_limite
- **GIVEN** un límite de 16 bytes
- **WHEN** se valida un texto de exactamente 16 bytes (`aaaaaaaaaaaaaaaa`)
- **THEN** se acepta
- **AND** un texto de 17 bytes se rechaza con un error identificable

#### Scenario: chat_3_3_no_envia_borrador_invalido
- **GIVEN** un borrador vacío, de solo espacios o de 4097 bytes, con los límites de producción
- **WHEN** el usuario pulsa Enviar
- **THEN** no sale ningún mensaje de chat
- **AND** el historial no cambia

### Requirement: Límites de producción

En producción, el sistema SHALL limitar cada mensaje a 4096 bytes UTF-8 y el historial a 200 mensajes. Ambos valores SHALL poder reemplazarse por otros en las pruebas.

#### Scenario: chat_1_12_valores_de_produccion
- **GIVEN** la configuración de producción
- **WHEN** se consultan los límites
- **THEN** el límite por mensaje es 4096 bytes
- **AND** el tope de historial es 200 mensajes

### Requirement: Contenido preservado de punta a punta

El sistema SHALL conservar el texto exacto, incluidos los caracteres no ASCII, al codificarlo para enviarlo y al decodificarlo al recibirlo. El texto SHALL mostrarse literalmente, sin interpretarlo como marcado.

#### Scenario: chat_1_5_codifica_y_decodifica_utf8
- **GIVEN** el texto `¿Qué tal?` (11 bytes)
- **WHEN** se codifica y se decodifica
- **THEN** el resultado es igual al original

### Requirement: El receptor no confía en el emisor

Al recibir, el sistema SHALL descartar sin interrumpir la aplicación todo payload que no sea UTF-8 válido, que esté vacío o que supere el límite configurado. Cada caso SHALL producir un error identificable.

#### Scenario: chat_1_6_descarta_payload_invalido_sin_panic
- **GIVEN** los payloads recibidos `[0xFF, 0xFE]` (UTF-8 inválido), vacío y de 17 bytes, con un límite de 16 bytes
- **WHEN** se decodifican
- **THEN** cada uno falla con un error identificable
- **AND** la aplicación no se interrumpe

### Requirement: Envío solo durante una llamada activa

El sistema SHALL rechazar el envío cuando no hay una llamada activa o el canal de envío no está listo, sin generar un envío ni modificar el historial.

#### Scenario: chat_1_7_rechaza_envio_sin_llamada_activa
- **GIVEN** una sesión inactiva o terminada, o un canal de envío que no está listo
- **WHEN** Alice intenta enviar `Hola`
- **THEN** se rechaza
- **AND** no se genera ningún comando de envío
- **AND** el historial no cambia

#### Scenario: chat_3_10_no_envia_con_running_call_en_falso
- **GIVEN** la pantalla de videollamada con `running_call` en falso y el borrador `Hola`
- **WHEN** el usuario pulsa Enviar
- **THEN** no sale ningún comando
- **AND** el historial no cambia

### Requirement: Reporte de fallo de envío por mensaje

Si el transporte no puede escribir un mensaje propio, el sistema SHALL marcar como "no enviado" solo ese mensaje, identificado por su id local y no por su texto. Los demás mensajes del historial MUST NOT cambiar, y la aplicación SHALL seguir operativa.

#### Scenario: chat_1_8_marca_no_enviado_por_id_local
- **GIVEN** que Alice envió dos mensajes con el mismo texto `ok`, con ids locales 1 y 2
- **WHEN** llega un error de envío para el id 2
- **THEN** solo el mensaje 2 queda "no enviado"
- **AND** el mensaje 1 queda igual
- **AND** ambos siguen en el historial

#### Scenario: chat_2_7_fallo_de_escritura_reporta_id
- **GIVEN** un fallo inyectado al escribir en el transporte
- **WHEN** A envía `001` con id local 1
- **THEN** se emite un error de envío asociado al id 1
- **AND** el cliente sigue operativo

#### Scenario: chat_2_9_envio_sin_asociacion_reporta_fallo
- **GIVEN** un comando de envío de chat con id local 1 y ninguna asociación SCTP establecida
- **WHEN** el transporte lo procesa
- **THEN** se emite un error de envío asociado al id 1
- **AND** el mensaje no se pierde en silencio

#### Scenario: chat_3_11_marca_no_enviado_si_el_canal_esta_cerrado
- **GIVEN** la pantalla de videollamada y un canal de comandos hacia el transporte que ya no acepta mensajes
- **WHEN** el usuario envía `Hola`
- **THEN** el historial tiene la entrada propia `Hola` marcada "no enviado"
- **AND** la vista lo muestra como "no enviado"
- **AND** la app no falla

#### Scenario: chat_3_6_muestra_no_enviado
- **GIVEN** dos mensajes propios en el historial, con ids 1 y 2
- **WHEN** la app recibe el error de envío del id 2
- **THEN** el mensaje 2 se muestra como "no enviado"
- **AND** el mensaje 1 queda igual

### Requirement: Entrega en orden, sin duplicados ni pérdidas

El sistema SHALL entregar al participante cada mensaje exactamente una vez, en el orden en que lo envió el emisor, incluso en ráfagas. Cada mensaje recibido SHALL agregarse al historial como mensaje del participante.

#### Scenario: chat_1_9_recibe_en_orden_sin_duplicados
- **GIVEN** que Alice recibe `uno`, `dos` y `tres` de Bob
- **WHEN** se procesan
- **THEN** su historial tiene las tres entradas del participante, una vez cada una y en ese orden

#### Scenario: chat_2_1_entrega_un_mensaje
- **GIVEN** los extremos A y B con el canal listo
- **WHEN** A envía `001`
- **THEN** B recibe exactamente una notificación de chat con el contenido `001`

#### Scenario: chat_2_2_preserva_orden_de_rafaga
- **GIVEN** los extremos A y B con el canal listo
- **WHEN** A envía `001`, `002` y `003`
- **THEN** B los recibe en ese orden, una vez cada uno

#### Scenario: chat_3_4_muestra_mensaje_recibido
- **GIVEN** la pantalla de videollamada
- **WHEN** llega la notificación de chat `Hola Alice` de bob
- **THEN** el historial tiene una entrada del participante `Hola Alice`
- **AND** la vista se puede construir sin fallar

#### Scenario: chat_3_5_procesa_una_notificacion_por_tick
- **GIVEN** tres notificaciones de chat pendientes de bob: `uno`, `dos` y `tres`
- **WHEN** la app procesa 3 ticks
- **THEN** el historial muestra `uno`, `dos` y `tres`, una vez cada uno y en ese orden

#### Scenario: chat_4_1_dos_apps_intercambian_en_ambos_sentidos
- **GIVEN** dos instancias de la app, A y B, en una llamada activa conectadas por el transporte
- **WHEN** A envía `a1`, `a2` y `a3`, y B envía `b1` y `b2`, intercalados
- **THEN** B muestra del participante `a1`, `a2` y `a3`, en ese orden y una vez cada uno
- **AND** A muestra del participante `b1` y `b2`, en ese orden y una vez cada uno
- **AND** ningún mensaje propio queda "no enviado"

### Requirement: Tope del historial

Cuando un mensaje nuevo haría superar el tope configurado, el sistema SHALL descartar el mensaje más viejo del historial.

#### Scenario: chat_1_10_descarta_el_mas_viejo_al_superar_el_tope
- **GIVEN** un tope de 3 y un historial con `a`, `b` y `c`
- **WHEN** llega `d`
- **THEN** el historial tiene 3 entradas: `b`, `c` y `d`

### Requirement: El chat vive solo durante la llamada

Al terminar la llamada, la termine cualquiera de los dos participantes, el sistema SHALL vaciar el historial y el borrador. Los mensajes por enviar y las notificaciones de una llamada anterior MUST NOT transmitirse ni mostrarse en la llamada siguiente.

#### Scenario: chat_1_11_nueva_sesion_arranca_vacia
- **GIVEN** un historial con mensajes y el borrador `hola`
- **WHEN** termina la sesión y empieza una nueva
- **THEN** la nueva sesión tiene el historial y el borrador vacíos

#### Scenario: chat_2_8_comandos_pendientes_no_pasan_a_nueva_llamada
- **GIVEN** 2 comandos de envío de chat pendientes en la cola
- **WHEN** termina la llamada y empieza otra
- **THEN** ninguno de los dos se transmite en la llamada nueva

#### Scenario: chat_3_7_fin_de_llamada_limpia_chat
- **GIVEN** un historial con mensajes y un borrador
- **WHEN** ocurre `EndCall`, o en otro caso `RemoteCallEnded`
- **THEN** la app vuelve al lobby
- **AND** en la llamada siguiente el historial y el borrador están vacíos

#### Scenario: chat_3_8_ignora_notificaciones_de_llamada_anterior
- **GIVEN** 2 notificaciones de chat del participante que llegaron con la app ya en el lobby
- **WHEN** empieza una llamada nueva y se procesan 2 ticks
- **THEN** la app no se interrumpe
- **AND** ninguna de las dos aparece en el historial

### Requirement: Aislamiento respecto de la transferencia de archivos

El chat SHALL viajar por un canal propio, separado del control y del contenido de archivos. Un mensaje de chat MUST NOT disparar ofertas ni alterar transferencias. Los mensajes de archivos MUST NOT generar notificaciones de chat. El flujo de archivos y su modal de oferta SHALL comportarse igual que antes.

#### Scenario: chat_2_3_oferta_de_archivo_no_genera_chat
- **GIVEN** `OFFER foto.png 100` en el stream de control de archivos
- **WHEN** B lo procesa
- **THEN** se genera la notificación de oferta de archivo de siempre
- **AND** no se genera ninguna notificación de chat

#### Scenario: chat_2_4_contenido_de_archivo_no_genera_chat
- **GIVEN** un payload que parece texto de chat en el stream de contenido de archivos
- **WHEN** B lo procesa
- **THEN** no se genera ninguna notificación de chat

#### Scenario: chat_2_5_chat_con_texto_offer_no_genera_oferta
- **GIVEN** los extremos A y B con el canal listo
- **WHEN** A envía por chat el texto `OFFER foto.png 100`
- **THEN** B recibe una notificación de chat con ese texto
- **AND** no se genera ninguna oferta de archivo

#### Scenario: chat_2_6_chat_durante_transferencia_de_archivo
- **GIVEN** una transferencia en curso de A a B de un archivo de 64 KiB cuyo byte i vale `i mod 251`
- **WHEN** A y B intercambian 20 mensajes de chat
- **THEN** B recibe los 20 mensajes de A en orden
- **AND** el archivo recibido es idéntico byte a byte al original

#### Scenario: chat_3_9_flujo_de_archivos_sin_cambios
- **GIVEN** la app con el chat incorporado
- **WHEN** se ejecutan los tests existentes de `unified_app` (`send_file_sends_sctp_command_with_current_path`, los de `EndCall` y `RemoteCallEnded`, `view_and_subscription_smoke_for_each_screen`)
- **THEN** pasan sin cambiar su semántica
- **AND** con un historial de chat no vacío, enviar un archivo sigue generando el mismo comando de envío de archivo con la ruta actual

#### Scenario: chat_4_2_chat_con_oferta_de_archivo_pendiente
- **GIVEN** dos instancias de la app, A y B, en una llamada activa conectadas por el transporte
- **AND** A le ofreció a B el archivo `foto.png` de 100 bytes y B muestra la oferta pendiente
- **WHEN** A y B intercambian mensajes de chat
- **THEN** los mensajes llegan en ambos sentidos
- **AND** la oferta pendiente de B sigue siendo `foto.png` de 100 bytes y la vista de B se construye sin fallar
- **AND** al aceptar la oferta, B emite la misma aceptación de archivo que sin chat
