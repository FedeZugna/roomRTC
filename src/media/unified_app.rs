// Aplicación unificada que maneja: Login -> Register -> Lobby -> Video
// Todo en una sola instancia de Iced para evitar el error RecreationAttempt

use super::constants::{VIDEO_HEIGHT_F32, VIDEO_WIDTH_F32};
use super::login_viewer::{LoginAction, LoginMessage, LoginState};
use super::register_viewer::{RegisterAction, RegisterMessage, RegisterState};
use crate::chat::{ChatAuthor, ChatEntry, ChatLimits, ChatState, prepare_send};
use iced::widget::{Image, button, column, container, row, scrollable, text, text_input};
use iced::{Element, Font, Length, Subscription, Task, Theme, window};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

const BOOTSTRAP_FONT: Font = Font::with_name("bootstrap-icons");
const ICON_REFRESH: &str = "\u{F116}";
const ICON_AVAILABLE: &str = "\u{F1D5}";
const ICON_BUSY: &str = "\u{F63C}";
const ICON_DISCONNECTED: &str = "\u{F6B6}";

// Re-exportamos tipos necesarios del cliente
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum UserStatus {
    Disconnected,
    Available,
    Busy,
}

#[derive(Debug, Clone)]
pub enum SignalingCommand {
    GetUserList,
    CallRequest { callee: String },
    AcceptCall { caller: String },
    RejectCall { caller: String },
    EndCall,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SctpCommand {
    SendFile {
        path: String,
    },
    AcceptFile {
        filename: String,
    },
    RejectFile {
        filename: String,
    },
    /// Mensaje de chat propio (id local) de la llamada `generation`.
    SendChat {
        id: u64,
        generation: u64,
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SctpNotification {
    FileOffer {
        filename: String,
        size: usize,
    },
    FileTransferComplete {
        filename: String,
    },
    FileTransferFailed {
        filename: String,
        error: String,
    },
    /// Mensaje de chat recibido durante la llamada `generation`.
    ChatReceived {
        generation: u64,
        text: String,
    },
    /// El transporte no pudo escribir el mensaje propio `id`.
    ChatSendFailed {
        id: u64,
        generation: u64,
    },
}

#[derive(Debug, Clone)]
pub struct FileTransferOffer {
    pub filename: String,
    pub size: usize,
}

#[derive(Debug, Clone)]
pub enum AppMessage {
    // Auth messages
    Login(LoginMessage),
    Register(RegisterMessage),
    AuthTick,

    // Lobby messages
    RequestUserList,
    UpdateUserList(Vec<(String, UserStatus)>),
    CallUserByClick { username: String },
    IncomingCall { caller: String },
    AcceptCall,
    RejectCall,
    CallWasAccepted, // Nuevo: cuando el otro acepta nuestra llamada
    RemoteCallEnded, // Nuevo: cuando el otro termina la llamada
    LobbyTick,

    // Video messages
    FrameUpdate(Instant),
    JoinCall,
    EndCall,
    FilePathChanged(String),
    PickFile,
    SendFile,
    SystemClose,
    ToggleAudio,

    // File transfer messages
    IncomingFileOffer { filename: String, size: usize },
    AcceptFileTransfer { filename: String },
    RejectFileTransfer { filename: String },
    FileTransferComplete { filename: String },
    FileTransferFailed { filename: String, error: String },

    // Chat messages
    ChatDraftChanged(String),
    ChatSend,
    ChatReceived { generation: u64, text: String },
    ChatSendFailed { id: u64, generation: u64 },
}

pub enum AppScreen {
    Login(LoginState),
    Register(RegisterState),
    Lobby {
        users: Vec<(String, UserStatus)>,
        incoming_call: Option<String>, // caller username
    },
    VideoCall {
        joined: bool,
        file_path: String,
        pending_file_offer: Option<FileTransferOffer>,
    },
}

pub struct UnifiedApp {
    screen: AppScreen,

    // Auth state
    pending_auth: Arc<Mutex<Option<PendingAuth>>>,
    auth_result: Arc<Mutex<Option<Result<String, String>>>>,
    current_username: Option<String>,

    // Signaling command channel
    signaling_tx: Option<Sender<SignalingCommand>>,
    signaling_response: Arc<Mutex<Option<SignalingResponse>>>,
    sctp_tx: Option<Sender<SctpCommand>>,
    sctp_notif_rx: Arc<Mutex<std::sync::mpsc::Receiver<SctpNotification>>>,

    // Video state
    local_frame: Arc<Mutex<Option<Vec<u8>>>>,
    remote_frame: Arc<Mutex<Option<Vec<u8>>>>,
    width: u32,
    height: u32,
    running_call: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,

    audio_muted_signal: Arc<AtomicBool>,
    is_muted: bool,

    // Chat de la llamada actual
    chat: ChatState,
    call_generation: Arc<AtomicU64>,
}

#[derive(Debug, Clone)]
pub enum SignalingResponse {
    UserList(Vec<(String, UserStatus)>),
    IncomingCall { caller: String },
    CallAccepted,
    CallRejected,
    CallEnded, // Nuevo: cuando el otro termina la llamada
    Error(String),
}

#[derive(Debug, Clone)]
pub struct PendingAuth {
    pub username: String,
    pub password: String,
    pub email: Option<String>,
    pub is_register: bool,
}

/// Configuration struct to bundle UnifiedApp parameters
pub struct UnifiedAppConfig {
    pub pending_auth: Arc<Mutex<Option<PendingAuth>>>,
    pub auth_result: Arc<Mutex<Option<Result<String, String>>>>,
    pub signaling_tx: Option<Sender<SignalingCommand>>,
    pub signaling_response: Arc<Mutex<Option<SignalingResponse>>>,
    pub sctp_tx: Option<Sender<SctpCommand>>,
    pub sctp_notif_rx: Arc<Mutex<std::sync::mpsc::Receiver<SctpNotification>>>,
    pub local_frame: Arc<Mutex<Option<Vec<u8>>>>,
    pub remote_frame: Arc<Mutex<Option<Vec<u8>>>>,
    pub width: u32,
    pub height: u32,
    pub running_call: Arc<AtomicBool>,
    /// Generación de la llamada actual, compartida con los hilos de red.
    pub call_generation: Arc<AtomicU64>,
    pub shutdown: Arc<AtomicBool>,
    pub audio_muted_signal: Arc<AtomicBool>,
}

impl UnifiedApp {
    pub fn new(config: UnifiedAppConfig) -> Self {
        Self {
            screen: AppScreen::Login(LoginState::new()),
            pending_auth: config.pending_auth,
            auth_result: config.auth_result,
            current_username: None,
            signaling_tx: config.signaling_tx,
            signaling_response: config.signaling_response,
            sctp_tx: config.sctp_tx,
            sctp_notif_rx: config.sctp_notif_rx,
            local_frame: config.local_frame,
            remote_frame: config.remote_frame,
            width: config.width,
            height: config.height,
            running_call: config.running_call,
            shutdown: config.shutdown,
            audio_muted_signal: config.audio_muted_signal,
            is_muted: false,
            chat: ChatState::default(),
            call_generation: config.call_generation,
        }
    }

    pub fn update(&mut self, message: AppMessage) -> Task<AppMessage> {
        match message {
            // === LOGIN ===
            AppMessage::Login(login_msg) => {
                if let AppScreen::Login(state) = &mut self.screen
                    && let Some(action) = state.update(login_msg)
                {
                    match action {
                        LoginAction::Login { username, password } => {
                            // Guardar el username inmediatamente
                            self.current_username = Some(username.clone());
                            *self.pending_auth.lock().unwrap() = Some(PendingAuth {
                                username,
                                password,
                                email: None,
                                is_register: false,
                            });
                        }
                        LoginAction::SwitchToRegister => {
                            self.screen = AppScreen::Register(RegisterState::new());
                        }
                    }
                }
                Task::none()
            }

            // === REGISTER ===
            AppMessage::Register(register_msg) => {
                if let AppScreen::Register(state) = &mut self.screen
                    && let Some(action) = state.update(register_msg)
                {
                    match action {
                        RegisterAction::Register {
                            username,
                            password,
                            email,
                        } => {
                            // Guardar el username inmediatamente
                            self.current_username = Some(username.clone());
                            *self.pending_auth.lock().unwrap() = Some(PendingAuth {
                                username,
                                password,
                                email,
                                is_register: true,
                            });
                        }
                        RegisterAction::BackToLogin => {
                            self.screen = AppScreen::Login(LoginState::new());
                        }
                    }
                }
                Task::none()
            }

            // === AUTH TICK ===
            AppMessage::AuthTick => {
                // verifico resultados de autenticación
                if let Some(result) = self.auth_result.lock().unwrap().take() {
                    match result {
                        Ok(msg) => {
                            println!("[Auth] Success: {}", msg);
                            // Guardar el username del usuario actual
                            if let Some(pending) = self.pending_auth.lock().unwrap().as_ref() {
                                let username = pending.username.clone();
                                println!("[Auth] Guardando username actual: '{}'", username);
                                self.current_username = Some(username);
                            }
                            self.screen = AppScreen::Lobby {
                                users: Vec::new(),
                                incoming_call: None,
                            };
                        }
                        Err(error) => {
                            println!("[Auth] Error: {}", error);
                            match &mut self.screen {
                                AppScreen::Login(state) => state.set_error(error),
                                AppScreen::Register(state) => state.set_error(error),
                                _ => {}
                            }
                        }
                    }
                }
                Task::none()
            }

            // === LOBBY ===
            AppMessage::RequestUserList => {
                if let Some(tx) = &self.signaling_tx {
                    let _ = tx.send(SignalingCommand::GetUserList);
                }
                Task::none()
            }

            AppMessage::UpdateUserList(users) => {
                if let AppScreen::Lobby {
                    users: user_list, ..
                } = &mut self.screen
                {
                    // Filtrar el usuario actual de la lista antes de almacenarla
                    let filtered_users: Vec<(String, UserStatus)> =
                        if let Some(ref current) = self.current_username {
                            users
                                .into_iter()
                                .filter(|(username, _)| username.trim() != current.trim())
                                .collect()
                        } else {
                            users
                        };

                    *user_list = filtered_users;
                }
                Task::none()
            }

            AppMessage::CallUserByClick { username } => {
                println!("[Lobby] Llamando a {} (click)", username);
                if let Some(tx) = &self.signaling_tx {
                    let _ = tx.send(SignalingCommand::CallRequest { callee: username });
                }
                Task::none()
            }

            AppMessage::IncomingCall { caller } => {
                if let AppScreen::Lobby { incoming_call, .. } = &mut self.screen {
                    *incoming_call = Some(caller);
                }
                Task::none()
            }

            AppMessage::AcceptCall => {
                // Get caller from incoming_call
                let caller = if let AppScreen::Lobby { incoming_call, .. } = &self.screen {
                    incoming_call.clone()
                } else {
                    None
                };

                if let Some(caller_name) = caller {
                    // Send accept command to signaling thread
                    if let Some(ref tx) = self.signaling_tx {
                        let _ = tx.send(SignalingCommand::AcceptCall {
                            caller: caller_name,
                        });
                    }

                    // Accept the call and transition to video
                    self.running_call.store(true, Ordering::SeqCst);
                    self.start_call_chat();
                    self.screen = AppScreen::VideoCall {
                        joined: true,
                        file_path: String::new(),
                        pending_file_offer: None,
                    };
                }
                Task::none()
            }

            AppMessage::RejectCall => {
                // Get caller from incoming_call
                let caller = if let AppScreen::Lobby { incoming_call, .. } = &mut self.screen {
                    incoming_call.take()
                } else {
                    None
                };

                if let Some(caller_name) = caller {
                    // Send reject command to signaling thread
                    if let Some(ref tx) = self.signaling_tx {
                        let _ = tx.send(SignalingCommand::RejectCall {
                            caller: caller_name,
                        });
                    }
                }

                // Clear incoming call
                if let AppScreen::Lobby { incoming_call, .. } = &mut self.screen {
                    *incoming_call = None;
                }
                Task::none()
            }

            AppMessage::CallWasAccepted => {
                // El otro usuario aceptó nuestra llamada, iniciamos video
                println!("[App] Llamada aceptada! Iniciando video...");
                self.running_call.store(true, Ordering::SeqCst);
                self.start_call_chat();
                self.screen = AppScreen::VideoCall {
                    joined: true,
                    file_path: String::new(),
                    pending_file_offer: None,
                };
                Task::none()
            }

            AppMessage::LobbyTick => {
                // Check for signaling responses
                if let Some(response) = self.signaling_response.lock().unwrap().take() {
                    match response {
                        SignalingResponse::UserList(users) => {
                            return Task::done(AppMessage::UpdateUserList(users));
                        }
                        SignalingResponse::IncomingCall { caller } => {
                            return Task::done(AppMessage::IncomingCall { caller });
                        }
                        SignalingResponse::CallAccepted => {
                            return Task::done(AppMessage::CallWasAccepted);
                        }
                        SignalingResponse::CallRejected => {
                            println!("[Lobby] El usuario rechazó la llamada");
                            // Podrías mostrar un mensaje en la UI si quieres
                        }
                        SignalingResponse::CallEnded => {
                            return Task::done(AppMessage::RemoteCallEnded);
                        }
                        SignalingResponse::Error(msg) => {
                            println!("[Lobby] Error: {}", msg);
                        }
                    }
                }
                Task::none()
            }

            // === VIDEO CALL ===
            AppMessage::FrameUpdate(_) => {
                // Durante la videollamada, también verificar mensajes de señalización
                if let Some(SignalingResponse::CallEnded) =
                    self.signaling_response.lock().unwrap().take()
                {
                    return Task::done(AppMessage::RemoteCallEnded);
                }

                // Verificar notificaciones SCTP (una por tick). Se suelta el lock
                // antes de procesarla.
                let notif = self
                    .sctp_notif_rx
                    .try_lock()
                    .ok()
                    .and_then(|guard| guard.try_recv().ok());
                if let Some(notif) = notif {
                    match notif {
                        SctpNotification::FileOffer { filename, size } => {
                            return Task::done(AppMessage::IncomingFileOffer { filename, size });
                        }
                        SctpNotification::FileTransferComplete { filename } => {
                            return Task::done(AppMessage::FileTransferComplete { filename });
                        }
                        SctpNotification::FileTransferFailed { filename, error } => {
                            return Task::done(AppMessage::FileTransferFailed { filename, error });
                        }
                        // El chat se procesa en el mismo tick
                        SctpNotification::ChatReceived { generation, text } => {
                            return self.update(AppMessage::ChatReceived { generation, text });
                        }
                        SctpNotification::ChatSendFailed { id, generation } => {
                            return self.update(AppMessage::ChatSendFailed { id, generation });
                        }
                    }
                }

                Task::none()
            }

            AppMessage::JoinCall => {
                // desde lobby, iniciar la videollamada
                self.running_call.store(true, Ordering::SeqCst);
                self.start_call_chat();
                self.screen = AppScreen::VideoCall {
                    joined: true,
                    file_path: String::new(),
                    pending_file_offer: None,
                };
                Task::none()
            }

            AppMessage::EndCall => {
                // Enviar comando al servidor para terminar la llamada
                if let Some(ref tx) = self.signaling_tx {
                    let _ = tx.send(SignalingCommand::EndCall);
                }

                // termina la llamada, al lobby
                self.running_call.store(false, Ordering::SeqCst);
                self.chat.reset();
                if let Ok(mut lf) = self.local_frame.lock() {
                    *lf = None;
                }
                if let Ok(mut rf) = self.remote_frame.lock() {
                    *rf = None;
                }
                self.screen = AppScreen::Lobby {
                    users: Vec::new(),
                    incoming_call: None,
                };
                Task::none()
            }

            AppMessage::RemoteCallEnded => {
                // El otro participante terminó la llamada, volvemos al lobby
                println!("[App] El otro usuario terminó la llamada");
                self.running_call.store(false, Ordering::SeqCst);
                self.chat.reset();
                if let Ok(mut lf) = self.local_frame.lock() {
                    *lf = None;
                }
                if let Ok(mut rf) = self.remote_frame.lock() {
                    *rf = None;
                }
                self.screen = AppScreen::Lobby {
                    users: Vec::new(),
                    incoming_call: None,
                };
                Task::none()
            }

            // === CHAT ===
            AppMessage::ChatDraftChanged(draft) => {
                if let AppScreen::VideoCall { .. } = self.screen {
                    self.chat.draft = draft;
                }
                Task::none()
            }
            AppMessage::ChatSend => {
                // Llamada activa: la pantalla es VideoCall. Canal listo: hay
                // canal de comandos y running_call (design D6).
                let in_call = matches!(self.screen, AppScreen::VideoCall { .. });
                let channel_ready =
                    self.sctp_tx.is_some() && self.running_call.load(Ordering::SeqCst);
                let draft = self.chat.draft.clone();
                if let Ok(out) = prepare_send(
                    &mut self.chat,
                    &draft,
                    in_call,
                    channel_ready,
                    ChatLimits::PRODUCTION,
                ) {
                    let cmd = SctpCommand::SendChat {
                        id: out.id,
                        generation: self.call_generation.load(Ordering::SeqCst),
                        text: out.text,
                    };
                    let sent = self.sctp_tx.as_ref().is_some_and(|tx| tx.send(cmd).is_ok());
                    if !sent {
                        // El hilo SCTP ya no recibe comandos: no se pierde en silencio
                        self.chat.mark_failed(out.id);
                    }
                    self.chat.draft.clear();
                }
                Task::none()
            }
            AppMessage::ChatReceived { generation, text } => {
                // Lo que no es de la llamada actual se descarta (design D2)
                if self.is_current_call(generation) {
                    self.chat
                        .push_peer(&text, ChatLimits::PRODUCTION.history_max);
                }
                Task::none()
            }
            AppMessage::ChatSendFailed { id, generation } => {
                if self.is_current_call(generation) {
                    self.chat.mark_failed(id);
                }
                Task::none()
            }

            AppMessage::FilePathChanged(path) => {
                if let AppScreen::VideoCall { file_path, .. } = &mut self.screen {
                    *file_path = path;
                }
                Task::none()
            }

            AppMessage::PickFile => {
                // Abrir el explorador de archivos
                Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Selecciona un archivo para enviar")
                            .pick_file()
                            .await
                    },
                    |file_handle| {
                        if let Some(file) = file_handle {
                            AppMessage::FilePathChanged(file.path().to_string_lossy().to_string())
                        } else {
                            AppMessage::FilePathChanged(String::new())
                        }
                    },
                )
            }

            AppMessage::SendFile => {
                if let AppScreen::VideoCall { file_path, .. } = &self.screen
                    && let Some(tx) = &self.sctp_tx
                {
                    let _ = tx.send(SctpCommand::SendFile {
                        path: file_path.clone(),
                    });
                }

                Task::none()
            }

            AppMessage::SystemClose => {
                // Full shutdown
                self.running_call.store(false, Ordering::SeqCst);
                self.shutdown.store(true, Ordering::SeqCst);
                iced::exit()
            }

            AppMessage::ToggleAudio => {
                self.is_muted = !self.is_muted; //invertir el estado visual
                self.audio_muted_signal
                    .store(self.is_muted, Ordering::Relaxed); //aviso a GStreamer que esta muteado

                Task::none()
            }

            AppMessage::IncomingFileOffer { filename, size } => {
                // Store the pending file offer
                if let AppScreen::VideoCall {
                    pending_file_offer, ..
                } = &mut self.screen
                {
                    *pending_file_offer = Some(FileTransferOffer {
                        filename: filename.clone(),
                        size,
                    });
                    println!(
                        "[App] Oferta de archivo recibida: {} ({} bytes)",
                        filename, size
                    );
                }
                Task::none()
            }

            AppMessage::AcceptFileTransfer { filename } => {
                // Send accept command via SCTP
                if let Some(tx) = &self.sctp_tx {
                    let _ = tx.send(SctpCommand::AcceptFile {
                        filename: filename.clone(),
                    });
                }

                // Clear the pending offer
                if let AppScreen::VideoCall {
                    pending_file_offer, ..
                } = &mut self.screen
                {
                    *pending_file_offer = None;
                }
                Task::none()
            }

            AppMessage::RejectFileTransfer { filename } => {
                // Send reject command via SCTP
                if let Some(tx) = &self.sctp_tx {
                    let _ = tx.send(SctpCommand::RejectFile {
                        filename: filename.clone(),
                    });
                }

                // Clear the pending offer
                if let AppScreen::VideoCall {
                    pending_file_offer, ..
                } = &mut self.screen
                {
                    *pending_file_offer = None;
                }
                Task::none()
            }

            AppMessage::FileTransferComplete { filename } => {
                println!("[App] Transferencia completada: {}", filename);
                Task::none()
            }

            AppMessage::FileTransferFailed { filename, error } => {
                eprintln!("[App] Transferencia fallida: {} - {}", filename, error);
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, AppMessage> {
        match &self.screen {
            AppScreen::Login(state) => state.view().map(AppMessage::Login),
            AppScreen::Register(state) => state.view().map(AppMessage::Register),
            AppScreen::Lobby {
                users,
                incoming_call,
            } => self.view_lobby(users, incoming_call),
            AppScreen::VideoCall {
                joined,
                file_path,
                pending_file_offer,
            } => self.view_video(*joined, file_path, pending_file_offer),
        }
    }

    fn view_lobby(
        &self,
        users: &[(String, UserStatus)],
        incoming_call: &Option<String>,
    ) -> Element<'_, AppMessage> {
        // Si hay una llamada entrante, mostrar diálogo
        if let Some(caller) = incoming_call {
            let content = column![
                text("Llamada entrante").size(24),
                text(format!("{} te está llamando", caller)).size(16),
                row![
                    button(text("Aceptar"))
                        .padding(10)
                        .style(|_theme: &Theme, _status| {
                            button::Style {
                                background: Some(iced::Background::Color(iced::Color::from_rgb(
                                    0.2, 0.7, 0.3,
                                ))),
                                text_color: iced::Color::WHITE,
                                border: iced::Border {
                                    color: iced::Color::from_rgb(0.3, 0.8, 0.4),
                                    width: 1.0,
                                    radius: 10.0.into(),
                                },
                                ..Default::default()
                            }
                        })
                        .on_press(AppMessage::AcceptCall),
                    button(text("Rechazar"))
                        .padding(10)
                        .style(|_theme: &Theme, _status| {
                            button::Style {
                                background: Some(iced::Background::Color(iced::Color::from_rgb(
                                    0.8, 0.2, 0.2,
                                ))),
                                text_color: iced::Color::WHITE,
                                border: iced::Border {
                                    color: iced::Color::from_rgb(0.9, 0.3, 0.3),
                                    width: 1.0,
                                    radius: 10.0.into(),
                                },
                                ..Default::default()
                            }
                        })
                        .on_press(AppMessage::RejectCall),
                ]
                .spacing(10),
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

        // User info for top right corner
        let current_user_text = if let Some(ref username) = self.current_username {
            format!("Usuario: {}", username)
        } else {
            "Usuario: desconocido".to_string()
        };

        // Lista de usuarios para llamar
        let refresh_icon_button = button(text(ICON_REFRESH).font(BOOTSTRAP_FONT).size(20))
            .padding(10)
            .style(|_theme: &Theme, _status| button::Style {
                background: None,
                text_color: iced::Color::WHITE,
                border: iced::Border {
                    color: iced::Color::TRANSPARENT,
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .on_press(AppMessage::RequestUserList);

        let users_header = row![text("Usuarios:").size(20), refresh_icon_button]
            .width(Length::Fill)
            .align_y(iced::alignment::Vertical::Center);

        let mut user_list_column = column![users_header].spacing(8);

        if users.is_empty() {
            user_list_column = user_list_column
                .push(text("No hay usuarios conectados. Presiona actualizar.").size(14));
        } else {
            for (username, status) in users {
                let (status_text, status_icon, status_color) = match status {
                    UserStatus::Available => (
                        "Disponible",
                        ICON_AVAILABLE,
                        iced::Color::from_rgb(0.2, 0.8, 0.3),
                    ),
                    UserStatus::Busy => {
                        ("Ocupado", ICON_BUSY, iced::Color::from_rgb(1.0, 0.7, 0.0))
                    }
                    UserStatus::Disconnected => (
                        "Desconectado",
                        ICON_DISCONNECTED,
                        iced::Color::from_rgb(0.8, 0.3, 0.3),
                    ),
                };

                let user_content = row![
                    text(status_icon)
                        .font(BOOTSTRAP_FONT)
                        .size(16)
                        .color(status_color),
                    text(format!("{} - {}", username, status_text)).size(16)
                ]
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center);

                // Solo los usuarios disponibles son clickeables para evitar errores
                let user_button = if matches!(status, UserStatus::Available) {
                    button(user_content)
                        .padding(10)
                        .width(Length::Fill)
                        .style(|_theme: &Theme, _status| button::Style {
                            background: Some(iced::Background::Color(iced::Color::from_rgb(
                                0.2, 0.6, 0.9,
                            ))),
                            text_color: iced::Color::WHITE,
                            border: iced::Border {
                                color: iced::Color::from_rgb(0.3, 0.7, 1.0),
                                width: 1.0,
                                radius: 8.0.into(),
                            },
                            ..Default::default()
                        })
                        .on_press(AppMessage::CallUserByClick {
                            username: username.clone(),
                        })
                } else {
                    // Usuario no disponible - botón deshabilitado
                    button(user_content).padding(10).width(Length::Fill).style(
                        |_theme: &Theme, _status| button::Style {
                            background: Some(iced::Background::Color(iced::Color::from_rgb(
                                0.3, 0.3, 0.3,
                            ))),
                            text_color: iced::Color::from_rgb(0.6, 0.6, 0.6),
                            border: iced::Border {
                                color: iced::Color::from_rgb(0.4, 0.4, 0.4),
                                width: 1.0,
                                radius: 8.0.into(),
                            },
                            ..Default::default()
                        },
                    )
                };

                user_list_column = user_list_column.push(user_button);
            }
        }

        let user_list_container = container(user_list_column).width(Length::Fill).padding(20);

        // Header with title and user info
        let header = row![
            text("Sala de espera").size(32),
            container(text("")).width(Length::Fill), // Spacer
            container(text(current_user_text).size(16))
                .padding(10)
                .style(|_theme: &Theme| {
                    container::Style {
                        background: Some(iced::Background::Color(iced::Color::from_rgba(
                            0.2, 0.2, 0.2, 0.8,
                        ))),
                        border: iced::Border {
                            color: iced::Color::from_rgb(0.4, 0.4, 0.4),
                            width: 1.0,
                            radius: 8.0.into(),
                        },
                        ..Default::default()
                    }
                }),
        ]
        .spacing(10)
        .padding(20)
        .align_y(iced::alignment::Vertical::Center);

        // Contenido principal del lobby
        let main_content = column![header, user_list_container,].width(Length::Fill);

        container(main_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_video(
        &self,
        _joined: bool,
        file_path: &str,
        pending_file_offer: &Option<FileTransferOffer>,
    ) -> Element<'_, AppMessage> {
        let local_img = self
            .local_frame
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|data| data.clone()));

        let remote_img = self
            .remote_frame
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|data| data.clone()));

        let local_view = if let Some(data) = local_img {
            let handle = Image::new(iced::widget::image::Handle::from_rgba(
                self.width,
                self.height,
                data,
            ));
            container(handle.width(VIDEO_WIDTH_F32).height(VIDEO_HEIGHT_F32))
                .width(VIDEO_WIDTH_F32)
                .height(VIDEO_HEIGHT_F32)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
        } else {
            container(text("Local (esperando...)").size(12))
                .width(VIDEO_WIDTH_F32)
                .height(VIDEO_HEIGHT_F32)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
        };

        let remote_view = if let Some(data) = remote_img {
            let handle = Image::new(iced::widget::image::Handle::from_rgba(
                self.width,
                self.height,
                data,
            ));
            container(handle.width(VIDEO_WIDTH_F32).height(VIDEO_HEIGHT_F32))
                .width(VIDEO_WIDTH_F32)
                .height(VIDEO_HEIGHT_F32)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
        } else {
            container(text("Remoto (esperando...)").size(12))
                .width(VIDEO_WIDTH_F32)
                .height(VIDEO_HEIGHT_F32)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
        };

        /*let end_button: Button<AppMessage> = button(text("Finalizar llamada"))
            .padding(10)
            .style(|_theme: &Theme, _status| button::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgb(
                    0.8, 0.2, 0.2,
                ))),
                text_color: iced::Color::WHITE,
                border: iced::Border {
                    color: iced::Color::from_rgb(0.9, 0.3, 0.3),
                    width: 1.0,
                    radius: 10.0.into(),
                },
                ..Default::default()
            })
            .on_press(AppMessage::EndCall);
        */

        let end_button = button(text("Finalizar llamada"))
            .padding(10)
            .style(
                |_theme: &Theme, _status: iced::widget::button::Status| button::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgb(
                        0.8, 0.2, 0.2,
                    ))),
                    text_color: iced::Color::WHITE,
                    border: iced::Border {
                        color: iced::Color::from_rgb(0.9, 0.3, 0.3),
                        width: 1.0,
                        radius: 10.0.into(),
                    },
                    ..Default::default()
                },
            )
            .on_press(AppMessage::EndCall);

        let (mute_text, mute_bg) = if self.is_muted {
            ("Activar Mic", iced::Color::from_rgb(0.9, 0.6, 0.1))
        } else {
            ("Mutear Mic", iced::Color::from_rgb(0.4, 0.4, 0.4))
        };

        let mute_button = button(text(mute_text))
            .padding(10)
            .style(
                move |_theme: &Theme, _status: iced::widget::button::Status| button::Style {
                    background: Some(iced::Background::Color(mute_bg)),
                    text_color: iced::Color::WHITE,
                    border: iced::Border {
                        color: iced::Color::WHITE,
                        width: 1.0,
                        radius: 10.0.into(),
                    },
                    ..Default::default()
                },
            )
            .on_press(AppMessage::ToggleAudio);

        // Botón para seleccionar archivo
        let pick_file_button = button(text("Seleccionar Archivo"))
            .padding(10)
            .style(|_theme: &Theme, _status| button::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgb(
                    0.3, 0.5, 0.7,
                ))),
                text_color: iced::Color::WHITE,
                border: iced::Border {
                    color: iced::Color::from_rgb(0.4, 0.6, 0.8),
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            })
            .on_press(AppMessage::PickFile);

