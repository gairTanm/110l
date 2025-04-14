use crate::debugger_command::DebuggerCommand;
use crate::dwarf_data::{DwarfData, Error as DwarfError, Line};
use crate::inferior::{Inferior, Status};
use rustyline::error::ReadlineError;
use rustyline::Editor;

pub struct Debugger {
    target: String,
    history_path: String,
    readline: Editor<()>,
    debug_data: DwarfData,
    breakpoints: Vec<u64>,
    inferior: Option<Inferior>,
}

#[derive(Clone, Debug)]
pub struct Breakpoint {
    addr: u64,
    orig_byte: u8,
}

impl Breakpoint {
    pub fn new(addr: u64) -> Breakpoint {
        return Breakpoint { addr, orig_byte: 0 };
    }

    pub fn set_orig_byte(&mut self, orig_byte: u8) {
        self.orig_byte = orig_byte;
    }

    pub fn get_orig_byte(&self) -> u8 {
        self.orig_byte
    }
    pub fn get_addr(&self) -> u64 {
        self.addr
    }
}

impl Debugger {
    /// Initializes the debugger.
    pub fn new(target: &str) -> Debugger {
        // TODO (milestone 3): initialize the DwarfData

        let debug_data = match DwarfData::from_file(target) {
            Ok(val) => val,
            Err(DwarfError::ErrorOpeningFile) => {
                println!("Could not open file {}", target);
                std::process::exit(1);
            }
            Err(DwarfError::DwarfFormatError(err)) => {
                println!("Could not debugging symbols from {}: {:?}", target, err);
                std::process::exit(1);
            }
        };

        debug_data.print();

        let history_path = format!("{}/.deet_history", std::env::var("HOME").unwrap());
        let mut readline = Editor::<()>::new();
        // Attempt to load history from ~/.deet_history if it exists
        let _ = readline.load_history(&history_path);

        Debugger {
            target: target.to_string(),
            history_path,
            readline,
            debug_data,
            breakpoints: vec![],
            inferior: None,
        }
    }

    fn run_from_cont(&mut self) {
        if self.inferior.is_none() {
            println!("Error: not tracking any process");
            return;
        }
        let status = self.inferior.as_mut().unwrap().cont().unwrap();
        match status {
            Status::Signaled(sig) => println!("\nChild signaled (signal {})", sig),
            Status::Exited(code) => {
                println!("Child exited (status {})", code);
                self.inferior = None;
            }
            Status::Stopped(sig, line_info) => {
                println!("Child stopped (signal {})", sig);
                if sig == nix::sys::signal::SIGTRAP {
                    println!(
                        "Stopped at {}",
                        self.debug_data
                            .get_line_from_addr(line_info)
                            .unwrap_or(Line {
                                file: String::from(""),
                                number: 0,
                                address: 0,
                            })
                    );
                }
            }
        }
    }

    fn flush_inferior(&mut self) {
        if self.inferior.is_some() {
            self.inferior.as_mut().unwrap().kill();
            self.inferior = None;
        }
    }

    pub fn run(&mut self) {
        loop {
            match self.get_next_command() {
                DebuggerCommand::Run(args) => {
                    self.flush_inferior();
                    if let Some(inferior) =
                        Inferior::new(&self.target, &args, &mut self.breakpoints)
                    {
                        self.inferior = Some(inferior);
                        self.run_from_cont();
                    } else {
                        println!("Error starting subprocess");
                    }
                }
                DebuggerCommand::Quit => {
                    self.flush_inferior();
                    return;
                }
                DebuggerCommand::Continue => {
                    self.run_from_cont();
                }
                DebuggerCommand::Backtrace => {
                    self.inferior
                        .as_mut()
                        .unwrap()
                        .print_backtrace(&self.debug_data)
                        .unwrap();
                }
                DebuggerCommand::AddBreakpoint(arg) => {
                    let target_addr = parse_address(&arg.to_string()).unwrap_or(0);
                    self.breakpoints.push(target_addr);
                    println!("Set breakpoint {} at {}", self.breakpoints.len() - 1, arg);
                    self.add_breakpoint_to_process(target_addr);
                }
            }
        }
    }

    fn add_breakpoint_to_process(&mut self, breakpoint: u64) {
        if self.inferior.is_some() {
            self.inferior.as_mut().unwrap().add_breakpoint(breakpoint);
        }
    }

    fn get_next_command(&mut self) -> DebuggerCommand {
        loop {
            // Print prompt and get next line of user input
            match self.readline.readline("(deet) ") {
                Err(ReadlineError::Interrupted) => {
                    // User pressed ctrl+c. We're going to ignore it
                    println!("Type \"quit\" to exit");
                }
                Err(ReadlineError::Eof) => {
                    // User pressed ctrl+d, which is the equivalent of "quit" for our purposes
                    return DebuggerCommand::Quit;
                }
                Err(err) => {
                    panic!("Unexpected I/O error: {:?}", err);
                }
                Ok(line) => {
                    if line.trim().len() == 0 {
                        continue;
                    }
                    self.readline.add_history_entry(line.as_str());
                    if let Err(err) = self.readline.save_history(&self.history_path) {
                        println!(
                            "Warning: failed to save history file at {}: {}",
                            self.history_path, err
                        );
                    }
                    let tokens: Vec<&str> = line.split_whitespace().collect();
                    if let Some(cmd) = DebuggerCommand::from_tokens(&tokens) {
                        return cmd;
                    } else {
                        println!("Unrecognized command.");
                    }
                }
            }
        }
    }
}

fn parse_address(addr: &str) -> Option<u64> {
    let addr_without_0x = if addr.to_lowercase().starts_with("*0x") {
        &addr[3..]
    } else {
        &addr
    };
    u64::from_str_radix(addr_without_0x, 16).ok()
}
