#![no_std]
#![no_main]

use cortex_m_rt::entry;
use panic_rtt_target as _;
use rtt_target::{rprintln, rtt_init_print};
use stm32f1xx_hal::{
    gpio::{gpiob::PB8, Floating, Input},
    pac,
    prelude::*,
    stm32::interrupt,
    time::MonoTimer,
};

#[allow(unused_imports)]
use infrared::{
    Receiver,
    receiver::{Event, PinInput},
    protocol::{Nec, Rc6},
    remotecontrol::{nec::*, rc5::*},
};
use stm32f1xx_hal::gpio::{Edge, ExtiPin};
use stm32f1xx_hal::time::Instant;

// Pin connected to the receiver
type RecvPin = PB8<Input<Floating>>;
//type IrReceiver = ConstReceiver<NecApple, Evented, PinInput<RecvPin>>

// Our timer. Needs to be accessible in the interrupt handler.
static mut MONO: Option<MonoTimer> = None;

// Our Infrared receiver
static mut RECEIVER: Option<Receiver<Rc6, Event, PinInput<RecvPin>>> = None;

#[entry]
fn main() -> ! {
    rtt_init_print!();

    let cp = cortex_m::Peripherals::take().unwrap();
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

    let mono = MonoTimer::new(cp.DWT, cp.DCB, clocks);
    let mono_freq = mono.frequency();

    let receiver = Receiver::with_pin( mono_freq.0 as usize, pin);

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

    if let Some(dt) = LAST.map(|i| i.elapsed()) {

        if let Ok(Some(cmd)) = receiver.event(dt as usize) {
            rprintln!("cmd: {}", cmd.cmd);
        }
    }

    LAST.replace(mono.now());

    receiver.pin().clear_interrupt_pending_bit();
}
