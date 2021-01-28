#![no_main]
#![no_std]

use cortex_m::{
    asm,
    peripheral::DWT,
};

use embedded_hal::digital::v2::OutputPin;
use panic_rtt_target as _;

use rtic::app;
use rtt_target::{rprintln, rtt_init_print};

use stm32f1xx_hal::{
    gpio::{gpiob::PB8, Floating, Input},
    pac::TIM2,
    prelude::*,
    timer::{CountDownTimer, Event, Timer},
    usb::{Peripheral, UsbBus, UsbBusType},
};

use usb_device::{bus, prelude::*};

use usbd_hid::{
    descriptor::{
        MouseReport,
        generator_prelude::*
    },
    hid_class::HIDClass,
};

use infrared::{
    protocols::nec::NecApple,
    remotecontrol::{
        Button,
        RemoteControl,
    },
    remotes::nec::Apple2009,
    PeriodicReceiver,
};

/// The pin connected to the infrared receiver module
type RecvPin = PB8<Input<Floating>>;

const SAMPLERATE: u32 = 20_000;

#[app(device = stm32f1xx_hal::stm32, peripherals = true, monotonic = rtic::cyccnt::CYCCNT)]
const APP: () = {
    struct Resources {
        usb_dev: UsbDevice<'static, UsbBusType>,
        usb_hid: HIDClass<'static, UsbBusType>,
        timer: CountDownTimer<TIM2>,
        receiver: PeriodicReceiver<NecApple, RecvPin>,
    }

    #[init]
    fn init(mut cx: init::Context) -> init::LateResources {
        static mut USB_BUS: Option<bus::UsbBusAllocator<UsbBusType>> = None;

        rtt_init_print!();

        cx.core.DCB.enable_trace();
        // required on Cortex-M7 devices that software lock the DWT (e.g. STM32F7)
        DWT::unlock();
        cx.core.DWT.enable_cycle_counter();

        let mut flash = cx.device.FLASH.constrain();
        let mut rcc = cx.device.RCC.constrain();

        let clocks = rcc
            .cfgr
            .use_hse(8.mhz())
            .sysclk(48.mhz())
            .pclk1(24.mhz())
            .freeze(&mut flash.acr);

        assert!(clocks.usbclk_valid());

        let mut gpioa = cx.device.GPIOA.split(&mut rcc.apb2);

        // BluePill board has a pull-up resistor on the D+ line.
        // Pull the D+ pin down to send a RESET condition to the USB bus.
        // This forced reset is needed only for development, without it host
        // will not reset your device when you upload new firmware.
        let mut usb_dp = gpioa.pa12.into_push_pull_output(&mut gpioa.crh);
        usb_dp.set_low().unwrap();
        asm::delay(clocks.sysclk().0 / 100);

        let usb_dm = gpioa.pa11;
        let usb_dp = usb_dp.into_floating_input(&mut gpioa.crh);

        let usb = Peripheral {
            usb: cx.device.USB,
            pin_dm: usb_dm,
            pin_dp: usb_dp,
        };

        *USB_BUS = Some(UsbBus::new(usb));

        let usb_hid = HIDClass::new(USB_BUS.as_ref().unwrap(), MouseReport::desc(), 60);

        rprintln!("Defining USB parameters...");
        let usb_dev = UsbDeviceBuilder::new(USB_BUS.as_ref().unwrap(), UsbVidPid(0, 0x3821))
            .manufacturer("Infrared")
            .product("Mouse")
            .serial_number("InfraredR09")
            .device_class(0x00)
            .device_sub_class(0x00)
            .device_protocol(0x00)
            .build();

        let mut timer =
            Timer::tim2(cx.device.TIM2, &clocks, &mut rcc.apb1).start_count_down(SAMPLERATE.hz());
        timer.listen(Event::Update);

        let mut gpiob = cx.device.GPIOB.split(&mut rcc.apb2);
        let pin = gpiob.pb8.into_floating_input(&mut gpiob.crh);

        let receiver = PeriodicReceiver::new(pin, SAMPLERATE);

        init::LateResources {
            usb_dev,
            usb_hid,
            timer,
            receiver,
        }
    }

    #[idle]
    fn idle(_ctx: idle::Context) -> ! {
        rprintln!("Setup done: in idle");
        loop {
            continue;
        }
    }

    #[task(binds = USB_LP_CAN_RX0, priority = 3, resources = [usb_dev, usb_hid])]
    fn usb_rx0(mut cx: usb_rx0::Context) {
        usb_poll(&mut cx.resources.usb_dev, &mut cx.resources.usb_hid);
    }

    #[task(binds = TIM2, resources = [timer, receiver, ], spawn = [keydown])]
    fn tim2_irq(cx: tim2_irq::Context) {
        static mut REPEATS: u32 = 0;
        let tim2_irq::Resources { timer, receiver } = cx.resources;

        timer.clear_update_interrupt_flag();

        if let Ok(Some(cmd)) = receiver.poll() {
            let is_repeated = cmd.repeat;

            if let Some(button) = Apple2009::decode(cmd) {
                if is_repeated {
                    *REPEATS += 1;
                } else {
                    *REPEATS = 0;
                }

                rprintln!("Received: {:?}, repeat: {}", button, *REPEATS);
                let report = button_to_mousereport(button, *REPEATS);
                cx.spawn.keydown(report).ok();
            }
        }
    }

    #[task(resources = [usb_hid])]
    fn keydown(mut cx: keydown::Context, mr: MouseReport) {
        cx.resources.usb_hid.lock(|kbd| send_mousereport(kbd, mr));
    }

    extern "C" {
        fn USART1();
    }
};

fn usb_poll<B: bus::UsbBus>(
    usb_dev: &mut UsbDevice<'static, B>,
    usb_hid: &mut HIDClass<'static, B>,
) {
    while usb_dev.poll(&mut [usb_hid]) {}
}

fn send_mousereport(kbd: &HIDClass<UsbBusType>, report: MouseReport) {
    loop {
        let r = kbd.push_input(&report);
        match r {
            Ok(_) => break,
            Err(UsbError::WouldBlock) => {
                continue;
            }
            Err(_) => break,
        }
    }
}

fn button_to_mousereport(button: Button, repeats: u32) -> MouseReport {

    // A very rough acceleration
    let steps = match repeats {
        0 => 2,
        r @ 1 ..= 5 => 2 << (r as i8),
        _ => 64,
    };

    let mut buttons = 0;
    let mut x = 0;
    let mut y = 0;

    match button {
        Button::Play_Pause => {
            // Hold the button long enough to get a repeat that we use to signal mouse button release
            buttons = u8::from(repeats == 0);
        },
        Button::Up => y = -steps,
        Button::Down => y = steps,
        Button::Right => x = steps,
        Button::Left => x = -steps,
        _ => (),
    };

    MouseReport { buttons, x, y }
}
