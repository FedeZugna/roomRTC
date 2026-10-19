use super::login_viewer::{LoginAction, LoginMessage, LoginState};
use super::register_viewer::{RegisterAction, RegisterMessage, RegisterState};
use iced::widget::container;
use iced::{Element, Length, Subscription, Task, Theme, window};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub enum AuthMessage {
    Login(LoginMessage),
    Register(RegisterMessage),
    JoinLobby,
    Tick,
}

pub enum AuthScreen {
    Login(LoginState),
    Register(RegisterState),
    Lobby,
}

pub struct AuthFlow {
    screen: AuthScreen,
    join_flag: Arc<AtomicBool>,
    pending_auth: Arc<Mutex<Option<PendingAuth>>>,
    auth_result: Arc<Mutex<Option<Result<String, String>>>>,
}

#[derive(Debug, Clone)]
pub struct PendingAuth {
    pub username: String,
    pub password: String,
    pub email: Option<String>,
    pub is_register: bool,
}

impl AuthFlow {
    pub fn new(
        join_flag: Arc<AtomicBool>,
        pending_auth: Arc<Mutex<Option<PendingAuth>>>,
        auth_result: Arc<Mutex<Option<Result<String, String>>>>,
    ) -> Self {
        Self {
            screen: AuthScreen::Login(LoginState::new()),
            join_flag,
            pending_auth,
            auth_result,
        }
    }

