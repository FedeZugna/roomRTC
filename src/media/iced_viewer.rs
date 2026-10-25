use iced::widget::{Button, Image, button, column, container, row, text};
use iced::window;
use iced::{Element, Length, Subscription, Task, Theme};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::constants::{VIDEO_HEIGHT_F32, VIDEO_WIDTH_F32};

#[derive(Debug, Clone)]
pub enum Message {
    FrameUpdate(Instant),
    EndCall,     // Terminar llamada (volver al lobby)
    JoinCall,    // Unirse a la llamada
    SystemClose, // Cerrar ventana (X)
}

pub struct VideoViewer {
    local_frame: Arc<Mutex<Option<Vec<u8>>>>,
    remote_frame: Arc<Mutex<Option<Vec<u8>>>>,
    width: u32,
    height: u32,
    running_call: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    joined: bool,
}

impl VideoViewer {
    pub fn new(
        local_frame: Arc<Mutex<Option<Vec<u8>>>>,
        remote_frame: Arc<Mutex<Option<Vec<u8>>>>,
        width: u32,
        height: u32,
        running_call: Arc<AtomicBool>,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        Self {
            local_frame,
            remote_frame,
            width,
            height,
            running_call,
            shutdown,
            joined: false, // en lobby
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::FrameUpdate(_) => Task::none(),

            Message::JoinCall => {
                self.running_call.store(true, Ordering::SeqCst);
                self.joined = true;
                Task::none()
            }

            Message::EndCall => {
                // Cortamos la llamada, pero NO cerramos el programa
                self.running_call.store(false, Ordering::SeqCst);

                // Limpiamos buffers para no dejar la última imagen congelada
                if let Ok(mut lf) = self.local_frame.lock() {
                    *lf = None;
                }
                if let Ok(mut rf) = self.remote_frame.lock() {
                    *rf = None;
                }

                self.joined = false; // Volvemos al lobby
                Task::none()
            }

            Message::SystemClose => {
                // Cierre total: matamos llamada + hilos y salimos
                self.running_call.store(false, Ordering::SeqCst);
                self.shutdown.store(true, Ordering::SeqCst);
                iced::exit()
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        // Si todavía no se unió, mostramos el lobby
        if !self.joined {
            let join_button: Button<Message> = button(text("Unirse a la llamada"))
                .padding(10)
                .on_press(Message::JoinCall);

            let content = column![
                text("Sala de espera").size(24),
                text("Cuando estés listo, unite a la llamada.").size(16),
                join_button,
            ]
            .spacing(20)
            .padding(20);

            return container(content)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into();
        }

        // --- A partir de acá dejás TODO como lo tenías (cámaras local/remota + botón "Finalizar llamada") ---
        let local_widget = if let Ok(frame_lock) = self.local_frame.lock() {
            if let Some(rgba_data) = frame_lock.as_ref() {
                let handle = iced::advanced::image::Handle::from_rgba(
                    self.width,
                    self.height,
                    rgba_data.clone(),
                );
                column![
                    text("Camara local").size(16),
                    Image::new(handle)
                        .width(Length::Fixed(VIDEO_WIDTH_F32))
                        .height(Length::Fixed(VIDEO_HEIGHT_F32)),
                ]
                .spacing(5)
            } else {
                column![
                    text("Camara local").size(16),
                    text("Esperando video...").size(14)
                ]
                .spacing(5)
            }
        } else {
            column![
                text("Camara local").size(16),
                text("Error de mutex").size(14)
            ]
            .spacing(5)
        };

        let remote_widget = if let Ok(frame_lock) = self.remote_frame.lock() {
            if let Some(rgba_data) = frame_lock.as_ref() {
                let handle = iced::advanced::image::Handle::from_rgba(
                    self.width,
                    self.height,
                    rgba_data.clone(),
                );
                column![
                    text("Camara remota").size(16),
                    Image::new(handle)
                        .width(Length::Fixed(VIDEO_WIDTH_F32))
                        .height(Length::Fixed(VIDEO_HEIGHT_F32)),
                ]
                .spacing(5)
            } else {
                column![
                    text("Camara remota").size(16),
                    text("Esperando video...").size(14)
                ]
                .spacing(5)
            }
        } else {
            column![
                text("Camara remota").size(16),
                text("Error de mutex").size(14)
            ]
            .spacing(5)
        };

        let videos_row = row![
            container(local_widget)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
            container(remote_widget)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        ]
        .spacing(10)
        .padding(10);

        let end_button: Button<Message> = button(text("Finalizar llamada"))
            .padding(10)
            .on_press(Message::EndCall);

        let content = column![videos_row, end_button].spacing(10).padding(10);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            window::frames().map(Message::FrameUpdate),
            window::close_events().map(|_| Message::SystemClose),
        ])
    }

