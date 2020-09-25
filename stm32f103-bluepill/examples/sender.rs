#![no_std]
#![no_main]

use cortex_m;
use cortex_m_rt::entry;
use rtt_target::{rprintln, rtt_init_print};
use stm32f1xx_hal::{
    pac::{self, interrupt, TIM2, TIM4},
    prelude::*,
    pwm::{PwmChannel, C4},
    timer::{CountDownTimer, Event, Tim4NoRemap, Timer},
};

use infrared::{
    remotes::rc5::Rc5CdPlayer,
    Button, RemoteControl,
    hal::HalSender,
};
use panic_rtt_target as _;

type PwmPin = PwmChannel<TIM4, C4>;
const TIMER_FREQ: u32 = 20_000;

// Global timer
static mut TIMER: Option<CountDownTimer<TIM2>> = None;
// Transmitter
static mut TRANSMITTER: Option<HalSender<PwmPin, u16>> = None;

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

    let mut timer =
        Timer::tim2(d.TIM2, &clocks, &mut rcc.apb1).start_count_down(TIMER_FREQ.hz());

    timer.listen(Event::Update);

    // PWM
    let mut afio = d.AFIO.constrain(&mut rcc.apb2);
    let mut gpiob = d.GPIOB.split(&mut rcc.apb2);
    let irled = gpiob.pb9.into_alternate_push_pull(&mut gpiob.crh);

    let pwm = Timer::tim4(d.TIM4, &clocks, &mut rcc.apb1).pwm::<Tim4NoRemap, _, _, _>(
        irled,
        &mut afio.mapr,
        38.khz(),
    );

    let mut irpin = pwm.split();

    irpin.set_duty(irpin.get_max_duty() / 2);
    irpin.disable();

    // Safe because the devices are only used in the interrupt handler
    unsafe {
        TIMER.replace(timer);
        TRANSMITTER.replace(HalSender::new(TIMER_FREQ, irpin));
    }

    unsafe { cortex_m::peripheral::NVIC::unmask(pac::Interrupt::TIM2); }

    rprintln!("Init done");
    loop {
        continue;
    }
}

#[interrupt]
fn TIM2() {
    // Clear the interrupt
    let timer = unsafe { TIMER.as_mut().unwrap() };
    timer.clear_update_interrupt_flag();

    let transmitter = unsafe { TRANSMITTER.as_mut().unwrap() };

    if transmitter.counter % (TIMER_FREQ * 2) == 0 {
        let cmd = Rc5CdPlayer::encode(Button::Next).unwrap();
        let r = transmitter.load(&cmd);
        rprintln!("Command loaded? {:?}", r);

        transmitter.tick();
    }
}
