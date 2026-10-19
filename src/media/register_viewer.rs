use iced::widget::{Column, button, column, container, text, text_input};
use iced::{Element, Length};

#[derive(Debug, Clone)]
pub enum RegisterMessage {
    UsernameChanged(String),
    PasswordChanged(String),
    EmailChanged(String),
    RegisterPressed,
    BackToLoginPressed,
}

#[derive(Debug, Clone)]
pub struct RegisterState {
    pub username: String,
    pub password: String,
    pub email: String,
    pub error_message: Option<String>,
}

impl Default for RegisterState {
    fn default() -> Self {
        Self::new()
    }
}

impl RegisterState {
    pub fn new() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            email: String::new(),
            error_message: None,
        }
    }

    pub fn update(&mut self, message: RegisterMessage) -> Option<RegisterAction> {
        match message {
            RegisterMessage::UsernameChanged(value) => {
                self.username = value;
                None
            }
            RegisterMessage::PasswordChanged(value) => {
                self.password = value;
                None
            }
            RegisterMessage::EmailChanged(value) => {
                self.email = value;
                None
            }
            RegisterMessage::RegisterPressed => {
                if self.username.is_empty() || self.password.is_empty() {
                    self.error_message = Some("Usuario y contraseña son obligatorios".to_string());
                    None
                } else {
                    let email = if self.email.is_empty() {
                        None
                    } else {
                        Some(self.email.clone())
                    };
                    Some(RegisterAction::Register {
                        username: self.username.clone(),
                        password: self.password.clone(),
                        email,
                    })
                }
            }
            RegisterMessage::BackToLoginPressed => Some(RegisterAction::BackToLogin),
        }
    }

    pub fn set_error(&mut self, error: String) {
        self.error_message = Some(error);
    }

    pub fn view(&self) -> Element<'_, RegisterMessage> {
        let title = text("Registrarse").size(32);

        let username_input = text_input("Usuario", &self.username)
            .on_input(RegisterMessage::UsernameChanged)
            .padding(10);

        let password_input = text_input("Contraseña", &self.password)
            .on_input(RegisterMessage::PasswordChanged)
            .secure(true)
            .padding(10);

        let email_input = text_input("Email (opcional)", &self.email)
            .on_input(RegisterMessage::EmailChanged)
            .padding(10);

        let register_button = button(text("Registrarse"))
            .padding(10)
            .on_press(RegisterMessage::RegisterPressed);

        let back_button = button(text("Volver al Login"))
            .padding(10)
            .on_press(RegisterMessage::BackToLoginPressed);

        let mut content: Column<'_, RegisterMessage> = column![
            title,
            username_input,
            password_input,
            email_input,
            register_button,
            back_button,
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
pub enum RegisterAction {
    Register {
        username: String,
        password: String,
        email: Option<String>,
    },
    BackToLogin,
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty_and_no_error() {
        let st = RegisterState::default();
        assert_eq!(st.username, "");
        assert_eq!(st.password, "");
        assert_eq!(st.email, "");
        assert_eq!(st.error_message, None);
    }

    #[test]
    fn new_is_empty_and_no_error() {
        let st = RegisterState::new();
        assert_eq!(st.username, "");
        assert_eq!(st.password, "");
        assert_eq!(st.email, "");
        assert_eq!(st.error_message, None);
    }

    #[test]
    fn username_changed_updates_username() {
        let mut st = RegisterState::new();

        let action = st.update(RegisterMessage::UsernameChanged("fede".to_string()));

        assert!(action.is_none());
        assert_eq!(st.username, "fede");
        assert_eq!(st.password, "");
        assert_eq!(st.email, "");
        assert_eq!(st.error_message, None);
    }

    #[test]
    fn password_changed_updates_password() {
        let mut st = RegisterState::new();

        let action = st.update(RegisterMessage::PasswordChanged("1234".to_string()));

        assert!(action.is_none());
        assert_eq!(st.password, "1234");
        assert_eq!(st.username, "");
        assert_eq!(st.email, "");
        assert_eq!(st.error_message, None);
    }

