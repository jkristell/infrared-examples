#![no_std]
#![no_main]

use cortex_m::asm;
use cortex_m_rt::entry;
use rtt_target::{rprintln, rtt_init_print};
use stm32f1xx_hal::{
    gpio::{gpiob::PB8, Floating, Input},
    pac,
    prelude::*,
    stm32::{interrupt, TIM2},
    timer::{CountDownTimer, Event, Timer},
};

#[allow(unused_imports)]
use infrared::{
    hal::PeriodicReceiver,
    protocols::Nec,
    remotes::{nec::*, rc5::*},
    Button,
};

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    rprintln!("{}", info);
    exit()
}

fn exit() -> ! {
    loop {
        asm::bkpt() // halt = exit probe-run
    }
}

// Pin connected to the receiver
type RecvPin = PB8<Input<Floating>>;
// Samplerate
const SAMPLERATE: u32 = 20_000;
// Our timer. Needs to be accessible in the interrupt handler.
static mut TIMER: Option<CountDownTimer<TIM2>> = None;
// Our Infrared receiver
static mut RECEIVER: Option<PeriodicReceiver<Nec, RecvPin>> = None;

#[entry]
fn main() -> ! {
    rtt_init_print!();

    let _core = cortex_m::Peripherals::take().unwrap();
    let device = pac::Peripherals::take().unwrap();

    let mut flash = device.FLASH.constrain();
    let mut rcc = device.RCC.constrain();

    let clocks = rcc
        .cfgr
        .use_hse(8.mhz())
        .sysclk(48.mhz())
        .pclk1(24.mhz())
        .freeze(&mut flash.acr);

    let mut gpiob = device.GPIOB.split(&mut rcc.apb2);
    let pin = gpiob.pb8.into_floating_input(&mut gpiob.crh);

    let mut timer =
        Timer::tim2(device.TIM2, &clocks, &mut rcc.apb1).start_count_down(SAMPLERATE.hz());

    timer.listen(Event::Update);

    let receiver = PeriodicReceiver::new(pin, SAMPLERATE);

    // Safe because the devices are only used from in the interrupt handler
    unsafe {
        TIMER.replace(timer);
        RECEIVER.replace(receiver);
    }

    unsafe {
        // Enable the timer interrupt
        pac::NVIC::unmask(pac::Interrupt::TIM2);
    }

    rprintln!("Init done!");

    loop {
        continue;
    }
}

#[interrupt]
fn TIM2() {
    let receiver = unsafe { RECEIVER.as_mut().unwrap() };

    if let Ok(Some(button)) = receiver.poll_button::<SpecialForMp3>() {
        match button {
            Button::Play_Paus => rprintln!("Play was pressed!"),
            Button::Power => rprintln!("Power on/off"),
            _ => rprintln!("Button: {:?}", button),
        };
    }

    // Clear the interrupt
    let timer = unsafe { TIMER.as_mut().unwrap() };
    timer.clear_update_interrupt_flag();
}
