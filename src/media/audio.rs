// Audio (encoder Opus atras con gstreamer)
//  - Encoder: mic -> opusenc -> appsink -> Receiver<Vec<u8>>
//    (si muted=true, descarta samples pero dejo pipeline vivo)
//  - Player:  appsrc <- Vec<u8> -> opusdec -> autoaudiosink
//

use gstreamer as gst;
use gstreamer::MessageView;
use gstreamer::prelude::*;
use gstreamer_app::{AppSink, AppSrc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

/// Inicia captura: Mic -> Opus -> Canal
pub fn start_audio_encoder(
    muted: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
) -> Result<Receiver<Vec<u8>>, Box<dyn std::error::Error>> {
    // Pipeline: Micrófono -> Convertir -> Resamplear -> Opus -> AppSink
    let pipeline_desc = "autoaudiosrc ! audioconvert ! audioresample ! opusenc ! rtpopuspay ! appsink name=sink emit-signals=true sync=false";

    
    let pipeline = gst::parse_launch(pipeline_desc)?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "Error casteando pipeline audio encoder")?;

    let appsink = pipeline
        .by_name("sink")
        .ok_or("No appsink")?
        .downcast::<AppSink>()
        .map_err(|_| "No AppSink")?;

    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let tx = Arc::new(Mutex::new(tx));

    appsink.set_callbacks(
        gstreamer_app::AppSinkCallbacks::builder()
            .new_sample(move |sink| {
                if muted.load(Ordering::Relaxed) {
                    let _ = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    return Ok(gst::FlowSuccess::Ok);
                }
                let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
                let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;

                if let Ok(sender) = tx.lock() {
                    let _ = sender.send(map.as_slice().to_vec());
                } else {
                    return Err(gst::FlowError::Error);
                }

                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );

    pipeline.set_state(gst::State::Playing)?;

    // Mantenemos vivo el pipeline
    std::thread::spawn(move || {
        let bus = match pipeline.bus() {
            Some(b) => b,
            None => {
                eprintln!("[Audio Encoder] Error: No se pudo obtener el bus del pipeline.");
                return;
            }
        };
        while !shutdown.load(Ordering::Relaxed) {
            if bus.timed_pop(gst::ClockTime::from_mseconds(100)).is_some() {
                // Aquí podrías manejar mensajes de error o EOS si quieres
            }
        }
        let _ = pipeline.set_state(gst::State::Null);
    });

    Ok(rx)
}

/// arranca reproducción: Appsrc -> Opus Dec -> Convert -> Auto Sink
pub fn start_audio_player(
    shutdown: Arc<AtomicBool>,
) -> Result<Sender<Vec<u8>>, Box<dyn std::error::Error>> {
    let pipeline_desc = "appsrc name=insrc format=time is-live=true ! application/x-rtp,media=audio,encoding-name=OPUS,payload=97 ! rtpopusdepay ! opusdec ! audioconvert ! audioresample ! autoaudiosink sync=false";

    let pipeline = gst::parse_launch(pipeline_desc)?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "Error casteando pipeline audio player")?;

    let appsrc = pipeline
        .by_name("insrc")
        .ok_or("No appsrc")?
        .downcast::<AppSrc>()
        .map_err(|_| "No AppSrc")?;

    appsrc.set_property("do-timestamp", true);

    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();

    pipeline.set_state(gst::State::Playing)?;

    // Hilo de control y alimentación de datos
    std::thread::spawn(move || {
        let bus = match pipeline.bus() {
            Some(b) => b,
            None => {
                eprintln!("[Audio Player] Error: No se pudo obtener el bus del pipeline.");
                return;
            }
        };

        // Hilo interno para empujar datos al appsrc
        let src_clone = appsrc.clone();
        std::thread::spawn(move || {
            for data in rx {
                let buffer = gst::Buffer::from_slice(data);
                if src_clone.push_buffer(buffer).is_err() {
                    break;
                }
            }
        });

        // Loop para detectar errores
        while !shutdown.load(Ordering::Relaxed) {
            if let Some(msg) = bus.timed_pop(gst::ClockTime::from_mseconds(100)) {
                match msg.view() {
                    MessageView::Error(err) => {
                        eprintln!("[AUDIO PLAYER ERROR] {}", err.error());
                        eprintln!(
                            "[AUDIO PLAYER DEBUG] {}",
                            err.debug().unwrap_or("sin debug".into())
                        );
                        break;
                    }
                    MessageView::Eos(..) => break,
                    _ => (),
                }
            }
        }
        let _ = pipeline.set_state(gst::State::Null);
    });

    Ok(tx)
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use std::thread;

    // Helper para inicializar GStreamer una sola vez
    fn init_gst() {
        let _ = gst::init();
    }

    #[test]
    fn test_audio_encoder_initialization_and_shutdown() {
        init_gst();
        let muted = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));

        //Inicio encoder
        let result = start_audio_encoder(muted.clone(), shutdown.clone());
        
        // Verifico que no haya error al crear el pipeline
        assert!(result.is_ok(), "El encoder debería iniciar correctamente (verifica que tengas micrófono)");
        let rx = result.unwrap();

        // Intento recibir al menos un paquete de datos (timeout corto)
        let first_packet = rx.recv_timeout(Duration::from_secs(2));
        
        if first_packet.is_err() {
            println!("ADVERTENCIA: No se recibieron datos de audio. ¿Tienes micrófono?");
        } else {
            assert!(first_packet.is_ok());
            let data = first_packet.unwrap();
            assert!(!data.is_empty(), "El vector de audio no debería estar vacío");
        }

        // Shutdown
        shutdown.store(true, Ordering::SeqCst);

        thread::sleep(Duration::from_millis(200));
    }

    #[test]
    fn test_audio_encoder_mute_logic() {
        init_gst();
        let muted = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));

        let rx = match start_audio_encoder(muted.clone(), shutdown.clone()) {
            Ok(r) => r,
            Err(_) => return, 
        };

        // Espero flujo de datos inicial 
        let _ = rx.recv_timeout(Duration::from_secs(1));

        // ACTIVO MUTE
        muted.store(true, Ordering::SeqCst);

        // Limpio el canal (drenar datos viejos que quedaron en buffer)
        while rx.try_recv().is_ok() {}

        // Espero un momento donde DEBERÍAN llegar datos si no estoy muteado
        thread::sleep(Duration::from_millis(500));

        // Verifico que el canal sigue vacío
        assert!(rx.try_recv().is_err(), "No deberíamos recibir datos mientras está muteado");

        // DESACTIVO MUTE
        muted.store(false, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(500));

        // VerificO que vuelven a llegar datos
 
        let packet = rx.recv_timeout(Duration::from_secs(2));
        assert!(packet.is_ok(), "Deberíamos volver a recibir datos al desmutear");

        shutdown.store(true, Ordering::SeqCst);
    }

    #[test]
    fn test_audio_player_initialization() {
        init_gst();
        let shutdown = Arc::new(AtomicBool::new(false));

        //Inicio player
        let result = start_audio_player(shutdown.clone());
        assert!(result.is_ok(), "El player debería iniciar correctamente");
        
        let tx = result.unwrap();

        // Simulo envío de datos
        let dummy_data = vec![0u8; 100]; 
        let send_result = tx.send(dummy_data);
        
        assert!(send_result.is_ok(), "Deberíamos poder enviar datos al canal del player");

        // Shutdown
        shutdown.store(true, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(100));
    }

    #[test]
    fn test_loopback_integration() {
        // Este test conecta la salida del encoder a la entrada del player.
        init_gst();
        
        let muted = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));

        let rx = start_audio_encoder(muted.clone(), shutdown.clone());
        let tx = start_audio_player(shutdown.clone());

        if rx.is_err() || tx.is_err() {
            println!("Saltando test de loopback por falta de dispositivos de audio");
            return;
        }

        let rx = rx.unwrap();
        let tx = tx.unwrap();

        // Simulo bucle de transmisión por 1 segundo
        let start = std::time::Instant::now();
        
        while start.elapsed().as_secs() < 1 {
            if let Ok(data) = rx.recv_timeout(Duration::from_millis(100)) {
                // Paso lo grabado directo al reproductor
                let _ = tx.send(data);
            }
        }

        shutdown.store(true, Ordering::SeqCst);
    }
}