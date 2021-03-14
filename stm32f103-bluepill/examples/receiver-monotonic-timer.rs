#![no_std]
#![no_main]

use cortex_m_rt::entry;
use panic_rtt_target as _;
use rtt_target::{rprintln, rtt_init_print};
use stm32f1xx_hal::{
    gpio::{gpiob::PB8, Floating, Input},
    pac,
    prelude::*,
    stm32::{interrupt},
    time::MonoTimer,
};

#[allow(unused_imports)]
use infrared::{
    hal::{EventReceiver, PeriodicReceiver},
    protocols::{Nec, Rc5},
    remotecontrol::Button,
    remotes::{nec::*, rc5::*},
};
use stm32f1xx_hal::gpio::{Edge, ExtiPin};
use stm32f1xx_hal::time::Instant;

// Pin connected to the receiver
type RecvPin = PB8<Input<Floating>>;

// Our timer. Needs to be accessible in the interrupt handler.
static mut MONO: Option<MonoTimer> = None;
// The scale factor for the mono timer
const MONO_SHIFT: u32 = 11;

// Our Infrared receiver
static mut RECEIVER: Option<EventReceiver<Nec, RecvPin>> = None;

#[entry]
fn main() -> ! {
    rtt_init_print!();

    let mut cp = cortex_m::Peripherals::take().unwrap();
    let d = pac::Peripherals::take().unwrap();

    let mut flash = d.FLASH.constrain();
    let mut rcc = d.RCC.constrain();
    let mut afio = d.AFIO.constrain(&mut rcc.apb2);

    let clocks = rcc
        .cfgr
        .use_hse(8.mhz())
        .sysclk(48.mhz())
        .pclk1(24.mhz())
        .freeze(&mut flash.acr);

    let mut gpiob = d.GPIOB.split(&mut rcc.apb2);
    let mut pin = gpiob.pb8.into_floating_input(&mut gpiob.crh);

    pin.make_interrupt_source(&mut afio);
    pin.trigger_on_edge(&d.EXTI, Edge::RISING_FALLING);
    pin.enable_interrupt(&d.EXTI);

    //Note: Remove after upgrading stm32f1xx-hal to version 0.7
    cp.DCB.enable_trace();
    let mono = MonoTimer::new(cp.DWT, clocks);

    let mono_freq = mono.frequency();
    let mono_freq_scaled = mono_freq.0 >> MONO_SHIFT;

    let receiver = EventReceiver::new(pin, mono_freq_scaled);

    // Safe because the devices are only used from in the interrupt handler
    unsafe {
        RECEIVER.replace(receiver);
        MONO.replace(mono);
    }

    unsafe {
        pac::NVIC::unmask(pac::Interrupt::EXTI9_5);
    }

    rprintln!("Infrared Receiver Ready!");

    loop {
        continue;
    }
}

#[interrupt]
fn EXTI9_5() {

    static mut LAST: Option<Instant> = None;

    let receiver = unsafe { RECEIVER.as_mut().unwrap() };
    let mono = unsafe { MONO.as_ref().unwrap() };

    if let Some(dt) = LAST.map(|i| i.elapsed() >> MONO_SHIFT) {
        // rprintln!("dt: {:?}", dt);

        if let Ok(Some(cmd)) = receiver.edge_event(dt) {
            rprintln!("cmd: {}", cmd.cmd);
        }
    }

    LAST.replace(mono.now());

    receiver.pin.clear_interrupt_pending_bit();
}