    #[test]
    fn email_changed_updates_email() {
        let mut st = RegisterState::new();

        let action = st.update(RegisterMessage::EmailChanged("a@b.com".to_string()));

        assert!(action.is_none());
        assert_eq!(st.email, "a@b.com");
        assert_eq!(st.username, "");
        assert_eq!(st.password, "");
        assert_eq!(st.error_message, None);
    }

    #[test]
    fn register_pressed_with_missing_username_sets_error() {
        let mut st = RegisterState::new();
        st.username = "".to_string();
        st.password = "pass".to_string();

        let action = st.update(RegisterMessage::RegisterPressed);

        assert!(action.is_none());
        assert_eq!(
            st.error_message.as_deref(),
            Some("Usuario y contraseña son obligatorios")
        );
    }

    #[test]
    fn register_pressed_with_missing_password_sets_error() {
        let mut st = RegisterState::new();
        st.username = "user".to_string();
        st.password = "".to_string();

        let action = st.update(RegisterMessage::RegisterPressed);

        assert!(action.is_none());
        assert_eq!(
            st.error_message.as_deref(),
            Some("Usuario y contraseña son obligatorios")
        );
    }

    #[test]
    fn register_pressed_with_both_missing_sets_error() {
        let mut st = RegisterState::new();

        let action = st.update(RegisterMessage::RegisterPressed);

        assert!(action.is_none());
        assert_eq!(
            st.error_message.as_deref(),
            Some("Usuario y contraseña son obligatorios")
        );
    }

    #[test]
    fn register_pressed_with_valid_fields_and_empty_email_returns_action_with_none_email() {
        let mut st = RegisterState::new();
        st.username = "user".to_string();
        st.password = "pass".to_string();
        st.email = "".to_string();

        let action = st.update(RegisterMessage::RegisterPressed);

        match action {
            Some(RegisterAction::Register {
                username,
                password,
                email,
            }) => {
                assert_eq!(username, "user");
                assert_eq!(password, "pass");
                assert_eq!(email, None);
            }
            other => panic!("Expected RegisterAction::Register, got: {:?}", other),
        }
    }

    #[test]
    fn register_pressed_with_valid_fields_and_nonempty_email_returns_action_with_some_email() {
        let mut st = RegisterState::new();
        st.username = "user".to_string();
        st.password = "pass".to_string();
        st.email = "user@mail.com".to_string();

        let action = st.update(RegisterMessage::RegisterPressed);

        match action {
            Some(RegisterAction::Register {
                username,
                password,
                email,
            }) => {
                assert_eq!(username, "user");
                assert_eq!(password, "pass");
                assert_eq!(email.as_deref(), Some("user@mail.com"));
            }
            other => panic!("Expected RegisterAction::Register, got: {:?}", other),
        }
    }

    #[test]
    fn back_to_login_pressed_returns_back_action() {
        let mut st = RegisterState::new();

        let action = st.update(RegisterMessage::BackToLoginPressed);

        assert!(matches!(action, Some(RegisterAction::BackToLogin)));
    }

    #[test]
    fn set_error_sets_error_message() {
        let mut st = RegisterState::new();

        st.set_error("boom".to_string());

        assert_eq!(st.error_message.as_deref(), Some("boom"));
    }

    #[test]
    fn view_builds_without_panic_when_no_error() {
        let st = RegisterState::new();
        let _el: iced::Element<'_, RegisterMessage> = st.view();
    }

    #[test]
    fn view_builds_without_panic_when_error_present() {
        let mut st = RegisterState::new();
        st.error_message = Some("algo salió mal".to_string());

        let _el: iced::Element<'_, RegisterMessage> = st.view();
    }

    #[test]
    fn update_does_not_clear_existing_error_on_typing() {
        let mut st = RegisterState::new();
        st.error_message = Some("err".to_string());

        let _ = st.update(RegisterMessage::UsernameChanged("u".to_string()));
        let _ = st.update(RegisterMessage::PasswordChanged("p".to_string()));
        let _ = st.update(RegisterMessage::EmailChanged("e@mail.com".to_string()));

        assert_eq!(st.error_message.as_deref(), Some("err"));
    }
}
