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

use infrared::receiver::{MultiReceiver5, PinInput, Poll};
use infrared::protocol::*;

type IrPin = PB8<Input<Floating>>;
type IrReceiver = MultiReceiver5<Rc6, Nec, NecSamsung, Rc5, NecApple, Poll, PinInput<IrPin>>;

const SAMPLERATE: u32 = 20_000;
static mut TIMER: Option<CountDownTimer<TIM2>> = None;
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
    let inpin = gpiob.pb8.into_floating_input(&mut gpiob.crh);

    let mut timer = Timer::tim2(d.TIM2, &clocks, &mut rcc.apb1).start_count_down(SAMPLERATE.hz());

    timer.listen(Event::Update);

    let receiver = MultiReceiver5::new(SAMPLERATE as usize, PinInput(inpin));

    // Safe because the devices are only used in the interrupt handler
    unsafe {
        TIMER.replace(timer);
        RECEIVER.replace(receiver);
    }

    // Enable the timer interrupt
    unsafe {
        cortex_m::peripheral::NVIC::unmask(pac::Interrupt::TIM2);
    }

    rprintln!("Ready!");

    loop {
        continue;
    }
}

#[interrupt]
fn TIM2() {
    let receiver = unsafe { RECEIVER.as_mut().unwrap() };

    if let Ok((neccmd, nescmd, rc5cmd, rc6cmd, applecmd)) = receiver.poll() {
        // Print any command we found
        print_cmd(neccmd);
        print_cmd(nescmd);
        print_cmd(rc5cmd);
        print_cmd(rc6cmd);
        print_cmd(applecmd);
    }

    // Clear the interrupt
    let timer = unsafe { TIMER.as_mut().unwrap() };
    timer.clear_update_interrupt_flag();
}

fn print_cmd<C: core::fmt::Debug>(cmd: Option<C>) {
    if let Some(cmd) = cmd {
        rprintln!("{:?}", cmd);
    }
}
