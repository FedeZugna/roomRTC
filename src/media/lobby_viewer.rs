use iced::widget::{Button, button, column, container, text};
use iced::{Element, Length, Subscription, Task, Theme};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone)]
pub enum LobbyMessage {
    Join, // Unirse a la llamada
}

pub struct Lobby {
    join_flag: Arc<AtomicBool>,
}

impl Lobby {
    pub fn new(join_flag: Arc<AtomicBool>) -> Self {
        Self { join_flag }
    }

    pub fn update(&mut self, message: LobbyMessage) -> Task<LobbyMessage> {
        match message {
            LobbyMessage::Join => {
                // Marcamos que el usuario aceptó unirse
                self.join_flag.store(true, Ordering::SeqCst);

                // Cerramos la app de Iced (se termina la ventana del lobby)
                iced::exit()
            }
        }
    }

    pub fn view(&self) -> Element<'_, LobbyMessage> {
        let join_button: Button<LobbyMessage> = button(text("Unirse a la llamada"))
            .padding(10)
            .on_press(LobbyMessage::Join);

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

    pub fn subscription(&self) -> Subscription<LobbyMessage> {
        // No necesitamos nada especial acá; si el usuario cierra la ventana
        // sin tocar "Unirse", simplemente join_flag queda en false.
        Subscription::none()
    }

    pub fn theme(&self) -> Theme {
        Theme::Dark
    }
}

pub fn run_lobby(join_flag: Arc<AtomicBool>) -> iced::Result {
    let flag_for_lobby = Arc::clone(&join_flag);

    iced::application("Lobby de la reunión", Lobby::update, Lobby::view)
        .subscription(Lobby::subscription)
        .theme(Lobby::theme)
        .run_with(move || (Lobby::new(flag_for_lobby), Task::none()))
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn update_join_sets_flag_true() {
        let join_flag = Arc::new(AtomicBool::new(false));
        let mut lobby = Lobby::new(Arc::clone(&join_flag));

        let _task = lobby.update(LobbyMessage::Join);

        assert!(join_flag.load(Ordering::SeqCst));
    }

    #[test]
    fn update_join_is_idempotent() {
        let join_flag = Arc::new(AtomicBool::new(false));
        let mut lobby = Lobby::new(Arc::clone(&join_flag));

        let _ = lobby.update(LobbyMessage::Join);
        let _ = lobby.update(LobbyMessage::Join);

        assert!(join_flag.load(Ordering::SeqCst));
    }

    #[test]
    fn theme_is_dark() {
        let join_flag = Arc::new(AtomicBool::new(false));
        let lobby = Lobby::new(join_flag);

        assert_eq!(lobby.theme(), Theme::Dark);
    }

    #[test]
    fn subscription_smoke() {
        let join_flag = Arc::new(AtomicBool::new(false));
        let lobby = Lobby::new(join_flag);

        // Este test solo asegura que se crea sin panics.
        let _sub = lobby.subscription();
    }

    #[test]
    fn view_builds_without_panic() {
        let join_flag = Arc::new(AtomicBool::new(false));
        let lobby = Lobby::new(join_flag);

        // Construir el Element no debería panic.
        let _el = lobby.view();
    }

    #[test]
    fn view_does_not_toggle_flag_by_itself() {
        let join_flag = Arc::new(AtomicBool::new(false));
        let lobby = Lobby::new(Arc::clone(&join_flag));

        let _ = lobby.view();

        // Solo debería cambiar con update(Join)
        assert!(!join_flag.load(Ordering::SeqCst));
    }

}
