use std::env;
use std::io::Write;
use std::time::Duration;

const CONTROL_REQUEST: u8 = 0x8c;
const QUERY_REQUEST: u8 = 0x83;
const CATEGORY: u8 = 0x00;
const POWER_FUNCTION: u8 = 0x00;
const VOLUME_CONTROL_FUNCTION: u8 = 0x05;
const MUTING_FUNCTION: u8 = 0x06;

const RESPONSE_HEADER: u8 = 0x70;
const RESPONSE_ANSWER: u8 = 0x00;

// NOTE:  It's got nothing to do with Rust but I would consider naming the other byte constants
//    used in those commands

// NOTE:  When receiving a read-only set of bytes, it's more common to accept a slice (`&[u8]` in
//    this case) instead of a `&Vec` - that's a more flexible type.  A Vec will be coerced to a
//    slice when referenced with `&` at the callsite, or when `.as_ref()` is explicitly called.
fn checksum(command: &Vec<u8>) -> u8 {
    // FIXME:  The only thing I see that can be considered strictly "wrong" is uncertain/unclear
    //    behavior here around integer overflows - Iterator::sum() will panic on overflow in debug
    //    mode, and will quietly ignore it in --release builds.  The reason overflow happens is
    //    because you're adding a bunch of 8-bit integers together and keeping the result in an 8-bit
    //    integer.  This isn't necessarily a _problem_ per se in this use case though because you're
    //    taking a checksum - the modulo 255 at the end is essentially being done for you "for free".
    //    The thing I would do is to to return
    //    `command.iter().fold(0, |total, n| total.wrapping_add(n))`; in release builds, this
    //    doesn't change behavior, but in debug builds it will prevent panics.  In both cases, it
    //    makes intent more obvious to the reader.
    let s: u8 = command.iter().sum();

    // NOTE:  It's not wrong to explicitly use the `return` keyword, but in case you don't know,
    //    Rust will implicitly return the result of the final expression in a function (or any
    //    other block, for that matter) if you leave off the `return` keyword and the final
    //    semicolon
    return s % 255;
}

// NOTE:  You may already know, but you can avoid the boxed dynamic dispatch by taking a
//    `&mut impl serialport::Serialport` in these function; this is syntactic sugar for generics,
//    but with so-called "argument-position impl Trait" syntax, it saves a heap allocation, saves
//    an extra pointer chase every time the value is used, and is more common/idiomatic.  The
//    reality is that that kind of performance tuning super doesn't matter for a tool like this, so
//    I wouldn't consider it a big deal if I were reviewing this code in a business context.
// NOTE:  I just discovered that `serialport::SerialPortBuilder::open()` returns a
//    `Box<dyn SerialPort>`.  That changes things significantly.  I would, honestly, make a pull
//    request to the repo that changes it to return a concrete type, since (after reading their
//    code) the type is chosen at compile time based on the OS for which it's being compiled, not
//    based on run-time factors.
fn power_on(port: &mut Box<dyn serialport::SerialPort>) {
    let args = vec![CONTROL_REQUEST, CATEGORY, POWER_FUNCTION, 0x02, 0x01];
    write_command(port, args);
}

fn power_off(port: &mut Box<dyn serialport::SerialPort>) {
    let args = vec![CONTROL_REQUEST, CATEGORY, POWER_FUNCTION, 0x02, 0x00];
    write_command(port, args);
}

fn volume_up(port: &mut Box<dyn serialport::SerialPort>) {
    let args = vec![
        CONTROL_REQUEST,
        CATEGORY,
        VOLUME_CONTROL_FUNCTION,
        0x03,
        0x00,
        0x00,
    ];
    write_command(port, args);
}

fn volume_down(port: &mut Box<dyn serialport::SerialPort>) {
    let args = vec![
        CONTROL_REQUEST,
        CATEGORY,
        VOLUME_CONTROL_FUNCTION,
        0x03,
        0x00,
        0x01,
    ];
    write_command(port, args);
}

fn mute_toggle(port: &mut Box<dyn serialport::SerialPort>) {
    let args = vec![CONTROL_REQUEST, CATEGORY, MUTING_FUNCTION, 0x02, 0x00];
    write_command(port, args);
}

fn is_powered_on(port: &mut Box<dyn serialport::SerialPort>) -> bool {
    let args = vec![QUERY_REQUEST, CATEGORY, POWER_FUNCTION, 0xff, 0xff];
    let data = write_command(port, args);
    return data[0] == 1;
}

