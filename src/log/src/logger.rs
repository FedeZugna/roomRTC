//! Este modulo contiene la implementacion de la estructura logger,
//! encacargada de registrar mensajes al archivo de log de Redis
use chrono;
use std::{
    fmt::Display,
    fs::OpenOptions,
    io::Write,
    sync::mpsc::{Receiver, Sender, channel},
    thread::spawn,
};
/// Struct to hold the logger sender
#[derive(Debug, Clone)]
pub struct Logger {
    /// Sender to send log messages
    pub logger: Sender<String>,
    /// Context prefix (e.g., "SERVER", "CLIENTE")
    context_prefix: String,
}

impl Logger {
    /// Constructor de la estructura.
    /// Mueve la escritura de mensaje a un hilo, permitiendo
    /// recibir mensajes y escribir simultaneamente
    /// # Parametros
    /// * `context`: Prefijo de contexto (e.g., "SERVER", "CLIENTE")
    pub fn new(context: &str) -> Logger {
        let (logger, receiver) = channel();
        let filename = "logger.log".to_string();
        let context_prefix = context.to_string();

        spawn(move || {
            write_to_file(&filename, receiver);
        });

        Logger {
            logger,
            context_prefix,
        }
    }

    pub fn null() -> Self {
        let (sender, receiver) = channel::<String>();
        spawn(move || for _ in receiver {});
        Self {
            logger: sender,
            context_prefix: "NULL".to_string(),
        }
    }

    /// Registra un mensaje de tipo info
    /// # Parametros
    /// * `message`: mensaje a registrar
    /// * `module`: unidad logica del programa que emite la informacion
    pub fn info(&self, message: &str, module: &str) {
        let now = chrono::Local::now();
        let timestamp = now.format("%Y-%m-%d %H:%M:%S").to_string();
        let log_message = format!(
            "[{}] - [INFO] - {timestamp} - {module}: {message}",
            self.context_prefix
        );
        self.log(log_message.as_str());
    }

    /// Registra un mensaje de tipo error
    /// # Parametros
    /// * `message`: mensaje a registrar
    /// * `module`: unidad logica del programa donde se produjo el error
    pub fn error(&self, message: &str, module: &str) {
        let now = chrono::Local::now();
        let timestamp = now.format("%Y-%m-%d %H:%M:%S").to_string();
        let log_message = format!(
            "[{}] - [ERROR] - {timestamp} - {module}: {message}",
            self.context_prefix
        );
        self.log(log_message.as_str());
    }

    /// Registra un mensaje de tipo warning
    /// # Parametros
    /// * `message`: mensaje a registrar
    /// * `module`: unidad logica del programa que emite la informacion
    pub fn warn(&self, message: &str, module: &str) {
        let now = chrono::Local::now();
        let timestamp = now.format("%Y-%m-%d %H:%M:%S").to_string();
        let log_message = format!(
            "[{}] - [WARNING] - {timestamp} - {module}: {message}",
            self.context_prefix
        );
        self.log(log_message.as_str());
    }

    /// Registra un mensaje de tipo DEBUG (muestra por consola)
    /// # Parametros
    /// * `message`: mensaje a registrar
    /// * `module`: unidad logica del programa que emite la informacion
    /// * `data`: informacion del caso particular
    pub fn debug<T: Display>(&self, message: &str, module: &str, data: T) {
        let now = chrono::Local::now();
        let timestamp = now.format("%Y-%m-%d %H:%M:%S").to_string();
        let log_message = format!(
            "[{}] - [DEBUG] - {timestamp} - {module}: {message} - {data}",
            self.context_prefix
        );
        self.log(log_message.as_str());
    }

    /// Envia un mensaje por el canal del logger
    /// # Parametros
    /// * `message`: mensaje a registrar
    fn log(&self, log_message: &str) {
        match self.logger.send(log_message.to_string()) {
            Ok(_) => {}
            Err(_) => {
                eprintln!("Error sending log message");
            }
        }
    }
}

/// Recibe mensajes y los escribe en un archivo con el nombre recibido por
/// parametro
///
/// # Parametros
/// * `filename`: nombre del archivo a crear para registrar los mensajes
/// * `reciever`: extremo de recepcion del canal que comparte con la
///   estructura principal por donde recibe los mensajes
fn write_to_file(filename: &str, receiver: Receiver<String>) {
    let file = OpenOptions::new().create(true).append(true).open(filename);

    let mut file = match file {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Error opening log file: {e}");
            return;
        }
    };

    for message in receiver {
        if let Err(e) = writeln!(file, "{message}") {
            eprintln!("Error writing to log file: {e}");
        }
    }
}
