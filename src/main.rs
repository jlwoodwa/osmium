#![feature(global_asm)]
#![feature(core_intrinsics)]
#![no_std]
#![no_main]
#![feature(asm)]

#[macro_use]
extern crate bitflags;
extern crate array_init;

#[macro_use]
pub mod uart;
pub mod csr;
pub mod elf;
pub mod files;
pub mod memlayout;
pub mod memutil;
pub mod paging;
pub mod proc;
pub mod trap;
pub mod utils;

use core::panic::PanicInfo;
use csr::satp;

extern "C" {
    static kernel_end: u8;
    static mut kernel_pgdir_ptr: u32;
    static mut temporary_pgdir_ptr: u32;
    static mut kernel_frames_ptr: u32;
    static mut stack_stop: u8;
}

const IO_REGION: u64 = 0x80000000;

fn get_kernel_end_addr() -> u64 {
    unsafe { (&kernel_end as *const u8) as u64 }
}

struct BootAlloc<'a> {
    pub procs: &'a mut [proc::Process<'a>; proc::N_PROCS],
    pub proc_pages: &'a mut [paging::PageTable; proc::N_PROCS],
    pub proc_tmp_pages: &'a mut [paging::PageTable; proc::N_PROCS],
}

// must call before memory management in order to reserve envs memory.
fn boot_alloc<'a>() -> (u64, BootAlloc<'a>) {
    let end = utils::round_up(get_kernel_end_addr(), paging::PGSIZE as u64);
    println!("end {:x}", get_kernel_end_addr());

    let proc_pages = unsafe { &mut *(end as *mut [paging::PageTable; proc::N_PROCS]) };
    let end = end + (paging::PGSIZE * proc::N_PROCS) as u64;

    let proc_tmp_pages = unsafe { &mut *(end as *mut [paging::PageTable; proc::N_PROCS]) };
    let end = end + (paging::PGSIZE * proc::N_PROCS) as u64;

    let procs = unsafe { &mut *(end as *mut [proc::Process; proc::N_PROCS]) };
    let end = end + (proc::N_PROCS as u64) * (proc::Process::size_of() as u64);

    let end = utils::round_up(end, paging::PGSIZE as u64);

    (
        end,
        BootAlloc {
            procs,
            proc_pages,
            proc_tmp_pages,
        },
    )
}

struct Kernel<'a> {
    kernel_pgdir: &'a mut paging::PageTable,
    mapper: paging::Map<'a>,
    allocator: paging::Allocator<'a>,
}

#[no_mangle]
pub extern "C" fn __start_rust() -> ! {
    //println!("hello\n\n");

    /*println!("enter two uint8_t values a, b please");
    let a = (uart::read_byte() - 0x30) as u32;
    let _ = uart::read_byte();
    let b = (uart::read_byte() - 0x30) as u32;
    let _ = uart::read_byte();
    println!("{} / {} = {}", a, b, a / b);
    println!("{} % {} = {}", a, b, a % b);
    println!("enter two int8_t values a, b please");
    let mut a = a as i32;
    let mut b = b as i32;

    println!("{} / {} = {}", a, b, a / b);
    println!("{} % {} = {}", a, b, a % b);
    a = -1i32 * a;
    println!("{} / {} = {}", a, b, a / b);
    println!("{} % {} = {}", a, b, a % b);
    b = -1i32 * b;
    println!("{} / {} = {}", a, b, a / b);
    println!("{} % {} = {}", a, b, a % b);
    a = -1i32 * a;
    println!("{} / {} = {}", a, b, a / b);
    println!("{} % {} = {}", a, b, a % b);
    */

    // setup kernel page table
    let kern_pgdir =
        unsafe { &mut *((&mut kernel_pgdir_ptr as *mut u32) as *mut paging::PageTable) };
    let kern_pgdir_addr = (kern_pgdir as *const paging::PageTable) as u32;
    let kern_tmp_pgdir =
        unsafe { &mut *((&mut temporary_pgdir_ptr as *mut u32) as *mut paging::PageTable) };

    paging::PageTable::setup_tmp_table(kern_pgdir, kern_tmp_pgdir);

    let kernel_frames = unsafe { (&mut kernel_frames_ptr as *mut u32) };
    println!("kern frames addr {:p}", kernel_frames);

    let mut mapper = paging::Map::new(kern_pgdir, kern_tmp_pgdir);
    println!("mapper created");

    let (kernel_memory_end, allocated) = boot_alloc();
    let is_used = |addr| {
        if (addr as u64) < kernel_memory_end + (paging::PGSIZE as u64) {
            return true;
        }
        if addr as u64 >= IO_REGION {
            return true;
        }
        false
    };
    let mut allocator = unsafe { paging::Allocator::new(kernel_frames, &is_used) };
    println!("allocator created");

    println!("envs start with {:x}", get_kernel_end_addr());
    if let Err(e) = mapper.boot_map_region(
        paging::VirtAddr::new(0),
        paging::PhysAddr::new(0),
        kernel_memory_end as usize,
        paging::Flag::READ | paging::Flag::WRITE | paging::Flag::EXEC | paging::Flag::VALID,
        &mut allocator,
    ) {
        panic!("Failed to map kernel region. Reason: {:?}", e);
    }
    println!("kernel mapping created");

    if let Err(e) = mapper.boot_map_region(
        paging::VirtAddr::new(unsafe { &stack_stop as *const u8 as u32 }),
        paging::PhysAddr::new(unsafe { &stack_stop as *const u8 as u64 }),
        paging::PGSIZE,
        paging::Flag::empty(),
        &mut allocator,
    ) {
        panic!("Failed to map kernel region. Reason: {:?}", e);
    }

    if let Err(e) = mapper.boot_map_region(
        paging::VirtAddr::new(IO_REGION as u32),
        paging::PhysAddr::new(IO_REGION),
        paging::PGSIZE,
        paging::Flag::READ | paging::Flag::WRITE | paging::Flag::EXEC | paging::Flag::VALID,
        &mut allocator,
    ) {
        panic!("Failed to map io region. Reason: {:?}", e);
    }
    println!("io mapping created");

    satp::SATP::set_ppn(kern_pgdir_addr >> paging::LOG_PGSIZE);
    satp::SATP::enable_paging();

    println!("kernel space (identity) paging works!");

    println!("Let's create an user process");
    let mut process_manager = proc::ProcessManager::new(
        allocated.procs,
        allocated.proc_pages,
        allocated.proc_tmp_pages,
    );
    let process = unsafe {
        &mut *(process_manager
            .alloc()
            .expect("failed to alloc process(program error)"))
    };
    process.create(&mut allocator, &mut mapper);

    // use proc's page table
    satp::SATP::set_ppn(process.ppn());

    let nop_file = match files::search("nop") {
        Some(file) => file,
        None => panic!("failed to find nop"),
    };

    println!("nop_file bytes: {}", nop_file.bytes as *const u8 as usize);
    let nop_elf = elf::Elf::new(nop_file.bytes).expect("failed to parse ELF");

    match process.load_elf(&nop_elf, &mut allocator) {
        Ok(()) => (),
        Err(e) => panic!("failed to load elf: {}", e),
    };
    let tf = trap::TrapFrame::new(nop_elf.elf.entry, memlayout::USER_SATCK_BOTTOMN);
    process.set_trap_frame(tf);
    process.run();
    println!("ok");
    loop {}
}

#[panic_handler]
#[no_mangle]
pub fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop {}
}

#[no_mangle]
pub extern "C" fn abort() -> ! {
    loop {}
}
