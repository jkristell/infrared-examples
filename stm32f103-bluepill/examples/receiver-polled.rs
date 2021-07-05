#![no_std]
#![no_main]

use cortex_m_rt::entry;
use panic_rtt_target as _;
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
    protocol::{
        NecApple,
        Nec,
    },
    remotecontrol::{AsButton, Button, RemoteControl},
    remotecontrol::{nec::*, rc5::*},
    Receiver,
};
use infrared::receiver::{PinInput, Poll};

// Pin connected to the receiver
type IrPin = PB8<Input<Floating>>;
type IrReceiver = Receiver<NecApple, Poll, PinInput<IrPin>>;

// Samplerate
const SAMPLERATE: usize = 100_000;
// Our timer. Needs to be accessible in the interrupt handler.
static mut TIMER: Option<CountDownTimer<TIM2>> = None;
// Our Infrared receiver
static mut RECEIVER: Option<IrReceiver> = None;

#[entry]
fn main() -> ! {
    rtt_init_print!();

    let _cp = cortex_m::Peripherals::take().unwrap();
    let d = pac::Peripherals::take().unwrap();

    let mut flash = d.FLASH.constrain();
    let mut rcc = d.RCC.constrain();

    let clocks = rcc
        .cfgr
        .use_hse(8.mhz())
        .sysclk(48.mhz())
        .pclk1(24.mhz())
        .freeze(&mut flash.acr);

    let mut gpiob = d.GPIOB.split(&mut rcc.apb2);
    let pin = gpiob.pb8.into_floating_input(&mut gpiob.crh);

    let mut timer = Timer::tim2(d.TIM2, &clocks, &mut rcc.apb1).start_count_down((SAMPLERATE as u32).hz());

    timer.listen(Event::Update);

    let receiver = Receiver::builder()
        .nec_apple()
        .polled()
        .resolution(SAMPLERATE)
        .pin(pin)
        .build();

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

    let r = receiver.poll();

    match r {
        Ok(Some(cmd)) => {
            if let Some(button) = Apple2009::decode(&cmd) {
                match button {
                    Button::Play_Pause => rprintln!("Play was pressed!"),
                    Button::Power => rprintln!("Power on/off"),
                    _ => rprintln!("{:?}", button),
                };
            }
        }
        Ok(None) => {},
        Err(err) => rprintln!("Err: {:?}", err),
    }

    // Clear the interrupt
    let timer = unsafe { TIMER.as_mut().unwrap() };
    timer.clear_update_interrupt_flag();
}
