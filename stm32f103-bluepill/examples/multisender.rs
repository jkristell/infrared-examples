#![no_std]
#![no_main]

use cortex_m;
use cortex_m_rt::entry;
use stm32f1xx_hal::{
    pac::{self, interrupt, TIM2, TIM4},
    prelude::*,
    pwm::{PwmChannel, C4},
    timer::{CountDownTimer, Event, Tim4NoRemap, Timer},
};

use infrared::{
    protocols::Rc5,
    remotecontrol::{Button, RemoteControl},
    remotes::rc5::CdPlayer,
    MultiSender,
};
use panic_halt as _;
use infrared::protocols::{
    Nec,
    nec::{NecCommand},
    rc5::sender::Rc5SenderState,
    nec::sender::NecSenderState,
};

type PwmPin = PwmChannel<TIM4, C4>;
const SAMPLERATE: u32 = 20_000;

// Global timer
static mut TIMER: Option<CountDownTimer<TIM2>> = None;
// Transmitter
static mut TRANSMITTER: Option<MultiSender<PwmPin>> = None;
// Sender data
static mut RC5_DATA: Option<Rc5SenderState> = None;
static mut NEC_DATA: Option<NecSenderState> = None;

#[entry]
fn main() -> ! {

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

    let mut timer = Timer::tim2(d.TIM2, &clocks, &mut rcc.apb1).start_count_down(SAMPLERATE.hz());

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

    // Create the sender
    let sender = MultiSender::new(SAMPLERATE, irpin) ;

    // Create the sender states
    let rc5 = sender.create_state();
    let nec = sender.create_state();

    // Safe because the devices are only used in the interrupt handler
    unsafe {
        TIMER.replace(timer);
        TRANSMITTER.replace(sender);
        RC5_DATA.replace(rc5);
        NEC_DATA.replace(nec);
    }

    unsafe {
        cortex_m::peripheral::NVIC::unmask(pac::Interrupt::TIM2);
    }

    loop {
        continue;
    }
}

#[interrupt]
fn TIM2() {
    // Clear the interrupt
    let timer = unsafe { TIMER.as_mut().unwrap() };
    timer.clear_update_interrupt_flag();

    let sender = unsafe { TRANSMITTER.as_mut().unwrap() };
    let rc5 = unsafe { RC5_DATA.as_mut().unwrap() };
    let nec = unsafe { NEC_DATA.as_mut().unwrap() };

    if 10 % (SAMPLERATE * 2) == 0 {
        let cmd = CdPlayer::encode(Button::Next).unwrap();
         sender.load::<Rc5>(rc5, &cmd);

        let cmd = NecCommand {
            addr: 10,
            cmd: 44,
            repeat: false
        };
        sender.load::<Nec>(nec, &cmd);

        sender.tick();
    }
}
