# Prueba manual — Hito 4 del chat

Registro de la parte manual del Hito 4 (`docs/chat/test_aceptacion.md`, pasos M1 a M6; design D8 del change `in-call-chat`). Cada paso se completa con un resultado: **pasó**, **falló** o **no se pudo ejecutar**, siempre con el motivo.

## Datos de la ejecución

| Campo | Valor |
|-------|-------|
| Fecha | |
| Quién la ejecutó | |
| Máquina y sistema (modelo, versión de macOS) | |
| Commit probado (`git rev-parse --short HEAD`) | |
| Usuarios de prueba (A y B) | |

## Preparación

Todo se corre desde la raíz del repo, en terminales separadas:

```bash
cargo run --bin server                                    # señalización en 127.0.0.1:8443
cargo run --bin cliente src/config/cliente1_mac.config    # cliente A
cargo run --bin cliente src/config/cliente2_mac.config    # cliente B
```

- [ ] `certs/` contiene `localhost+2.pem`, `localhost+2-key.pem` y `rootCA.pem`.
- [ ] Los dos usuarios están registrados y logueados.
- [ ] A llama a B y B acepta. Se ven los dos videos y hay audio.

Entre llamadas se **reinician los dos clientes**. M5 es la única excepción.

---

## M1 — Saludo en ambos sentidos

**Pasos:** A envía `hola desde A` (con Enter). B envía `hola desde B` (con el botón Enviar). A envía `uno`, `dos` y `tres` seguidos.

**Esperado:** cada mensaje aparece una sola vez del otro lado, en orden, como `Participante: …`. Del lado propio aparece como `Yo: …`, sin "(no enviado)".

| Resultado | Observaciones |
|-----------|---------------|
| | |

## M2 — Chat durante la transferencia de un archivo

**Pasos:**
1. Elegir un archivo de **menos de 4 MB** (límite `SCTP_MAX_FILE_SIZE`). Calcular su hash: `shasum -a 256 <archivo>`.
2. A selecciona el archivo y lo envía. B ve el modal de oferta y acepta.
3. Mientras se transfiere, A y B intercambian al menos 5 mensajes cada uno.
4. Al terminar, B tiene `received_<nombre>` en el directorio desde donde corre su cliente. Calcular `shasum -a 256 received_<nombre>`.

**Esperado:** los hashes coinciden, los mensajes llegan en orden y sin duplicados, y no hay una interrupción notable de audio ni de video.

| Campo | Valor |
|-------|-------|
| Archivo y tamaño | |
| `shasum -a 256` del original | |
| `shasum -a 256` del recibido | |
| ¿Coinciden? | |
| Mensajes en orden y sin duplicados | |
| Interrupción de medios (audio o video), cualitativa | |
| Resultado | |
| Observaciones | |

### Medición de latencia

| Campo | Valor |
|-------|-------|
| Método (grabación de pantalla contando cuadros, o cronómetro) | |
| Cuadros por segundo de la grabación (si aplica) | |
| Muestras (mensaje → tiempo entre el Enter y su aparición del otro lado) | |
| Latencia sin transferencia (al menos 3 muestras) | |
| Latencia durante la transferencia (al menos 3 muestras) | |

**Límites de la medición** (completar o confirmar):
- Resolución: ±1 cuadro (unos 17 ms a 60 fps) con grabación, o la reacción humana (unos 200 ms) con cronómetro.
- Los dos clientes corren en la misma máquina y se comunican por loopback, así que no representa una red real.
- Mide lo que percibe el usuario, no la latencia del transporte. Los logs no tienen marca de tiempo.
- Otros:

## M3 — Mensaje apenas empieza la llamada

**Pasos:** en cuanto aparece la pantalla de videollamada (antes de que se vea el video remoto), A envía `temprano`.

**Esperado:** `temprano` llega a B, **o** queda del lado de A como `Yo: temprano (no enviado)`. Nunca debe pasar que no llegue y tampoco quede marcado.

| Campo | Valor |
|-------|-------|
| ¿Llegó a B? | |
| ¿Quedó "no enviado" en A? | |
| Resultado (falla si no llegó y no quedó marcado) | |
| Observaciones (cuánto tardó en verse el video remoto) | |

## M4 — Corte controlado

**Pasos:** con mensajes en el historial, A pulsa "Finalizar llamada". Después, cada usuario cierra su ventana.

**Esperado:** A y B vuelven al lobby, y la app no se cuelga. Se registra qué pasa al cerrar la ventana.

| Campo | Valor |
|-------|-------|
| ¿A volvió al lobby? | |
| ¿B volvió al lobby (corte remoto)? | |
| ¿Algún proceso quedó colgado al cerrar? ¿Cuál y cómo se terminó? | |
| Resultado | |
| Observaciones | |

## M5 — Segunda llamada sin reiniciar (hallazgo)

**Pasos:** sin reiniciar los clientes después de M4, A vuelve a llamar a B, y B acepta. Probar video, audio y un mensaje de chat.

**Qué se registra:** si la segunda llamada vuelve a negociar DTLS y SCTP. Es la hipótesis de `docs/chat/exploracion.md`, punto 3, que predice que no. **Esto es un hallazgo, no un fallo del chat:** la renegociación entre llamadas está fuera del alcance del change.

| Campo | Valor |
|-------|-------|
| ¿Hay video y audio? | |
| ¿Llega el chat? ¿Queda "no enviado"? | |
| Mensajes relevantes en la consola de cada cliente | |
| ¿Confirma o refuta la hipótesis? | |

## M6 — Etiquetas y ancho del panel

**Pasos:** revisar el panel de chat durante M1 a M3, también con el modal de oferta visible (M2).

| Ítem | Esperado | ¿Se confirma? / Observaciones |
|------|----------|-------------------------------|
| Etiqueta propia | `Yo: <texto>` | |
| Etiqueta del participante | `Participante: <texto>` | |
| Mensaje no enviado | `Yo: <texto> (no enviado)`, en rojo | |
| Ancho del panel (300 px) | Entra junto a los dos videos sin cortarse | |
| Mensajes largos o muchos mensajes | Se pueden leer, y el historial tiene scroll | |
| Con el modal de oferta visible | Se registra si el campo de chat se puede usar | |

Si alguna etiqueta o el ancho cambian, primero se actualiza design D6 y la spec (tarea 4.6).

---

## Limitaciones de la Mac

| Limitación | ¿Ocurrió? | Efecto en la prueba | Cómo se resolvió o sorteó |
|------------|-----------|---------------------|---------------------------|
| Permiso de cámara para `avfvideosrc` | | | |
| Cliente colgado al cerrar la ventana | | | |
| Otras | | | |

## Hallazgos

| # | Hallazgo | ¿Es del chat? | Acción (criterio o test nuevo, o solo registro) |
|---|----------|---------------|-------------------------------------------------|
| | | | |

## Resumen

| Paso | Resultado |
|------|-----------|
| M1 | |
| M2 | |
| M3 | |
| M4 | |
| M5 (hallazgo) | |
| M6 | |