        // Mostrar archivo seleccionado y botón de envío
        let file_display = if file_path.is_empty() {
            text("No se ha seleccionado ningún archivo").size(14)
        } else {
            text(format!("Archivo: {}", file_path)).size(14)
        };

        let send_button: button::Button<'_, AppMessage, Theme, iced::Renderer> =
            button(text("Enviar Archivo"))
                .padding(10)
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgb(
                        0.2, 0.6, 0.9,
                    ))),
                    text_color: iced::Color::WHITE,
                    border: iced::Border {
                        color: iced::Color::from_rgb(0.3, 0.7, 1.0),
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    ..Default::default()
                });

        // Deshabilitar el botón si no hay archivo seleccionado O si hay una transferencia pendiente
        let send_button = if !file_path.is_empty() && pending_file_offer.is_none() {
            send_button.on_press(AppMessage::SendFile)
        } else {
            send_button
        };

        let sctp_controls = column![
            row![pick_file_button].spacing(10),
            file_display,
            send_button
        ]
        .spacing(10)
        .align_x(iced::alignment::Horizontal::Left);

        let controls_row = row![mute_button, end_button]
            .spacing(20)
            .align_y(iced::alignment::Vertical::Center);

        // Base content (always visible)
        let base_content = column![
            text("Videollamada activa").size(20),
            row![local_view, remote_view, self.view_chat()].spacing(20),
            text("Transferencia de archivos (SCTP)").size(16),
            sctp_controls,
            controls_row,
        ]
        .spacing(20)
        .padding(20);

        // Add file offer dialog if present (as overlay)
        if let Some(offer) = pending_file_offer {
            let size_kb = offer.size as f64 / 1024.0;
            let size_text = if size_kb < 1024.0 {
                format!("{:.2} KB", size_kb)
            } else {
                format!("{:.2} MB", size_kb / 1024.0)
            };

            let filename_display = offer.filename.clone();
            let filename_clone = offer.filename.clone();
            let filename_clone2 = offer.filename.clone();

            let accept_button: button::Button<'_, AppMessage, Theme, iced::Renderer> =
                button(text("Aceptar"))
                    .padding(10)
                    .style(|_theme: &Theme, _status| button::Style {
                        background: Some(iced::Background::Color(iced::Color::from_rgb(
                            0.2, 0.7, 0.3,
                        ))),
                        text_color: iced::Color::WHITE,
                        border: iced::Border {
                            color: iced::Color::from_rgb(0.3, 0.8, 0.4),
                            width: 1.0,
                            radius: 8.0.into(),
                        },
                        ..Default::default()
                    })
                    .on_press(AppMessage::AcceptFileTransfer {
                        filename: filename_clone,
                    });

            let reject_button: button::Button<'_, AppMessage, Theme, iced::Renderer> =
                button(text("Rechazar"))
                    .padding(10)
                    .style(|_theme: &Theme, _status| button::Style {
                        background: Some(iced::Background::Color(iced::Color::from_rgb(
                            0.7, 0.2, 0.2,
                        ))),
                        text_color: iced::Color::WHITE,
                        border: iced::Border {
                            color: iced::Color::from_rgb(0.8, 0.3, 0.3),
                            width: 1.0,
                            radius: 8.0.into(),
                        },
                        ..Default::default()
                    })
                    .on_press(AppMessage::RejectFileTransfer {
                        filename: filename_clone2,
                    });

            // Modal dialog - compact and centered
            let modal_bg = container(
                column![
                    text("Archivo entrante").size(20).color(iced::Color::WHITE),
                    text(filename_display).size(16).color(iced::Color::WHITE),
                    text(size_text)
                        .size(14)
                        .color(iced::Color::from_rgb(0.8, 0.8, 0.8)),
                    row![accept_button, reject_button].spacing(20)
                ]
                .spacing(15)
                .padding(30)
                .align_x(iced::alignment::Horizontal::Center),
            )
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgb(
                    0.15, 0.15, 0.15,
                ))),
                border: iced::Border {
                    color: iced::Color::from_rgb(0.3, 0.5, 0.7),
                    width: 3.0,
                    radius: 15.0.into(),
                },
                shadow: iced::Shadow {
                    color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.8),
                    offset: iced::Vector::new(0.0, 4.0),
                    blur_radius: 20.0,
                },
                ..Default::default()
            })
            .width(400)
            .height(Length::Shrink)
            .center_x(Length::Fill)
            .center_y(Length::Fill);

            // Stack: base content + modal overlay
            let content = iced::widget::stack![base_content, modal_bg];

            return container(content)
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }

        // No file offer: just base content
        container(base_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    // Panel de chat: historial literal, campo de texto (Enter envía) y botón.
    fn view_chat(&self) -> Element<'_, AppMessage> {
        let lines = self
            .chat
            .history
            .iter()
            .fold(column![].spacing(6), |col, entry| {
                let line = text(chat_line(entry)).size(14);
                let line = if entry.not_sent {
                    line.color(iced::Color::from_rgb(0.9, 0.3, 0.3))
                } else {
                    line
                };
                col.push(line)
            });

        let input = text_input("Escribí un mensaje...", &self.chat.draft)
            .on_input(AppMessage::ChatDraftChanged)
            .on_submit(AppMessage::ChatSend)
            .padding(8);

        let send_button = button(text("Enviar"))
            .padding(8)
            .on_press(AppMessage::ChatSend);

        column![
            text("Chat").size(16),
            scrollable(lines).height(Length::Fixed(VIDEO_HEIGHT_F32 - 90.0)),
            row![input, send_button].spacing(8),
        ]
        .spacing(10)
        .width(Length::Fixed(300.0))
        .into()
    }

    pub fn subscription(&self) -> Subscription<AppMessage> {
        match &self.screen {
            AppScreen::Login(_) | AppScreen::Register(_) => {
                // verifico resultados de autenticación
                window::frames().map(|_| AppMessage::AuthTick)
            }
            AppScreen::Lobby { .. } => {
                // verifico respuestas del signaling
                window::frames().map(|_| AppMessage::LobbyTick)
            }
            AppScreen::VideoCall { .. } => {
                // desde video llamada, verificar frames
                Subscription::batch([
                    window::frames().map(AppMessage::FrameUpdate),
                    window::close_events().map(|_| AppMessage::SystemClose),
                ])
            }
        }
    }

    // Llamada nueva: nueva generación y chat vacío. Lo que quede de la llamada
    // anterior en los canales se descarta por generación (design D2).
    fn start_call_chat(&mut self) {
        self.call_generation.fetch_add(1, Ordering::SeqCst);
        self.chat.reset();
    }

    // Un mensaje de chat pertenece a la llamada en curso.
    fn is_current_call(&self, generation: u64) -> bool {
        matches!(self.screen, AppScreen::VideoCall { .. })
            && generation == self.call_generation.load(Ordering::SeqCst)
    }

    pub fn theme(&self) -> Theme {
        Theme::Dark
    }
}

