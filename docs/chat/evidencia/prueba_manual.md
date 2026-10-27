# Prueba manual — Hito 4 del chat

Registro de la parte manual del Hito 4 (`docs/chat/test_aceptacion.md`, pasos M1 a M6; design D8 del change `in-call-chat`). Cada paso se completa con un resultado: **pasó**, **falló** o **no se pudo ejecutar**, siempre con el motivo.

## Datos de la ejecución

| Campo | Valor |
|-------|-------|
| Fecha | 2026-10-27|
| Quién la ejecutó | Fede|
| Máquina y sistema (modelo, versión de macOS) | Macbook Pro - macOS 26.4|
| Commit probado (`git rev-parse --short HEAD`) | 1f47c91 |
| Usuarios de prueba (A y B) |A y B |

## Preparación

Todo se corre desde la raíz del repo, en terminales separadas:

```bash
cargo run --bin server                                    # señalización en 127.0.0.1:8443
cargo run --bin cliente src/config/cliente1_mac.config    # cliente A
cargo run --bin cliente src/config/cliente2_mac.config    # cliente B
```

- [X] `certs/` contiene `localhost+2.pem`, `localhost+2-key.pem` y `rootCA.pem`.
- [X] Los dos usuarios están registrados y logueados.
- [X] A llama a B y B acepta. Se ven los dos videos y hay audio.

Entre llamadas se **reinician los dos clientes**. M5 es la única excepción.

---

## M1 — Saludo en ambos sentidos

**Pasos:** A envía `hola desde A` (con Enter). B envía `hola desde B` (con el botón Enviar). A envía `uno`, `dos` y `tres` seguidos.

**Esperado:** cada mensaje aparece una sola vez del otro lado, en orden, con el nombre de quien lo envió (`<usuario A>: …` en B). Del lado propio aparece con el nombre propio, sin "(no enviado)".

| Resultado | Observaciones |
|-----------|---------------|
| Paso | |

## M2 — Chat durante la transferencia de un archivo

**Pasos:**
1. Elegir un archivo de **menos de 4 MB** (límite `SCTP_MAX_FILE_SIZE`). Calcular su hash: `shasum -a 256 <archivo>`.
2. A selecciona el archivo y lo envía. B ve el modal de oferta y acepta.
3. Mientras se transfiere, A y B intercambian al menos 5 mensajes cada uno.
4. Al terminar, B tiene `received_<nombre>` en el directorio desde donde corre su cliente. Calcular `shasum -a 256 received_<nombre>`.

**Esperado:** los hashes coinciden, los mensajes llegan en orden y sin duplicados, y no hay una interrupción notable de audio ni de video.

| Campo | Valor |
|-------|-------|
| Archivo y tamaño | imagen.jpg - 23 KB|
| `shasum -a 256` del original | c6ed8ff697d6fed814275849b71f4867ca9d5b5cfec4ee7d2ea1f7aad27d184a |
| `shasum -a 256` del recibido | c6ed8ff697d6fed814275849b71f4867ca9d5b5cfec4ee7d2ea1f7aad27d184a |
| ¿Coinciden? | SI |
| Mensajes en orden y sin duplicados | Paso |
| Interrupción de medios (audio o video), cualitativa | Paso |
| Resultado | Paso |
| Observaciones | NO |

### Medición de latencia

| Campo | Valor |
|-------|-------|
| Método (grabación de pantalla contando cuadros, o cronómetro) | No se puede ejecutar por no tener las herramienta necesarias|
| Cuadros por segundo de la grabación (si aplica) | No aplica|
| Muestras (mensaje → tiempo entre el Enter y su aparición del otro lado) | Paso |
| Latencia sin transferencia (al menos 3 muestras) | Se reciben siempre los mensajes enviados. Se repite la muestra en 3 llamadas distintas|
| Latencia durante la transferencia (al menos 3 muestras) | Se reciben correctamente tanto los mensajes como los archivos. LA muestra se toma de 3 llamadas distintas.|

**Límites de la medición** (completar o confirmar):
- Resolución: ±1 cuadro (unos 17 ms a 60 fps) con grabación, o la reacción humana (unos 200 ms) con cronómetro.
- Los dos clientes corren en la misma máquina y se comunican por loopback, así que no representa una red real.
- Mide lo que percibe el usuario, no la latencia del transporte. Los logs no tienen marca de tiempo.
- Otros:

## M3 — Mensaje apenas empieza la llamada

**Pasos:** en cuanto aparece la pantalla de videollamada (antes de que se vea el video remoto), A envía `temprano`.

**Esperado:** `temprano` llega a B, **o** queda del lado de A como `<usuario A>: temprano (no enviado)`. Nunca debe pasar que no llegue y tampoco quede marcado.

| Campo | Valor |
|-------|-------|
| ¿Llegó a B? | Si |
| ¿Quedó "no enviado" en A? | No |
| Resultado (falla si no llegó y no quedó marcado) | Paso |
| Observaciones (cuánto tardó en verse el video remoto) | NA |

## M4 — Corte controlado

**Pasos:** con mensajes en el historial, A pulsa "Finalizar llamada". Después, cada usuario cierra su ventana.

**Esperado:** A y B vuelven al lobby, y la app no se cuelga. Se registra qué pasa al cerrar la ventana.

| Campo | Valor |
|-------|-------|
| ¿A volvió al lobby? | Paso |
| ¿B volvió al lobby (corte remoto)? | Paso |
| ¿Algún proceso quedó colgado al cerrar? ¿Cuál y cómo se terminó? | Ningun proceso quedo colgado |
| Resultado | Paso |
| Observaciones | |

## M5 — Segunda llamada sin reiniciar (hallazgo)

**Pasos:** sin reiniciar los clientes después de M4, A vuelve a llamar a B, y B acepta. Probar video, audio y un mensaje de chat.

**Qué se registra:** si la segunda llamada vuelve a negociar DTLS y SCTP. Es la hipótesis de `docs/chat/exploracion.md`, punto 3, que predice que no. **Esto es un hallazgo, no un fallo del chat:** la renegociación entre llamadas está fuera del alcance del change.

| Campo | Valor |
|-------|-------|
| ¿Hay video y audio? | SI |
| ¿Llega el chat? ¿Queda "no enviado"? | Llega el chat |
| Mensajes relevantes en la consola de cada cliente | No se capturó la consola de los clientes. La conclusión se apoya en el diseño (asociación SCTP única por proceso, exploracion.md:25) y en que chat, video y audio siguieron funcionando en la segunda llamada. |
| ¿Confirma o refuta la hipótesis? | Confirma |

## M6 — Etiquetas y tamaño del panel

**Pasos:** revisar el panel de chat durante M1 a M3, también con el modal de oferta visible (M2).

| Ítem | Esperado | ¿Se confirma? / Observaciones |
|------|----------|-------------------------------|
| Etiqueta propia | `<usuario propio>: <texto>` | Paso |
| Etiqueta del participante | `<usuario del par>: <texto>`, en los dos sentidos (A llama a B, y B llama a A) |Paso |
| Mensaje no enviado | `<usuario propio>: <texto> (no enviado)`, en rojo | Paso |
| Ancho del panel (360 px) | Entra junto a los dos videos sin cortarse | Paso |
| Campo de texto | Ocupa el ancho del panel y tiene un alto cómodo para escribir | Paso |
| Mensajes largos o muchos mensajes | Se pueden leer, y el historial tiene scroll | Paso |
| Con el modal de oferta visible | Se registra si el campo de chat se puede usar | Paso |

Si alguna etiqueta o el ancho cambian, primero se actualiza design D6 y la spec (tarea 4.6).

---
## Limitaciones de la Mac

Contexto: CLAUDE.md registra dos limitaciones conocidas de macOS. Esto fue lo observado en esta ejecución.

| Limitación conocida | Observado en esta ejecución |
|---|---|
| La cámara (avfvideosrc) abre pero no se muestra en la llamada | A usó cliente3_mac.config; B usó cliente2_mac.config. Se vio el video de A en B. |


Limitaciones propias de esta prueba: la latencia no se midió (ver la sección de latencia) y las dos instancias corrieron en la misma Mac.
## Hallazgos

| # | Hallazgo | ¿Es del chat? | Acción (criterio o test nuevo, o solo registro) |
|---|----------|---------------|-------------------------------------------------|
| M5 | La segunda llamada reutiliza DTLS y SCTP | No | Solo se registra. |


## Resumen

| Paso | Resultado |
|------|-----------|
| M1 | Paso |
| M2 | Paso |
| M3 | Paso |
| M4 | Paso |
| M5 (hallazgo) | Paso |
| M6 | Paso |