fn power_toggle(port: &mut Box<dyn serialport::SerialPort>) {
    if is_powered_on(port) {
        println!("is on - turning off!");
        power_off(port);
    } else {
        println!("is off - turning on!");
        power_on(port);
    }
}

fn print_status(port: &mut Box<dyn serialport::SerialPort>) {
    if is_powered_on(port) {
        println!("Power: on");
    } else {
        println!("Power: off");
    }
}

fn print_usage() {
    eprintln!("usage: DEVICE [on|off|power|volume-up|volume-down|mute|status]");
}

// NOTE:  With the "contents" variable, you can avoid more allocation(s) in a few different ways,
//    depending on how far you wanted to take it.
//      * You can make `contents` itself mutable, since `write_command()` is taking ownership of it,
//        and skip cloning before you push the checksum onto it.
//      * You can go a step farther and not make Vecs at all; you can do static slices in the
//        individual command functions above with `&[]` syntax, accept `contents` as a `&[u8]`, and
//        separately `port.write_all(contents)` and `port.write_all(&[c])`; that would need to be
//        tested to make sure that it doesn't alter timing too significantly as to break the
//        receiving device; if it does, precalculating the checksums and including them in those
//        static slices is an option.  Personally, I would probably stick with a Vec over
//        precalculating the checksums; the readability is more important than the performance IMO.
// NOTE:  Another change that might make sense is to return a `Result<Vec<u8>, SomeError>` from this
//    function and from the various command functions, and let `main()` be responsible for reporting
//    errors and terminating nonzero.  One very simple option for the Error type is simply
//    `&'static str`; then you can e.g. `return Err("unexpected response header")`
fn write_command(port: &mut Box<dyn serialport::SerialPort>, contents: Vec<u8>) -> Vec<u8> {
    let mut vec = contents.clone();
    let c = checksum(&vec);
    vec.push(c);
    port.write_all(&vec).unwrap();

    let mut resp_buf = vec![0; 3];
    // NOTE:  `Vec::as_mut_slice()` isn't wrong here and it's what I would use, but JSYK when
    //    reading other projects, `&mut resp_buf[..]` is another way to accomplish the same thing
    //    that is used pretty commonly.
    port.read(resp_buf.as_mut_slice())
        .expect("failure to read response");

    if resp_buf[0] != RESPONSE_HEADER {
        eprintln!("error: unexpected response header");
        std::process::exit(1);
    }
    if resp_buf[1] != RESPONSE_ANSWER {
        eprintln!("error: unexpected response answer");
        std::process::exit(1);
    }
    if vec[0] == QUERY_REQUEST {
        let mut resp_data_buf = vec![0; resp_buf[2] as usize];
        port.read(resp_data_buf.as_mut_slice())
            .expect("failure to read response data");
        let resp_checksum = resp_data_buf.pop().expect("error");
        // NOTE:  You can avoid the clone here by passing `&resp_data_buf` to `resp_buf.extend()`
        resp_buf.extend(resp_data_buf.clone());
        if resp_checksum != checksum(&resp_buf) {
            eprintln!("error: invalid response checksum");
            std::process::exit(1);
        }
        return resp_data_buf;
    } else {
        let resp_checksum = resp_buf.pop().expect("error");
        if resp_checksum != checksum(&resp_buf) {
            eprintln!("error: invalid response checksum");
            std::process::exit(1);
        }
        // NOTE:  This isn't wrong, but you can replace this with simply `vec![]` - for an empty
        //    one you don't need to explicitly pass a length and fill value
        return vec![0; 0];
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    match args.len() {
        3 => {}
        _ => {
            print_usage();
            eprintln!("error: unexpected argument(s)");
            std::process::exit(1);
        }
    }

    let mut port = serialport::new(&args[1], 9600)
        .timeout(Duration::from_millis(500))
        .open()
        .expect("Failed to open port.");
    match &args[2][..] {
        "on" => power_on(&mut port),
        "off" => power_off(&mut port),
        "power" => power_toggle(&mut port),
        "volume-up" => volume_up(&mut port),
        "volume-down" => volume_down(&mut port),
        "mute" => mute_toggle(&mut port),
        "status" => print_status(&mut port),
        _ => {
            eprintln!("error: invalid action");
            std::process::exit(1);
        }
    };
}
