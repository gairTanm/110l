use crate::debugger::Breakpoint;
use crate::dwarf_data::{DwarfData, Line};
use nix::sys::ptrace;
use nix::sys::signal;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};

pub enum Status {
    /// Indicates inferior stopped. Contains the signal that stopped the process, as well as the
    /// current instruction pointer that it is stopped at.
    Stopped(signal::Signal, usize),

    /// Indicates inferior exited normally. Contains the exit status code.
    Exited(i32),

    /// Indicates the inferior exited due to a signal. Contains the signal that killed the
    /// process.
    Signaled(signal::Signal),
}

/// This function calls ptrace with PTRACE_TRACEME to enable debugging on a process. You should use
/// pre_exec with Command to call this in the child process.
fn child_traceme() -> Result<(), std::io::Error> {
    ptrace::traceme().or(Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "ptrace TRACEME failed",
    )))
}

#[derive(Debug)]
pub struct Inferior {
    child: Child,
    breakpoint_map: HashMap<u64, Breakpoint>,
}

impl Inferior {
    pub fn new(target: &str, args: &Vec<String>, breakpoints: &Vec<u64>) -> Option<Inferior> {
        let mut cmd = Command::new(&target);
        let cmd = cmd.args(args);

        unsafe { cmd.pre_exec(child_traceme) };
        let child = cmd.spawn().expect("couldn't create the child process");

        let mut inferior = Inferior {
            child,
            breakpoint_map: HashMap::new(),
        };

        let status = inferior.wait(None).unwrap();
        match status {
            Status::Stopped(sig, _) if sig == nix::sys::signal::SIGTRAP => (),
            _ => return None,
        }

        for (idx, breakpoint) in breakpoints.iter().enumerate() {
            match inferior.add_breakpoint(*breakpoint) {
                Some(_) => println!("Set breakpoint {} at 0x{:#x}", idx, breakpoint),
                None => println!(
                    "WARNING: Cannot set breakpoint {} at 0x{:#x}!",
                    idx,
                    breakpoint
                ),
            }
        }
        Some(inferior)
    }

    pub fn add_breakpoint(&mut self, breakpoint_addr: u64) -> Option<Breakpoint> {
        let orig_byte = match self.write_byte(breakpoint_addr, 0xcc) {
            Ok(orig_byte) => orig_byte,
            Err(error) => {
                println!("Error while adding breakpoint: {:?}", error);
                0
            }
        };
        let mut breakpoint = match self.breakpoint_map.get(&breakpoint_addr) {
            Some(bp) => bp.clone(),
            None => Breakpoint::new(breakpoint_addr),
        };
        breakpoint.set_orig_byte(orig_byte);
        let _ = self
            .breakpoint_map
            .insert(breakpoint_addr, breakpoint.clone());

        Some(breakpoint)
    }

    pub fn kill(&mut self) -> () {
        self.child.kill().expect("couldn't kill the process");
        let status = self.child.wait().expect("failed to reap child");
        println!("Killed inferior process {} with {}", self.pid(), status);
    }

    fn write_byte(&mut self, addr: u64, val: u8) -> Result<u8, nix::Error> {
        let aligned_addr = align_addr_to_word(addr);
        let byte_offset = addr - aligned_addr;
        let word = ptrace::read(self.pid(), aligned_addr as ptrace::AddressType)? as u64;
        let orig_byte = (word >> 8 * byte_offset) & 0xff;
        let masked_word = word & !(0xff << 8 * byte_offset);
        let updated_word = masked_word | ((val as u64) << 8 * byte_offset);
        ptrace::write(
            self.pid(),
            aligned_addr as ptrace::AddressType,
            updated_word as *mut std::ffi::c_void,
        )?;
        Ok(orig_byte as u8)
    }

    pub fn cont(&mut self) -> Result<Status, nix::Error> {
        let mut regs = ptrace::getregs(self.pid()).unwrap();

        let mut rip = regs.rip as u64;
        rip -= 1;

        if self.breakpoint_map.contains_key(&rip) {
            let bp = self.breakpoint_map.get(&rip).unwrap().clone();
            let _ = self.write_byte(bp.get_addr(), bp.get_orig_byte());
            regs.rip -= 1;
            let _ = ptrace::setregs(self.pid(), regs);
            let _ = ptrace::step(self.pid(), None);
            let status = self.wait(None).unwrap();
            let is_sigtrap = match status {
                Status::Stopped(sig, _) => sig == nix::sys::signal::SIGTRAP,
                _ => false,
            };
            if !is_sigtrap {
                return Ok(status);
            }

            self.add_breakpoint(bp.get_addr());
        }

        let _ = ptrace::cont(self.pid(), None);
        let status = self.wait(None).unwrap();

        Ok(status)
    }

    /// Returns the pid of this inferior.
    pub fn pid(&self) -> Pid {
        nix::unistd::Pid::from_raw(self.child.id() as i32)
    }

    /// Calls waitpid on this inferior and returns a Status to indicate the state of the process
    /// after the waitpid call.
    pub fn wait(&self, options: Option<WaitPidFlag>) -> Result<Status, nix::Error> {
        Ok(match waitpid(self.pid(), options)? {
            WaitStatus::Exited(_pid, exit_code) => Status::Exited(exit_code),
            WaitStatus::Signaled(_pid, signal, _core_dumped) => Status::Signaled(signal),
            WaitStatus::Stopped(_pid, signal) => {
                let regs = ptrace::getregs(self.pid())?;
                Status::Stopped(signal, regs.rip as usize)
            }
            other => panic!("waitpid returned unexpected status: {:?}", other),
        })
    }

    pub fn print_backtrace(&self, dwarf_data: &DwarfData) -> Result<(), nix::Error> {
        let regs = ptrace::getregs(self.pid()).unwrap();

        let mut rbp = regs.rbp as usize;
        let mut rip = regs.rip as usize;
        let mut rbp_plus_8: usize;

        loop {
            let line = dwarf_data.get_line_from_addr(rip).unwrap_or(Line {
                file: String::from(""),
                number: 0,
                address: 0,
            });
            let function = dwarf_data
                .get_function_from_addr(rip)
                .unwrap_or(String::from("couldn't find the function"));

            println!("{} ({})", function, line);
            if function == String::from("main") {
                break;
            }
            rbp_plus_8 = rbp + 8;
            rip = ptrace::read(self.pid(), rbp_plus_8 as ptrace::AddressType)? as usize;
            rbp = ptrace::read(self.pid(), rbp as ptrace::AddressType)? as usize;
        }

        Ok(())
    }
}

fn align_addr_to_word(addr: u64) -> u64 {
    addr & (-(size_of::<u64>() as i64) as u64)
}
