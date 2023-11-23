#[derive(Debug, thiserror::Error)]
pub enum InvalidCommand {
    #[error("Unknown command '{0}'")]
    UnknownCommand(String),
    #[error("Invalid volume level '{0}'; must be an integer [0-255]")]
    InvalidSpecificVolume(String),
}

#[derive(Debug, thiserror::Error)]
pub enum CommandFailure {
    #[error("Failed to send command across serial port:  {0}")]
    WriteCommand(std::io::Error),
    #[error("Failed to send comamnd checksum across serial port:  {0}")]
    WriteChecksum(std::io::Error),
    #[error("Unexpected response header:  {0:#04x}")]
    UnexpectedResponseHeader(u8),
    #[error("Unexpected response answer:  {0:#04x}")]
    UnexpectedResponseAnswer(u8),
    #[error("Failed to read response from serial port:  {0}")]
    ReadResponse(std::io::Error),
    #[error("Failed to read response data from serial port:  {0}")]
    ReadResponseData(std::io::Error),
    #[error("Empty response")]
    EmptyResponse,
    #[error("Response checksum was not correct")]
    InvalidResponseChecksum,
}
