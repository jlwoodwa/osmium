use syscall;
use core::fmt::Write;

struct UART;
impl Write for UART {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        syscall::sys_write(s.as_bytes(), s.len());
        Ok(())
    }
}

pub fn print(arg: ::core::fmt::Arguments) {
    UART.write_fmt(arg).expect("failed to send by UART");
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::uart::print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => (print!("\n"));
    ($arg:expr) => (print!(concat!($arg, "\n")));
    ($fmt:expr, $($arg:tt)*) => (print!(concat!($fmt, "\n"), $($arg)*));
}


pub fn buffered_readline(buffer: &mut [u8]) -> (usize, bool) {
    let l = buffer.len();
    let mut buf = [0u8; 1];
    for i in 0..l {
        syscall::sys_read(&mut buf, 1);
        if buf[0] == b'\n' {
            return (i, true);
        }
        buffer[i] = buf[0];
    }
    (l, false)
}