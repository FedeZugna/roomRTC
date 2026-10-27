//! Tests del Hito 4 (design D7): dos `UnifiedApp` unidas por un puente de test
//! que hace, en un solo hilo, lo que hacen los hilos SCTP TX y RX de
//! `cliente.rs`, sobre el harness SCTP en memoria. Sin red, DTLS ni cámara.

use super::tests::{force_video, make_app_with_all_channels};
use super::*;
use crate::chat::{CHAT_MAX_BYTES, ChatAuthor};
use crate::client::transport::sctp_chat::{
    FileControl, Incoming, notify_without_association, route_incoming, send_chat,
};
use crate::client::transport::sctp_harness::{Peer, SctpPair};
use bytes::Bytes;
use iced::futures::{FutureExt, StreamExt};
use sctp_proto::{Association, PayloadProtocolIdentifier};
use std::sync::mpsc;

/// Extremo del puente: los canales de una app y lo que el hilo TX/RX lee de
/// ella (generación y `running_call`).
struct BridgeSide {
    commands: mpsc::Receiver<SctpCommand>,
    notifications: mpsc::Sender<SctpNotification>,
    generation: Arc<AtomicU64>,
    running_call: Arc<AtomicBool>,
    // Mensajes de control a escribir en el stream 0, como el hilo TX
    pending_control: Vec<String>,
    // Comandos de archivos que emitió la app
    file_commands: Vec<SctpCommand>,
    // Mensajes de control recibidos por el stream 0
    received_control: Vec<FileControl>,
}

