use iced::widget::{Column, button, column, container, text, text_input};
use iced::{Element, Length};

#[derive(Debug, Clone)]
pub enum LoginMessage {
    UsernameChanged(String),
    PasswordChanged(String),
    LoginPressed,
    RegisterPressed,
}

#[derive(Debug, Clone)]
pub struct LoginState {
    pub username: String,
    pub password: String,
    pub error_message: Option<String>,
}

impl Default for LoginState {
    fn default() -> Self {
        Self::new()
    }
}
impl LoginState {
    pub fn new() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            error_message: None,
        }
    }

    pub fn update(&mut self, message: LoginMessage) -> Option<LoginAction> {
        match message {
            LoginMessage::UsernameChanged(value) => {
                self.username = value;
                None
            }
            LoginMessage::PasswordChanged(value) => {
                self.password = value;
                None
            }
            LoginMessage::LoginPressed => {
                if self.username.is_empty() || self.password.is_empty() {
                    self.error_message = Some("Por favor complete todos los campos".to_string());
                    None
                } else {
                    Some(LoginAction::Login {
                        username: self.username.clone(),
                        password: self.password.clone(),
                    })
                }
            }
            LoginMessage::RegisterPressed => Some(LoginAction::SwitchToRegister),
        }
    }

    pub fn set_error(&mut self, error: String) {
        self.error_message = Some(error);
    }

    pub fn view(&self) -> Element<'_, LoginMessage> {
        let title = text("Iniciar Sesión").size(32);

        let username_input = text_input("Usuario", &self.username)
            .on_input(LoginMessage::UsernameChanged)
            .padding(10);

        let password_input = text_input("Contraseña", &self.password)
            .on_input(LoginMessage::PasswordChanged)
            .secure(true)
            .padding(10);

        let login_button = button(text("Iniciar Sesión"))
            .padding(10)
            .on_press(LoginMessage::LoginPressed);

        let register_button = button(text("Registrarse"))
            .padding(10)
            .on_press(LoginMessage::RegisterPressed);

        let mut content: Column<'_, LoginMessage> = column![
            title,
            username_input,
            password_input,
            login_button,
            register_button,
        ]
        .spacing(20)
        .padding(40)
        .width(400);

        if let Some(error) = &self.error_message {
            content = content.push(text(error).style(|_theme| text::Style {
                color: Some(iced::Color::from_rgb(1.0, 0.0, 0.0)),
            }));
        }

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }
}

#[derive(Debug, Clone)]
pub enum LoginAction {
    Login { username: String, password: String },
    SwitchToRegister,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty_and_no_error() {
        let st = LoginState::default();
        assert_eq!(st.username, "");
        assert_eq!(st.password, "");
        assert_eq!(st.error_message, None);
    }

    #[test]
    fn new_is_empty_and_no_error() {
        let st = LoginState::new();
        assert_eq!(st.username, "");
        assert_eq!(st.password, "");
        assert_eq!(st.error_message, None);
    }

    #[test]
    fn username_changed_updates_username() {
        let mut st = LoginState::new();

        let action = st.update(LoginMessage::UsernameChanged("fede".to_string()));

        assert!(action.is_none());
        assert_eq!(st.username, "fede");
        assert_eq!(st.password, "");
        // no debería tocar error_message acá
        assert_eq!(st.error_message, None);
    }

    #[test]
    fn password_changed_updates_password() {
        let mut st = LoginState::new();

        let action = st.update(LoginMessage::PasswordChanged("1234".to_string()));

        assert!(action.is_none());
        assert_eq!(st.password, "1234");
        assert_eq!(st.username, "");
        assert_eq!(st.error_message, None);
    }

    #[test]
    fn login_pressed_with_missing_fields_sets_error_and_no_action() {
        let mut st = LoginState::new();
        st.username = "user".to_string();
        st.password = "".to_string();

        let action = st.update(LoginMessage::LoginPressed);

        assert!(action.is_none());
        assert_eq!(
            st.error_message.as_deref(),
            Some("Por favor complete todos los campos")
        );
    }

    #[test]
    fn login_pressed_with_both_empty_sets_error_and_no_action() {
        let mut st = LoginState::new();

        let action = st.update(LoginMessage::LoginPressed);

        assert!(action.is_none());
        assert_eq!(
            st.error_message.as_deref(),
            Some("Por favor complete todos los campos")
        );
    }

    #[test]
    fn login_pressed_with_valid_fields_returns_login_action_with_cloned_values() {
        let mut st = LoginState::new();
        st.username = "user".to_string();
        st.password = "pass".to_string();

        let action = st.update(LoginMessage::LoginPressed);

        match action {
            Some(LoginAction::Login { username, password }) => {
                assert_eq!(username, "user");
                assert_eq!(password, "pass");
            }
            other => panic!("Expected LoginAction::Login, got: {:?}", other),
        }

        assert_eq!(st.error_message, None);
        assert_eq!(st.username, "user");
        assert_eq!(st.password, "pass");
    }

    #[test]
    fn register_pressed_returns_switch_action() {
        let mut st = LoginState::new();

        let action = st.update(LoginMessage::RegisterPressed);

        assert!(matches!(action, Some(LoginAction::SwitchToRegister)));
    }

    #[test]
    fn set_error_sets_error_message() {
        let mut st = LoginState::new();

        st.set_error("boom".to_string());

        assert_eq!(st.error_message.as_deref(), Some("boom"));
    }

    #[test]
    fn view_builds_without_panic_when_no_error() {
        let st = LoginState::new();
        let _el: iced::Element<'_, LoginMessage> = st.view();
    }

    #[test]
    fn view_builds_without_panic_when_error_present() {
        let mut st = LoginState::new();
        st.error_message = Some("algo salió mal".to_string());

        let _el: iced::Element<'_, LoginMessage> = st.view();
    }

    #[test]
    fn update_does_not_clear_existing_error_on_typing() {
        let mut st = LoginState::new();
        st.error_message = Some("err".to_string());

        let _ = st.update(LoginMessage::UsernameChanged("u".to_string()));
        let _ = st.update(LoginMessage::PasswordChanged("p".to_string()));

        assert_eq!(st.error_message.as_deref(), Some("err"));
    }
}
