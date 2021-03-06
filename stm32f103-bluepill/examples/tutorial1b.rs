#![no_std]
#![no_main]

use cortex_m_rt::entry;
use stm32f1xx_hal::{
    gpio::{gpiob::PB8, Floating, Input},
    pac::{self, interrupt, TIM2},
    prelude::*,
    timer::{CountDownTimer, Event, Timer},
};

use panic_rtt_target as _;
use rtt_target::{rprintln, rtt_init_print};

use infrared::{
    Receiver,
    protocol::{Rc6, Rc6Command},
    remotecontrol::{Button, DeviceType, RemoteControl},
    ProtocolId,
};
use infrared::receiver::{Poll, PinInput};

// Sample rate
const TIMER_FREQ: u32 = 20_000;

// Our receivertype
type IrReceiver = Receiver<Rc6, Poll, PinInput<PB8<Input<Floating>>>>;

// Globals
static mut TIMER: Option<CountDownTimer<TIM2>> = None;
static mut RECEIVER: Option<IrReceiver> = None;

struct Rc6Tv;
impl RemoteControl for Rc6Tv {
    const MODEL: &'static str = "Rc6 Tv";
    const DEVTYPE: DeviceType = DeviceType::TV;
    const PROTOCOL: ProtocolId = ProtocolId::Rc6;
    const ADDRESS: u32 = 0;
    type Cmd = Rc6Command;
    const BUTTONS: &'static [(u32, Button)] = &[
        // Cmdid to Button mappings
        (1, Button::One),
        (2, Button::Two),
        (3, Button::Three),
        (4, Button::Four),
        (5, Button::Five),
        (6, Button::Six),
        (7, Button::Seven),
        (8, Button::Eight),
        (9, Button::Nine),
        (12, Button::Power),
        (76, Button::VolumeUp),
        (77, Button::VolumeDown),
        (60, Button::Teletext),
    ];
}

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
        Timer::tim2(device.TIM2, &clocks, &mut rcc.apb1).start_count_down(TIMER_FREQ.hz());

    timer.listen(Event::Update);
    let receiver = Receiver::with_pin(TIMER_FREQ as usize, pin);

    // Safe because the devices are only used in the interrupt handler
    unsafe {
        TIMER.replace(timer);
        RECEIVER.replace(receiver);
    }

    unsafe {
        // Enable the timer interrupt
        pac::NVIC::unmask(pac::Interrupt::TIM2);
    }

    rprintln!("Ready!");

    loop {
        continue;
    }
}

#[interrupt]
fn TIM2() {
    let receiver = unsafe { RECEIVER.as_mut().unwrap() };

    if let Ok(Some(button)) = receiver.poll_remotecontrol::<Rc6Tv>() {
        use Button::*;

        match button {
            Teletext => rprintln!("Teletext!"),
            Power => rprintln!("Power on/off"),
            _ => rprintln!("Button: {:?}", button),
        };
    }

    // Clear the interrupt
    let timer = unsafe { TIMER.as_mut().unwrap() };
    timer.clear_update_interrupt_flag();
}