impl BridgeSide {
    fn new(
        app: &UnifiedApp,
        commands: mpsc::Receiver<SctpCommand>,
        notifications: mpsc::Sender<SctpNotification>,
    ) -> Self {
        BridgeSide {
            commands,
            notifications,
            generation: Arc::clone(&app.call_generation),
            running_call: Arc::clone(&app.running_call),
            pending_control: Vec::new(),
            file_commands: Vec::new(),
            received_control: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Side {
    A,
    B,
}

/// Puente de test: `a` es el caller de la asociación y `b` el callee.
struct ChatBridge {
    pair: SctpPair,
    a: BridgeSide,
    b: BridgeSide,
}

impl ChatBridge {
    fn connect(a: BridgeSide, b: BridgeSide) -> Result<Self, String> {
        Ok(ChatBridge {
            pair: SctpPair::connect()?,
            a,
            b,
        })
    }

    fn side(&self, side: Side) -> &BridgeSide {
        match side {
            Side::A => &self.a,
            Side::B => &self.b,
        }
    }

    /// Oferta de archivo de `from`, sin leer un archivo real: el próximo
    /// `pump` la escribe en el stream 0 como `OFFER <nombre> <tamaño>`.
    fn offer_file(&mut self, from: Side, filename: &str, size: usize) {
        let side = match from {
            Side::A => &mut self.a,
            Side::B => &mut self.b,
        };
        side.pending_control
            .push(format!("OFFER {} {}", filename, size));
    }

    /// Lleva los comandos de cada app a la asociación y lo recibido a la otra
    /// app, como los hilos SCTP TX y RX.
    fn pump(&mut self) -> Result<(), String> {
        let ChatBridge { pair, a, b } = self;
        transmit(a, &mut pair.a)?;
        transmit(b, &mut pair.b)?;
        pair.drive();
        receive(a, &mut pair.a)?;
        receive(b, &mut pair.b)
    }
}

fn notify(side: &BridgeSide, notification: SctpNotification) -> Result<(), String> {
    side.notifications
        .send(notification)
        .map_err(|e| format!("la app no recibe notificaciones: {e}"))
}

// Escribe un mensaje de control en el stream 0, abriéndolo si hace falta,
// como el hilo SCTP TX.
fn write_control(assoc: &mut Association, msg: &str) -> Result<(), String> {
    let mut stream = match assoc.stream(0) {
        Ok(stream) => stream,
        Err(_) => assoc
            .open_stream(0, PayloadProtocolIdentifier::String)
            .map_err(|e| format!("no se pudo abrir el stream 0: {e:?}"))?,
    };
    stream
        .write_sctp(
            &Bytes::copy_from_slice(msg.as_bytes()),
            PayloadProtocolIdentifier::String,
        )
        .map(|_| ())
        .map_err(|e| format!("no se pudo escribir el control: {e:?}"))
}

// Lado TX: control pendiente y comandos de la app, en orden.
fn transmit(side: &mut BridgeSide, peer: &mut Peer) -> Result<(), String> {
    let commands: Vec<SctpCommand> = side.commands.try_iter().collect();
    let Some(assoc) = peer.assoc_mut() else {
        // Sin asociación, un chat se reporta como fallido (design D6)
        for cmd in &commands {
            if let Some(notification) = notify_without_association(cmd) {
                notify(side, notification)?;
            }
        }
        return Ok(());
    };

    for msg in std::mem::take(&mut side.pending_control) {
        write_control(assoc, &msg)?;
    }

    for cmd in commands {
        match cmd {
            SctpCommand::SendChat {
                id,
                generation,
                text,
            } => {
                let sent = send_chat(
                    assoc,
                    id,
                    generation,
                    &text,
                    side.generation.load(Ordering::SeqCst),
                    side.running_call.load(Ordering::SeqCst),
                );
                if let Err(e) = sent {
                    notify(side, e.to_notification())?;
                }
            }
            SctpCommand::AcceptFile { ref filename } => {
                write_control(assoc, &format!("ACCEPT {}", filename))?;
                side.file_commands.push(cmd);
            }
            SctpCommand::RejectFile { ref filename } => {
                write_control(assoc, &format!("REJECT {}", filename))?;
                side.file_commands.push(cmd);
            }
            // Sin archivo real: la oferta se escribe con `offer_file`
            SctpCommand::SendFile { .. } => side.file_commands.push(cmd),
        }
    }
    Ok(())
}

// Lado RX: clasifica lo recibido con la generación de la app receptora.
fn receive(side: &mut BridgeSide, peer: &mut Peer) -> Result<(), String> {
    let generation = side.generation.load(Ordering::SeqCst);
    for (stream_id, data) in peer.take_received() {
        match route_incoming(stream_id, &data, generation, CHAT_MAX_BYTES) {
            Incoming::Chat(notification) => notify(side, notification)?,
            Incoming::FileControl(control) => {
                if let FileControl::Offer { filename, size } = &control {
                    notify(
                        side,
                        SctpNotification::FileOffer {
                            filename: filename.clone(),
                            size: *size,
                        },
                    )?;
                }
                side.received_control.push(control);
            }
            Incoming::FileContent(_) | Incoming::Ignored => {}
        }
    }
    Ok(())
}

/// Ejecuta un `Task` como el runtime de iced: cada `Action::Output` listo se
/// pasa a `update`, y también se ejecuta el `Task` que devuelve (design D7).
fn run_task(app: &mut UnifiedApp, task: Task<AppMessage>) {
    let Some(mut stream) = iced_runtime::task::into_stream(task) else {
        return;
    };
    while let Some(Some(action)) = stream.next().now_or_never() {
        if let iced_runtime::Action::Output(msg) = action {
            let next = app.update(msg);
            run_task(app, next);
        }
    }
}

// Ticks de `FrameUpdate` por ronda: más que las notificaciones pendientes,
// porque cada tick procesa una sola.
const TICKS: usize = 10;

// App en `VideoCall` con `running_call` en true (helper del Hito 3), con su
// extremo del puente.
fn app_in_call(username: &str) -> (UnifiedApp, BridgeSide) {
    let (mut app, _sig_rx, sctp_rx, notif_tx) = make_app_with_all_channels();
    force_video(&mut app);
    app.running_call.store(true, Ordering::SeqCst);
    app.current_username = Some(username.to_string());
    let side = BridgeSide::new(&app, sctp_rx, notif_tx);
    (app, side)
}

fn send(app: &mut UnifiedApp, text: &str) {
    let _ = app.update(AppMessage::ChatDraftChanged(text.to_string()));
    let _ = app.update(AppMessage::ChatSend);
}

fn ticks(app: &mut UnifiedApp, n: usize) {
    for _ in 0..n {
        let task = app.update(AppMessage::FrameUpdate(Instant::now()));
        run_task(app, task);
    }
}

fn texts_by(app: &UnifiedApp, own: bool) -> Vec<String> {
    app.chat
        .history
        .iter()
        .filter(|e| matches!(e.author, ChatAuthor::Own { .. }) == own)
        .map(|e| e.text.clone())
        .collect()
}

fn any_not_sent(app: &UnifiedApp) -> bool {
    app.chat.history.iter().any(|e| e.not_sent)
}

fn pending_offer(app: &UnifiedApp) -> Option<(String, usize)> {
    match &app.screen {
        AppScreen::VideoCall {
            pending_file_offer: Some(offer),
            ..
        } => Some((offer.filename.clone(), offer.size)),
        _ => None,
    }
}

#[test]
fn chat_4_1_dos_apps_intercambian_en_ambos_sentidos() -> Result<(), String> {
    let (mut a, side_a) = app_in_call("alice");
    let (mut b, side_b) = app_in_call("bob");
    let mut bridge = ChatBridge::connect(side_a, side_b)?;

    // Intercalados, con el puente y los ticks entre rondas
    send(&mut a, "a1");
    send(&mut b, "b1");
    send(&mut a, "a2");
    bridge.pump()?;
    ticks(&mut a, TICKS);
    ticks(&mut b, TICKS);
    send(&mut b, "b2");
    send(&mut a, "a3");
    bridge.pump()?;
    ticks(&mut a, TICKS);
    ticks(&mut b, TICKS);

    assert_eq!(texts_by(&b, false), ["a1", "a2", "a3"]);
    assert_eq!(texts_by(&a, false), ["b1", "b2"]);
    assert_eq!(texts_by(&a, true), ["a1", "a2", "a3"]);
    assert_eq!(texts_by(&b, true), ["b1", "b2"]);
    assert!(!any_not_sent(&a));
    assert!(!any_not_sent(&b));
    Ok(())
}

#[test]
fn chat_4_2_chat_con_oferta_de_archivo_pendiente() -> Result<(), String> {
    let (mut a, side_a) = app_in_call("alice");
    let (mut b, side_b) = app_in_call("bob");
    let mut bridge = ChatBridge::connect(side_a, side_b)?;
    let oferta = Some(("foto.png".to_string(), 100));

    // A ofrece el archivo y B muestra la oferta pendiente
    bridge.offer_file(Side::A, "foto.png", 100);
    bridge.pump()?;
    ticks(&mut b, TICKS);
    assert_eq!(pending_offer(&b), oferta);
    let _ = b.view();

    // Chat en ambos sentidos con la oferta pendiente
    send(&mut a, "te mandé una foto");
    send(&mut b, "ya la veo");
    bridge.pump()?;
    ticks(&mut a, TICKS);
    ticks(&mut b, TICKS);

    assert_eq!(texts_by(&b, false), ["te mandé una foto"]);
    assert_eq!(texts_by(&a, false), ["ya la veo"]);
    assert!(!any_not_sent(&a));
    assert!(!any_not_sent(&b));
    assert_eq!(pending_offer(&b), oferta);
    let _ = b.view();

    // Aceptar emite la misma aceptación de siempre, que llega a A por el stream 0
    let _ = b.update(AppMessage::AcceptFileTransfer {
        filename: "foto.png".to_string(),
    });
    bridge.pump()?;

    assert_eq!(pending_offer(&b), None);
    assert_eq!(
        bridge.side(Side::B).file_commands,
        vec![SctpCommand::AcceptFile {
            filename: "foto.png".to_string()
        }]
    );
    assert_eq!(
        bridge.side(Side::A).received_control,
        vec![FileControl::Accept {
            filename: "foto.png".to_string()
        }]
    );
    Ok(())
}