    pub fn theme(&self) -> Theme {
        Theme::Dark
    }
}

pub fn run_viewer(
    local_frame: Arc<Mutex<Option<Vec<u8>>>>,
    remote_frame: Arc<Mutex<Option<Vec<u8>>>>,
    width: u32,
    height: u32,
    running_call: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
) -> iced::Result {
    let running_for_viewer = Arc::clone(&running_call);
    let shutdown_for_viewer = Arc::clone(&shutdown);

    iced::application("Ventana Iced", VideoViewer::update, VideoViewer::view)
        .subscription(VideoViewer::subscription)
        .theme(VideoViewer::theme)
        .run_with(move || {
            (
                VideoViewer::new(
                    local_frame,
                    remote_frame,
                    width,
                    height,
                    running_for_viewer,
                    shutdown_for_viewer,
                ),
                Task::none(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    type SetupResult = (
        VideoViewer,
        Arc<AtomicBool>,             // running_call
        Arc<AtomicBool>,             // shutdown
        Arc<Mutex<Option<Vec<u8>>>>, // local_frame
        Arc<Mutex<Option<Vec<u8>>>>, // remote_frame
    );

    // Helper para configurar el entorno de prueba
    // Retorna el Viewer y referencias a las variables compartidas para verificar efectos
    fn setup() -> SetupResult {
        let local_frame = Arc::new(Mutex::new(None));
        let remote_frame = Arc::new(Mutex::new(None));
        let running_call = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));

        let viewer = VideoViewer::new(
            local_frame.clone(),
            remote_frame.clone(),
            640,
            480,
            running_call.clone(),
            shutdown.clone(),
        );

        (viewer, running_call, shutdown, local_frame, remote_frame)
    }

    #[test]
    fn test_initial_state_is_lobby() {
        let (viewer, running_call, _, _, _) = setup();

        // Verifico estado interno
        assert!(!viewer.joined, "Debería iniciar en false (Lobby)");

        // Verifico estado compartido
        assert!(
            !running_call.load(Ordering::SeqCst),
            "La llamada no debería estar corriendo al inicio"
        );
    }

    #[test]
    fn test_join_call_starts_transmission() {
        let (mut viewer, running_call, _, _, _) = setup();

        //  Envio mensaje JoinCall
        let _ = viewer.update(Message::JoinCall);

        // El estado interno debe cambiar
        assert!(viewer.joined, "Debería haber cambiado a true (Video View)");

        // El flag compartido debe activarse
        assert!(
            running_call.load(Ordering::SeqCst),
            "running_call debería ser true"
        );
    }

    #[test]
    fn test_end_call_stops_transmission_and_clears_buffers() {
        let (mut viewer, running_call, _, local_frame, _) = setup();

        // Simulo que ya estamos en llamada y hay video
        viewer.joined = true;
        running_call.store(true, Ordering::SeqCst);
        {
            let mut guard = local_frame.lock().unwrap();
            *guard = Some(vec![255, 0, 0, 255]); // Datos falsos de video
        }

        // Usuario corta la llamada
        let _ = viewer.update(Message::EndCall);

        // Estado interno vuelve a lobby
        assert!(!viewer.joined, "Debería volver al lobby (false)");

        // Se detiene la transmisión
        assert!(
            !running_call.load(Ordering::SeqCst),
            "running_call debería pasar a false"
        );

        // Los buffers deben limpiarse para no mostrar el último frame congelado
        let guard = local_frame.lock().unwrap();
        assert!(
            guard.is_none(),
            "El buffer local debería haberse limpiado (None)"
        );
    }

    #[test]
    fn test_system_close_triggers_shutdown() {
        let (mut viewer, running_call, shutdown, _, _) = setup();

        // Simulo que estoy en llamada
        viewer.joined = true;
        running_call.store(true, Ordering::SeqCst);

        // Usuario cierra la ventana con la X
        let _ = viewer.update(Message::SystemClose);

        // Debe cortar la llamada
        assert!(
            !running_call.load(Ordering::SeqCst),
            "Debe detener la llamada antes de salir"
        );

        // Debe activar el shutdown global
        assert!(
            shutdown.load(Ordering::SeqCst),
            "Debe activar el flag de shutdown"
        );
    }
}
