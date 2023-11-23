use core::str::FromStr;
use std::io::Write;
use std::time::Duration;

use clap::Parser;
use serialport::SerialPort;

mod error;
use error::CommandFailure;
use error::InvalidCommand;

const CONTROL_REQUEST: u8 = 0x8c;
const QUERY_REQUEST: u8 = 0x83;
const CATEGORY: u8 = 0x00;
const POWER_FUNCTION: u8 = 0x00;
const VOLUME_CONTROL_FUNCTION: u8 = 0x05;
const MUTING_FUNCTION: u8 = 0x06;

const RESPONSE_HEADER: u8 = 0x70;
const RESPONSE_ANSWER: u8 = 0x00;

#[derive(Debug, Parser)]
struct Config {
    /// Path to serial port device
    device: String,
    /// [on|off|power|volume-up|volume-down|volume:level|mute|status]
    command: UserCommand,
}

// TODO:  VolumeSpecific is written in the assumption that a specific volume is set using a value
// between 0 and 255; I have no idea if that assumption is correct.
#[derive(Debug, Clone, Eq, PartialEq)]
enum UserCommand {
    PowerOn,
    PowerOff,
    PowerToggle,
    VolumeUp,
    VolumeDown,
    VolumeSpecific(u8),
    ToggleMute,
    GetPowerStatus,
}

impl FromStr for UserCommand {
    type Err = InvalidCommand;
    // The Err cases will both allocate new strings with a copy of input, or a substring of input
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "on" => Ok(Self::PowerOn),
            "off" => Ok(Self::PowerOff),
            "power" => Ok(Self::PowerToggle),
            "volume-up" => Ok(Self::VolumeUp),
            "volume-down" => Ok(Self::VolumeDown),
            "mute" => Ok(Self::ToggleMute),
            "status" => Ok(Self::GetPowerStatus),
            s => match s.strip_prefix("volume:") {
                Some(level) => match u8::from_str(level) {
                    Ok(v) => Ok(Self::VolumeSpecific(v)),
                    Err(_) => Err(InvalidCommand::InvalidSpecificVolume(level.to_owned())),
                },
                None => Err(InvalidCommand::UnknownCommand(s.to_owned())),
            },
        }
    }
}

impl UserCommand {
    fn execute(&self, port: &mut Box<dyn SerialPort>) -> Result<(), CommandFailure> {
        let command = match self {
            Self::PowerOn => DeviceCommand::PowerOn,
            Self::PowerOff => DeviceCommand::PowerOff,
            Self::PowerToggle => {
                let result = DeviceCommand::GetPowerStatus.execute(port)?;
                if response_indicates_powered_on(&result) {
                    println!("is on - turning off");
                    DeviceCommand::PowerOff
                } else {
                    println!("is off - turning on");
                    DeviceCommand::PowerOn
                }
            }
            Self::VolumeUp => DeviceCommand::VolumeUp,
            Self::VolumeDown => DeviceCommand::VolumeDown,
            Self::VolumeSpecific(v) => DeviceCommand::VolumeSpecific(*v),
            Self::ToggleMute => DeviceCommand::ToggleMute,
            Self::GetPowerStatus => DeviceCommand::GetPowerStatus,
        };

        let result = command.execute(port)?;

        if self == &Self::GetPowerStatus {
            if response_indicates_powered_on(&result) {
                println!("Power: on");
            } else {
                println!("Power: off");
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
enum DeviceCommand {
    PowerOn,
    PowerOff,
    VolumeUp,
    VolumeDown,
    VolumeSpecific(u8),
    ToggleMute,
    GetPowerStatus,
}

impl DeviceCommand {
    // If you want to go nuts with truly dumb optimizations, you can have this return a
    // `Cow<'static, [u8]>` so that cases other than `VolumeSpecific` don't need to perform an
    // allocation; they wouldn't need to perform an allocation because the complete binary command
    // is known statically.
    fn as_bytes(&self) -> Vec<u8> {
        match self {
            Self::PowerOn => vec![CONTROL_REQUEST, CATEGORY, POWER_FUNCTION, 0x02, 0x01],
            Self::PowerOff => vec![CONTROL_REQUEST, CATEGORY, POWER_FUNCTION, 0x02, 0x00],
            Self::VolumeUp => vec![
                CONTROL_REQUEST,
                CATEGORY,
                VOLUME_CONTROL_FUNCTION,
                0x03,
                0x00,
                0x00,
            ],
            Self::VolumeDown => vec![
                CONTROL_REQUEST,
                CATEGORY,
                VOLUME_CONTROL_FUNCTION,
                0x03,
                0x00,
                0x01,
            ],
            Self::VolumeSpecific(v) => vec![
                CONTROL_REQUEST,
                CATEGORY,
                VOLUME_CONTROL_FUNCTION,
                /* TODO: ??? */
            ],
            Self::ToggleMute => vec![CONTROL_REQUEST, CATEGORY, MUTING_FUNCTION, 0x02, 0x00],
            Self::GetPowerStatus => vec![QUERY_REQUEST, CATEGORY, POWER_FUNCTION, 0xff, 0xff],
        }
    }

    fn execute(&self, port: &mut Box<dyn SerialPort>) -> Result<Vec<u8>, CommandFailure> {
        let raw = self.as_bytes();
        let c = checksum(&raw);
        port.write_all(&raw).map_err(CommandFailure::WriteCommand)?;
        port.write_all(&[c])
            .map_err(CommandFailure::WriteChecksum)?;

        let mut resp_buf = vec![0; 3];
        port.read(resp_buf.as_mut_slice())
            .map_err(CommandFailure::ReadResponse)?;

        if resp_buf[0] != RESPONSE_HEADER {
            return Err(CommandFailure::UnexpectedResponseHeader(resp_buf[0]));
        }
        if resp_buf[1] != RESPONSE_ANSWER {
            return Err(CommandFailure::UnexpectedResponseAnswer(resp_buf[1]));
        }
        if raw[0] == QUERY_REQUEST {
            let mut resp_data_buf = vec![0; resp_buf[2] as usize];
            port.read(resp_data_buf.as_mut_slice())
                .map_err(CommandFailure::ReadResponseData)?;
            let resp_checksum = resp_data_buf.pop().ok_or(CommandFailure::EmptyResponse)?;
            resp_buf.extend(&resp_data_buf);
            if resp_checksum != checksum(&resp_buf) {
                return Err(CommandFailure::InvalidResponseChecksum);
            }
            Ok(resp_data_buf)
        } else {
            let resp_checksum = resp_buf.pop().ok_or(CommandFailure::EmptyResponse)?;
            if resp_checksum != checksum(&resp_buf) {
                return Err(CommandFailure::InvalidResponseChecksum);
            }
            Ok(vec![])
        }
    }
}

fn checksum(command: &[u8]) -> u8 {
    command.iter().fold(0, |total, n| total.wrapping_add(*n))
}

fn response_indicates_powered_on(response: &[u8]) -> bool {
    match response {
        [] => false,
        [1, ..] => true,
        _ => false,
    }
}

fn main() {
    let config = Config::parse();

    let mut port = serialport::new(&config.device, 9600)
        .timeout(Duration::from_millis(500))
        .open()
        .expect("Failed to open port.");
    match config.command.execute(&mut port) {
        Ok(()) => println!("OK"),
        Err(e) => {
            eprintln!("{e:?}");
            std::process::exit(1);
        }
    }
}