/// Texto de una línea del historial del chat. Se muestra literal, sin
/// interpretar marcado.
fn chat_line(entry: &ChatEntry) -> String {
    match entry.author {
        ChatAuthor::Own { .. } if entry.not_sent => format!("Yo: {} (no enviado)", entry.text),
        ChatAuthor::Own { .. } => format!("Yo: {}", entry.text),
        ChatAuthor::Peer => format!("Participante: {}", entry.text),
    }
}

pub fn run_unified_app(config: UnifiedAppConfig) -> iced::Result {
    let pending_auth_clone = Arc::clone(&config.pending_auth);
    let auth_result_clone = Arc::clone(&config.auth_result);
    let signaling_response_clone = Arc::clone(&config.signaling_response);
    let sctp_notif_rx_clone = Arc::clone(&config.sctp_notif_rx);
    let local_frame_clone = Arc::clone(&config.local_frame);
    let remote_frame_clone = Arc::clone(&config.remote_frame);
    let running_call_clone = Arc::clone(&config.running_call);
    let call_generation_clone = Arc::clone(&config.call_generation);
    let shutdown_clone = Arc::clone(&config.shutdown);
    let audio_muted_clone = Arc::clone(&config.audio_muted_signal);
    let signaling_tx = config.signaling_tx;
    let sctp_tx = config.sctp_tx;
    let width = config.width;
    let height = config.height;

    iced::application("Cliente RTC", UnifiedApp::update, UnifiedApp::view)
        .subscription(UnifiedApp::subscription)
        .theme(UnifiedApp::theme)
        .font(iced_fonts::BOOTSTRAP_FONT_BYTES)
        .run_with(move || {
            (
                UnifiedApp::new(UnifiedAppConfig {
                    pending_auth: pending_auth_clone,
                    auth_result: auth_result_clone,
                    signaling_tx,
                    signaling_response: signaling_response_clone,
                    sctp_tx,
                    sctp_notif_rx: sctp_notif_rx_clone,
                    local_frame: local_frame_clone,
                    remote_frame: remote_frame_clone,
                    width,
                    height,
                    running_call: running_call_clone,
                    call_generation: call_generation_clone,
                    shutdown: shutdown_clone,
                    audio_muted_signal: audio_muted_clone,
                }),
                Task::none(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::{Duration, Instant};

    fn make_app_with_channels() -> (
        UnifiedApp,
        mpsc::Receiver<SignalingCommand>,
        mpsc::Receiver<SctpCommand>,
    ) {
        let (app, sig_rx, sctp_rx, _sctp_notif_tx) = make_app_with_all_channels();
        (app, sig_rx, sctp_rx)
    }

    // Igual que `make_app_with_channels`, pero también devuelve el emisor de
    // notificaciones SCTP para inyectar lo que llegaría del hilo RX.
    fn make_app_with_all_channels() -> (
        UnifiedApp,
        mpsc::Receiver<SignalingCommand>,
        mpsc::Receiver<SctpCommand>,
        mpsc::Sender<SctpNotification>,
    ) {
        let pending_auth = Arc::new(Mutex::new(None));
        let auth_result = Arc::new(Mutex::new(None));

        let (sig_tx, sig_rx) = mpsc::channel::<SignalingCommand>();
        let signaling_response = Arc::new(Mutex::new(None));

        let (sctp_tx, sctp_rx) = mpsc::channel::<SctpCommand>();

        let (sctp_notif_tx, sctp_notif_rx_inner) = mpsc::channel::<SctpNotification>();
        let sctp_notif_rx = Arc::new(Mutex::new(sctp_notif_rx_inner));

        let width = 2;
        let height = 2;

        let local_frame = Arc::new(Mutex::new(None));
        let remote_frame = Arc::new(Mutex::new(None));

        let running_call = Arc::new(AtomicBool::new(false));
        let call_generation = Arc::new(AtomicU64::new(0));
        let shutdown = Arc::new(AtomicBool::new(false));
        let audio_muted_signal = Arc::new(AtomicBool::new(false));

        let app = UnifiedApp::new(UnifiedAppConfig {
            pending_auth,
            auth_result,
            signaling_tx: Some(sig_tx),
            signaling_response,
            sctp_tx: Some(sctp_tx),
            sctp_notif_rx,
            local_frame,
            remote_frame,
            width,
            height,
            running_call,
            call_generation,
            shutdown,
            audio_muted_signal,
        });

        (app, sig_rx, sctp_rx, sctp_notif_tx)
    }

    // Helper del Hito 3: pantalla de videollamada con `running_call` en true.
    fn make_app_in_call() -> (
        UnifiedApp,
        mpsc::Receiver<SctpCommand>,
        mpsc::Sender<SctpNotification>,
    ) {
        let (mut app, _sig_rx, sctp_rx, notif_tx) = make_app_with_all_channels();
        force_video(&mut app);
        app.running_call.store(true, Ordering::SeqCst);
        (app, sctp_rx, notif_tx)
    }

    fn force_lobby(app: &mut UnifiedApp) {
        app.screen = AppScreen::Lobby {
            users: Vec::new(),
            incoming_call: None,
        };
    }

    fn force_video(app: &mut UnifiedApp) {
        app.screen = AppScreen::VideoCall {
            joined: true,
            file_path: String::new(),
            pending_file_offer: None,
        };
    }

    #[test]
    fn theme_is_dark() {
        let (app, _, _) = make_app_with_channels();
        assert_eq!(app.theme(), Theme::Dark);
    }

    #[test]
    fn request_user_list_sends_command() {
        let (mut app, sig_rx, _) = make_app_with_channels();
        force_lobby(&mut app);

        let _ = app.update(AppMessage::RequestUserList);

        let cmd = sig_rx.recv_timeout(Duration::from_millis(100)).unwrap();
        assert!(matches!(cmd, SignalingCommand::GetUserList));
    }

    #[test]
    fn call_user_by_click_sends_call_request() {
        let (mut app, sig_rx, _) = make_app_with_channels();
        force_lobby(&mut app);

        let _ = app.update(AppMessage::CallUserByClick {
            username: "alice".to_string(),
        });

        let cmd = sig_rx.recv_timeout(Duration::from_millis(100)).unwrap();
        match cmd {
            SignalingCommand::CallRequest { callee } => assert_eq!(callee, "alice"),
            other => panic!("Expected CallRequest, got: {:?}", other),
        }
    }

    #[test]
    fn update_user_list_filters_current_user() {
        let (mut app, _sig_rx, _) = make_app_with_channels();
        force_lobby(&mut app);

        app.current_username = Some("me".to_string());

        let users = vec![
            ("me ".to_string(), UserStatus::Available),
            ("alice".to_string(), UserStatus::Available),
            ("bob".to_string(), UserStatus::Busy),
        ];

        let _ = app.update(AppMessage::UpdateUserList(users));

        match &app.screen {
            AppScreen::Lobby { users, .. } => {
                assert_eq!(users.len(), 2);
                assert!(users.iter().all(|(u, _)| u.trim() != "me"));
            }
            _ => panic!("Expected Lobby screen"),
        }
    }

    #[test]
    fn incoming_call_sets_incoming_call_field() {
        let (mut app, _sig_rx, _) = make_app_with_channels();
        force_lobby(&mut app);

        let _ = app.update(AppMessage::IncomingCall {
            caller: "alice".to_string(),
        });

        match &app.screen {
            AppScreen::Lobby { incoming_call, .. } => {
                assert_eq!(incoming_call.as_deref(), Some("alice"));
            }
            _ => panic!("Expected Lobby screen"),
        }
    }

    #[test]
    fn accept_call_sends_accept_and_switches_to_video_and_sets_running_call() {
        let (mut app, sig_rx, _) = make_app_with_channels();
        force_lobby(&mut app);

        // set incoming_call
        if let AppScreen::Lobby { incoming_call, .. } = &mut app.screen {
            *incoming_call = Some("alice".to_string());
        }

        assert!(!app.running_call.load(Ordering::SeqCst));

        let _ = app.update(AppMessage::AcceptCall);

        // verifica envío al signaling
        let cmd = sig_rx.recv_timeout(Duration::from_millis(100)).unwrap();
        match cmd {
            SignalingCommand::AcceptCall { caller } => assert_eq!(caller, "alice"),
            other => panic!("Expected AcceptCall, got: {:?}", other),
        }

        // verifica transición a video y flag
        assert!(app.running_call.load(Ordering::SeqCst));
        assert!(matches!(
            app.screen,
            AppScreen::VideoCall { joined: true, .. }
        ));
    }

    #[test]
    fn accept_call_without_incoming_call_does_nothing() {
        let (mut app, sig_rx, _) = make_app_with_channels();
        force_lobby(&mut app);

        let _ = app.update(AppMessage::AcceptCall);

        // no debería enviar nada
        assert!(sig_rx.recv_timeout(Duration::from_millis(50)).is_err());
        assert!(!app.running_call.load(Ordering::SeqCst));
        assert!(matches!(app.screen, AppScreen::Lobby { .. }));
    }

    #[test]
    fn reject_call_sends_reject_and_clears_incoming_call() {
        let (mut app, sig_rx, _) = make_app_with_channels();
        force_lobby(&mut app);

        if let AppScreen::Lobby { incoming_call, .. } = &mut app.screen {
            *incoming_call = Some("alice".to_string());
        }

        let _ = app.update(AppMessage::RejectCall);

        let cmd = sig_rx.recv_timeout(Duration::from_millis(100)).unwrap();
        match cmd {
            SignalingCommand::RejectCall { caller } => assert_eq!(caller, "alice"),
            other => panic!("Expected RejectCall, got: {:?}", other),
        }

        match &app.screen {
            AppScreen::Lobby { incoming_call, .. } => assert!(incoming_call.is_none()),
            _ => panic!("Expected Lobby screen"),
        }
    }

    #[test]
    fn end_call_sends_end_call_clears_frames_and_returns_to_lobby() {
        let (mut app, sig_rx, _) = make_app_with_channels();
        force_video(&mut app);

        app.running_call.store(true, Ordering::SeqCst);

        // mete frames “válidos” (RGBA = width*height*4 = 16 bytes para 2x2)
        *app.local_frame.lock().unwrap() = Some(vec![0u8; 16]);
        *app.remote_frame.lock().unwrap() = Some(vec![0u8; 16]);

        let _ = app.update(AppMessage::EndCall);

        let cmd = sig_rx.recv_timeout(Duration::from_millis(100)).unwrap();
        assert!(matches!(cmd, SignalingCommand::EndCall));

        // flags y limpieza
        assert!(!app.running_call.load(Ordering::SeqCst));
        assert!(app.local_frame.lock().unwrap().is_none());
        assert!(app.remote_frame.lock().unwrap().is_none());

        // vuelve al lobby
        assert!(matches!(app.screen, AppScreen::Lobby { .. }));
    }

    #[test]
    fn remote_call_ended_clears_frames_and_returns_to_lobby_without_sending_endcall() {
        let (mut app, sig_rx, _) = make_app_with_channels();
        force_video(&mut app);

        app.running_call.store(true, Ordering::SeqCst);
        *app.local_frame.lock().unwrap() = Some(vec![0u8; 16]);
        *app.remote_frame.lock().unwrap() = Some(vec![0u8; 16]);

        let _ = app.update(AppMessage::RemoteCallEnded);

        // no debería enviar EndCall
        assert!(sig_rx.recv_timeout(Duration::from_millis(50)).is_err());

        assert!(!app.running_call.load(Ordering::SeqCst));
        assert!(app.local_frame.lock().unwrap().is_none());
        assert!(app.remote_frame.lock().unwrap().is_none());
        assert!(matches!(app.screen, AppScreen::Lobby { .. }));
    }

    #[test]
    fn file_path_changed_updates_state_in_video() {
        let (mut app, _sig_rx, _) = make_app_with_channels();
        force_video(&mut app);

        let _ = app.update(AppMessage::FilePathChanged("/tmp/a.bin".to_string()));

        match &app.screen {
            AppScreen::VideoCall { file_path, .. } => assert_eq!(file_path, "/tmp/a.bin"),
            _ => panic!("Expected VideoCall screen"),
        }
    }

    #[test]
    fn send_file_sends_sctp_command_with_current_path() {
        let (mut app, _sig_rx, sctp_rx) = make_app_with_channels();
        force_video(&mut app);

        if let AppScreen::VideoCall { file_path, .. } = &mut app.screen {
            *file_path = "/tmp/a.bin".to_string();
        }

        let _ = app.update(AppMessage::SendFile);

        let cmd = sctp_rx.recv_timeout(Duration::from_millis(100)).unwrap();
        match cmd {
            SctpCommand::SendFile { path } => assert_eq!(path, "/tmp/a.bin"),
            other => panic!("Expected SendFile, got: {:?}", other),
        }
    }

    #[test]
    fn system_close_sets_shutdown_and_stops_running_call() {
        let (mut app, _sig_rx, _) = make_app_with_channels();
        force_video(&mut app);

        app.running_call.store(true, Ordering::SeqCst);
        assert!(!app.shutdown.load(Ordering::SeqCst));

        let _ = app.update(AppMessage::SystemClose);

        assert!(!app.running_call.load(Ordering::SeqCst));
        assert!(app.shutdown.load(Ordering::SeqCst));
    }

    #[test]
    fn toggle_audio_flips_is_muted_and_updates_atomic_signal() {
        let (mut app, _sig_rx, _) = make_app_with_channels();

        assert!(!app.is_muted);
        assert!(!app.audio_muted_signal.load(Ordering::Relaxed));

        let _ = app.update(AppMessage::ToggleAudio);

        assert!(app.is_muted);
        assert!(app.audio_muted_signal.load(Ordering::Relaxed));

        let _ = app.update(AppMessage::ToggleAudio);

        assert!(!app.is_muted);
        assert!(!app.audio_muted_signal.load(Ordering::Relaxed));
    }

    #[test]
    fn frame_update_consumes_call_ended_response_if_present() {
        let (mut app, _sig_rx, _) = make_app_with_channels();
        force_video(&mut app);

        *app.signaling_response.lock().unwrap() = Some(SignalingResponse::CallEnded);

        let _ = app.update(AppMessage::FrameUpdate(Instant::now()));

        assert!(app.signaling_response.lock().unwrap().is_none());
    }

    #[test]
    fn view_and_subscription_smoke_for_each_screen() {
        let (mut app, _sig_rx, _) = make_app_with_channels();

        // Login
        let _ = app.view();
        let _ = app.subscription();

        // Register
        app.screen = AppScreen::Register(RegisterState::new());
        let _ = app.view();
        let _ = app.subscription();

        // Lobby
        force_lobby(&mut app);
        let _ = app.view();
        let _ = app.subscription();

        // Video
        force_video(&mut app);
        *app.local_frame.lock().unwrap() = None;
        *app.remote_frame.lock().unwrap() = None;

        let _ = app.view();
        let _ = app.subscription();
    }

    // ================= Chat (Hito 3) =================

    use crate::chat::{ChatAuthor, ChatEntry};

    fn current_gen(app: &UnifiedApp) -> u64 {
        app.call_generation.load(Ordering::SeqCst)
    }

    fn own(id: u64, text: &str) -> ChatEntry {
        ChatEntry {
            author: ChatAuthor::Own { id },
            text: text.to_string(),
            not_sent: false,
        }
    }

    fn peer(text: &str) -> ChatEntry {
        ChatEntry {
            author: ChatAuthor::Peer,
            text: text.to_string(),
            not_sent: false,
        }
    }

    fn history(app: &UnifiedApp) -> Vec<ChatEntry> {
        app.chat.history.iter().cloned().collect()
    }

    fn type_and_send(app: &mut UnifiedApp, text: &str) {
        let _ = app.update(AppMessage::ChatDraftChanged(text.to_string()));
        let _ = app.update(AppMessage::ChatSend);
    }

    fn tick(app: &mut UnifiedApp) {
        let _ = app.update(AppMessage::FrameUpdate(Instant::now()));
    }

    #[test]
    fn chat_3_1_actualiza_borrador() {
        let (mut app, _sctp_rx, _notif_tx) = make_app_in_call();

        let _ = app.update(AppMessage::ChatDraftChanged("Hola Bob".to_string()));

        assert_eq!(app.chat.draft, "Hola Bob");
    }

    #[test]
    fn chat_3_2_enviar_agrega_al_historial_y_limpia_borrador() {
        let (mut app, sctp_rx, _notif_tx) = make_app_in_call();

        type_and_send(&mut app, "Hola Bob");

        assert_eq!(
            sctp_rx.try_recv(),
            Ok(SctpCommand::SendChat {
                id: 1,
                generation: current_gen(&app),
                text: "Hola Bob".to_string()
            })
        );
        assert_eq!(history(&app), vec![own(1, "Hola Bob")]);
        assert_eq!(app.chat.draft, "");
        let _ = app.view();
    }

    #[test]
    fn chat_3_3_no_envia_borrador_invalido() {
        let (mut app, sctp_rx, _notif_tx) = make_app_in_call();
        let largo = "a".repeat(4097);

        for draft in ["", "   ", largo.as_str()] {
            type_and_send(&mut app, draft);
            assert_eq!(sctp_rx.try_recv(), Err(mpsc::TryRecvError::Empty));
            assert!(history(&app).is_empty());
        }

        // Control: un borrador válido sí sale, así que el rechazo no es casual
        type_and_send(&mut app, "ok");
        assert!(matches!(
            sctp_rx.try_recv(),
            Ok(SctpCommand::SendChat { .. })
        ));
    }

    #[test]
    fn chat_3_4_muestra_mensaje_recibido() {
        let (mut app, _sctp_rx, notif_tx) = make_app_in_call();
        let generation = current_gen(&app);

        let _ = notif_tx.send(SctpNotification::ChatReceived {
            generation,
            text: "Hola Alice".to_string(),
        });
        tick(&mut app);

        assert_eq!(history(&app), vec![peer("Hola Alice")]);
        let _ = app.view();
    }

    #[test]
    fn chat_3_5_procesa_una_notificacion_por_tick() {
        let (mut app, _sctp_rx, notif_tx) = make_app_in_call();
        let generation = current_gen(&app);
        for text in ["uno", "dos", "tres"] {
            let _ = notif_tx.send(SctpNotification::ChatReceived {
                generation,
                text: text.to_string(),
            });
        }

        tick(&mut app);
        assert_eq!(history(&app), vec![peer("uno")]);
        tick(&mut app);
        tick(&mut app);

        assert_eq!(history(&app), vec![peer("uno"), peer("dos"), peer("tres")]);
    }

    #[test]
    fn chat_3_6_muestra_no_enviado() {
        let (mut app, _sctp_rx, notif_tx) = make_app_in_call();
        type_and_send(&mut app, "uno");
        type_and_send(&mut app, "dos");
        let generation = current_gen(&app);

        let _ = notif_tx.send(SctpNotification::ChatSendFailed { id: 2, generation });
        tick(&mut app);

        let mut fallido = own(2, "dos");
        fallido.not_sent = true;
        assert_eq!(history(&app), vec![own(1, "uno"), fallido]);
        let lines: Vec<String> = app.chat.history.iter().map(chat_line).collect();
        assert!(!lines[0].contains("no enviado"));
        assert!(lines[1].contains("dos") && lines[1].contains("no enviado"));
        let _ = app.view();
    }

    #[test]
    fn chat_3_7_fin_de_llamada_limpia_chat() {
        for fin in [AppMessage::EndCall, AppMessage::RemoteCallEnded] {
            let (mut app, _sctp_rx, _notif_tx) = make_app_in_call();
            let _ = app.update(AppMessage::JoinCall);
            type_and_send(&mut app, "hola");
            let _ = app.update(AppMessage::ChatDraftChanged("borrador".to_string()));
            assert_eq!(history(&app).len(), 1);

            let _ = app.update(fin);
            assert!(matches!(app.screen, AppScreen::Lobby { .. }));
            let _ = app.update(AppMessage::JoinCall);

            assert!(history(&app).is_empty());
            assert_eq!(app.chat.draft, "");
        }
    }

    #[test]
    fn chat_3_8_ignora_notificaciones_de_llamada_anterior() {
        let (mut app, _sctp_rx, notif_tx) = make_app_in_call();
        let _ = app.update(AppMessage::JoinCall);
        let _ = app.update(AppMessage::EndCall);
        // Llegan con la app en el lobby: el hilo RX las marca con la
        // generación vigente, que es la de la llamada que terminó
        let vieja = current_gen(&app);
        for text in ["uno", "dos"] {
            let _ = notif_tx.send(SctpNotification::ChatReceived {
                generation: vieja,
                text: text.to_string(),
            });
        }

        let _ = app.update(AppMessage::JoinCall);
        tick(&mut app);
        tick(&mut app);

        assert!(history(&app).is_empty());
        // Control: un mensaje de la llamada nueva sí aparece
        let _ = notif_tx.send(SctpNotification::ChatReceived {
            generation: current_gen(&app),
            text: "nuevo".to_string(),
        });
        tick(&mut app);
        assert_eq!(history(&app), vec![peer("nuevo")]);
    }

    #[test]
    fn chat_3_9_flujo_de_archivos_sin_cambios() {
        let (mut app, _sig_rx, sctp_rx) = make_app_with_channels();
        force_video(&mut app);
        app.chat.push_own("hola", crate::chat::CHAT_HISTORY_MAX);
        if let AppScreen::VideoCall { file_path, .. } = &mut app.screen {
            *file_path = "/tmp/a.bin".to_string();
        }

        let _ = app.update(AppMessage::SendFile);

        assert_eq!(
            sctp_rx.try_recv(),
            Ok(SctpCommand::SendFile {
                path: "/tmp/a.bin".to_string()
            })
        );
        assert_eq!(sctp_rx.try_recv(), Err(mpsc::TryRecvError::Empty));
    }

    #[test]
    fn chat_3_10_no_envia_con_running_call_en_falso() {
        let (mut app, sctp_rx, _notif_tx) = make_app_in_call();
        app.running_call.store(false, Ordering::SeqCst);

        type_and_send(&mut app, "Hola");

        assert_eq!(sctp_rx.try_recv(), Err(mpsc::TryRecvError::Empty));
        assert!(history(&app).is_empty());
        // Control: con running_call en true el mismo envío sí sale
        app.running_call.store(true, Ordering::SeqCst);
        let _ = app.update(AppMessage::ChatSend);
        assert!(matches!(
            sctp_rx.try_recv(),
            Ok(SctpCommand::SendChat { .. })
        ));
    }

    #[test]
    fn chat_3_11_marca_no_enviado_si_el_canal_esta_cerrado() {
        let (mut app, sctp_rx, _notif_tx) = make_app_in_call();
        // El hilo SCTP TX terminó: el canal de comandos ya no acepta mensajes
        drop(sctp_rx);

        type_and_send(&mut app, "Hola");

        let mut fallido = own(1, "Hola");
        fallido.not_sent = true;
        assert_eq!(history(&app), vec![fallido]);
        let lines: Vec<String> = app.chat.history.iter().map(chat_line).collect();
        assert!(lines[0].contains("Hola") && lines[0].contains("no enviado"));
        let _ = app.view();
    }
}
