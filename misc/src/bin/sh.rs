#![no_main]
#![no_std]
#![feature(asm)]

#[macro_use]
extern crate misc;

use core::str;
use misc::syscall;
use misc::uart;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut buf = [0u8; 256];
    println!("** Osh **");
    loop {
        print!("$ ");
        let (len, b) = uart::buffered_readline(&mut buf);
        if len == 0 {
            continue;
        }
        if !b {
            println!("Sorry. too long. Please enter shorter command");
            continue;
        }
        let cmd = match str::from_utf8(&buf) {
            Ok(s) => s,
            Err(_) => {
                println!("Failed to parse your input. Try again.");
                continue;
            }
        };
        if buf[0] == b'e' && buf[1] == b'x' && buf[2] == b'i' && buf[3] == b't'  && len == 4{
            syscall::sys_exit(0);
        }
        match syscall::sys_fork() {
            syscall::ForkResult::Parent(id) => {
                loop {
                    match syscall::sys_check_process_status(id) {
                        syscall::ProcessStatus::Running | syscall::ProcessStatus::Runnable => {
                            syscall::sys_yield();
                        },
                        _ => {
                            break;
                        }
                    }
                }
            },
            syscall::ForkResult::Fail => {
                println!("fork failed");
            },
            syscall::ForkResult::Child => {
                syscall::sys_execve(cmd, len as u32, &[], &[]);
            }
        }
    }
}