    pub fn update(&mut self, message: AuthMessage) -> Task<AuthMessage> {
        match message {
            AuthMessage::Login(login_msg) => {
                if let AuthScreen::Login(state) = &mut self.screen
                    && let Some(action) = state.update(login_msg)
                {
                    match action {
                        LoginAction::Login { username, password } => {
                            // almacena el estado de autenticación pendiente
                            *self.pending_auth.lock().unwrap() = Some(PendingAuth {
                                username,
                                password,
                                email: None,
                                is_register: false,
                            });
                        }
                        LoginAction::SwitchToRegister => {
                            self.screen = AuthScreen::Register(RegisterState::new());
                        }
                    }
                }
                Task::none()
            }
            AuthMessage::Register(register_msg) => {
                if let AuthScreen::Register(state) = &mut self.screen
                    && let Some(action) = state.update(register_msg)
                {
                    match action {
                        RegisterAction::Register {
                            username,
                            password,
                            email,
                        } => {
                            // almacena el estado de autenticación pendiente
                            *self.pending_auth.lock().unwrap() = Some(PendingAuth {
                                username,
                                password,
                                email,
                                is_register: true,
                            });
                        }
                        RegisterAction::BackToLogin => {
                            self.screen = AuthScreen::Login(LoginState::new());
                        }
                    }
                }
                Task::none()
            }
            AuthMessage::JoinLobby => {
                // usuario hizo clic en unirse al lobby
                self.join_flag.store(true, Ordering::SeqCst);
                Task::none()
            }
            AuthMessage::Tick => {
                // verifico si el usuario quiere unirse (desde el lobby)
                if self.join_flag.load(Ordering::SeqCst) {
                    // El usuario hizo clic en unirse, cerrar esta ventana
                    return iced::exit();
                }

                // Verifico si hay un resultado de autenticación del hilo en segundo plano
                if let Some(result) = self.auth_result.lock().unwrap().take() {
                    match result {
                        Ok(msg) => {
                            println!("[Auth] Success: {}", msg);
                            // cambio al lobby en caso de éxito
                            self.screen = AuthScreen::Lobby;
                        }
                        Err(error) => {
                            println!("[Auth] Error: {}", error);
                            // Mostrar error en la pantalla actual
                            match &mut self.screen {
                                AuthScreen::Login(state) => {
                                    state.set_error(error);
                                }
                                AuthScreen::Register(state) => {
                                    state.set_error(error);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, AuthMessage> {
        match &self.screen {
            AuthScreen::Login(state) => state.view().map(AuthMessage::Login),
            AuthScreen::Register(state) => state.view().map(AuthMessage::Register),
            AuthScreen::Lobby => self.view_lobby(),
        }
    }

    fn view_lobby(&self) -> Element<'_, AuthMessage> {
        use iced::widget::{button, column, text};

        let join_button = button(text("Unirse a la llamada"))
            .padding(10)
            .on_press(AuthMessage::JoinLobby);

        let content = column![
            text("Sala de espera").size(24),
            text("Cuando estés listo, unite a la llamada.").size(16),
            join_button,
        ]
        .spacing(20)
        .padding(20);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    pub fn subscription(&self) -> Subscription<AuthMessage> {
        // Use window frames to periodically check for auth results
        window::frames().map(|_| AuthMessage::Tick)
    }

    pub fn theme(&self) -> Theme {
        Theme::Dark
    }
}

pub fn run_auth_flow(
    join_flag: Arc<AtomicBool>,
    pending_auth: Arc<Mutex<Option<PendingAuth>>>,
    auth_result: Arc<Mutex<Option<Result<String, String>>>>,
) -> iced::Result {
    let join_flag_clone = Arc::clone(&join_flag);
    let pending_auth_clone = Arc::clone(&pending_auth);
    let auth_result_clone = Arc::clone(&auth_result);

    iced::application("Autenticación", AuthFlow::update, AuthFlow::view)
        .subscription(AuthFlow::subscription)
        .theme(AuthFlow::theme)
        .run_with(move || {
            (
                AuthFlow::new(join_flag_clone, pending_auth_clone, auth_result_clone),
                Task::none(),
            )
        })
}



#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::sync::atomic::{AtomicBool, Ordering};

    // Helper para configurar el entorno de prueba limpio
    fn setup() -> (
        AuthFlow,
        Arc<AtomicBool>,
        Arc<Mutex<Option<PendingAuth>>>,
        Arc<Mutex<Option<Result<String, String>>>>
    ) {
        let join_flag = Arc::new(AtomicBool::new(false));
        let pending_auth = Arc::new(Mutex::new(None));
        let auth_result = Arc::new(Mutex::new(None));

        let flow = AuthFlow::new(
            join_flag.clone(),
            pending_auth.clone(),
            auth_result.clone(),
        );

        (flow, join_flag, pending_auth, auth_result)
    }

    #[test]
    fn test_initial_state_is_login() {
        let (flow, _, _, _) = setup();
        // Verifico que arranque en la pantalla de Login
        assert!(matches!(flow.screen, AuthScreen::Login(_)));
    }

    #[test]
    fn test_join_lobby_sets_flag() {
        let (mut flow, join_flag, _, _) = setup();

        // Simulo el clic en "Unirse a la llamada"
        let _ = flow.update(AuthMessage::JoinLobby);

        // El flag debe haber cambiado a true
        assert_eq!(join_flag.load(Ordering::SeqCst), true);
    }

    #[test]
    fn test_tick_handles_auth_success() {
        let (mut flow, _, _, auth_result) = setup();

        // Simulo que el hilo auth_thread escribio un OK
        {
            let mut guard = auth_result.lock().unwrap();
            *guard = Some(Ok("Login Exitoso".to_string()));
        }

        // Ejecuto un Tick
        let _ = flow.update(AuthMessage::Tick);

        // Deberia haber cambiado la pantalla al Lobby
        assert!(matches!(flow.screen, AuthScreen::Lobby));
        
        // El resultado debería haberse consumido (vuelto a None)
        assert!(auth_result.lock().unwrap().is_none());
    }

    #[test]
    fn test_tick_handles_auth_failure() {
        let (mut flow, _, _, auth_result) = setup();

        // Simulo que el hilo reportó un ERROR
        {
            let mut guard = auth_result.lock().unwrap();
            *guard = Some(Err("Contraseña incorrecta".to_string()));
        }

        let _ = flow.update(AuthMessage::Tick);

        // Debería mantener la pantalla de Login (no pasar al Lobby)
        assert!(matches!(flow.screen, AuthScreen::Login(_)));

        // El resultado debe haberse consumido
        assert!(auth_result.lock().unwrap().is_none());
    }


    
}