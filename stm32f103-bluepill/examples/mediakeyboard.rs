#![no_main]
#![no_std]

use panic_rtt_target as _;
use stm32f1xx_hal::usb::UsbBusType;
use usbd_hid::{
    descriptor::{MediaKey, MediaKeyboardReport},
    hid_class::HIDClass,
};

use infrared::remotecontrol::Button;
use usb_device::{bus, prelude::*};

#[rtic::app(device = stm32f1xx_hal::stm32, peripherals = true, dispatchers = [USART1])]
mod app {
    use cortex_m::asm;
    use embedded_hal::digital::v2::OutputPin;
    use panic_rtt_target as _;
    use rtt_target::{rprintln, rtt_init_print};
    use stm32f1xx_hal::{
        gpio::{gpiob::PB8, ExtiPin, Floating, Input},
        prelude::*,
        usb::{Peripheral, UsbBus, UsbBusType},
    };
    use usbd_hid::{
        descriptor::{generator_prelude::*, MediaKey, MediaKeyboardReport},
        hid_class::HIDClass,
    };

    use usb_device::{bus, prelude::*};

    use infrared::{protocol::NecApple, remotecontrol::nec::Apple2009, Receiver};

    use dwt_systick_monotonic::DwtSystick;
    use rtic::time::{duration::Milliseconds, Instant};
    use stm32f1xx_hal::gpio::Edge;
    use infrared::receiver::{PinInput, Event};

    /// The pin connected to the infrared receiver module
    type RxPin = PB8<Input<Floating>>;

    #[monotonic(binds = SysTick, default = true)]
    type InfraMono = DwtSystick<48_000_000>;

    #[resources]
    struct Resources {
        usb_dev: UsbDevice<'static, UsbBusType>,
        usb_kbd: HIDClass<'static, UsbBusType>,
        receiver: Receiver<NecApple, Event, PinInput<crate::app::RxPin>>,
    }

    #[init]
    fn init(mut cx: init::Context) -> (init::LateResources, init::Monotonics) {
        static mut USB_BUS: Option<bus::UsbBusAllocator<UsbBusType>> = None;

        rtt_init_print!();

        let mut flash = cx.device.FLASH.constrain();
        let mut rcc = cx.device.RCC.constrain();
        let mut afio = cx.device.AFIO.constrain(&mut rcc.apb2);

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

        let usb_kbd = HIDClass::new(USB_BUS.as_ref().unwrap(), MediaKeyboardReport::desc(), 64);

        let usb_dev = UsbDeviceBuilder::new(USB_BUS.as_ref().unwrap(), UsbVidPid(0x16c0, 0x27dd))
            .manufacturer("Infrared")
            .product("Mediakeyboard")
            .serial_number("TEST")
            .device_class(0x03) // HID
            .build();

        let mut gpiob = cx.device.GPIOB.split(&mut rcc.apb2);
        let mut pin = gpiob.pb8.into_floating_input(&mut gpiob.crh);
        pin.make_interrupt_source(&mut afio);
        pin.trigger_on_edge(&cx.device.EXTI, Edge::RISING_FALLING);
        pin.enable_interrupt(&cx.device.EXTI);

        let mono_clock = clocks.hclk().0;
        rprintln!("Mono clock: {}", mono_clock);

        let resolution = 48_000_000;
        let receiver = Receiver::with_pin(resolution , pin);

        let monot = DwtSystick::new(&mut cx.core.DCB, cx.core.DWT, cx.core.SYST, mono_clock);

        let late = init::LateResources {
            receiver,
            usb_dev,
            usb_kbd,
        };

        (late, init::Monotonics(monot))
    }

    #[idle]
    fn idle(_ctx: idle::Context) -> ! {
        rprintln!("Setup done. In idle");
        loop {
            continue;
        }
    }

    #[task(binds = USB_LP_CAN_RX0, priority = 3, resources = [usb_dev, usb_kbd])]
    fn usb_rx0(cx: usb_rx0::Context) {
        let usb_dev = cx.resources.usb_dev;
        let usb_kbd = cx.resources.usb_kbd;

        (usb_dev, usb_kbd).lock(|usb_dev, usb_kbd| {
            super::usb_poll(usb_dev, usb_kbd);
        });
    }

    #[task(binds = EXTI9_5, resources = [receiver])]
    fn ir_rx(mut cx: ir_rx::Context) {
        static mut LAST: Option<Instant<InfraMono>> = None;

        let now = monotonics::InfraMono::now();

        cx.resources.receiver.lock(|r| {
            r.pin().clear_interrupt_pending_bit();

            if let Some(last) = LAST {
                let dt = *now.checked_duration_since(&last).unwrap().integer();

                if let Ok(Some(button)) = r.event_remotecontrol::<Apple2009>(dt as usize) {
                    rprintln!("{:?}", button);
                    let key = super::button_to_mediakey(button);
                    keydown::spawn(key).unwrap();
                }
            };
        });

        *LAST = Some(now);
    }

    #[task(resources = [usb_kbd])]
    fn keydown(mut cx: keydown::Context, key: MediaKey) {
        cx.resources
            .usb_kbd
            .lock(|kbd| super::send_keycode(kbd, key));

        keyup::spawn_after(Milliseconds(20_u32)).unwrap();
    }

    #[task(resources = [usb_kbd])]
    fn keyup(mut cx: keyup::Context) {
        cx.resources
            .usb_kbd
            .lock(|kbd| super::send_keycode(kbd, MediaKey::Zero));
    }
}

fn usb_poll<B: bus::UsbBus>(
    usb_dev: &mut UsbDevice<'static, B>,
    usb_kbd: &mut HIDClass<'static, B>,
) {
    while usb_dev.poll(&mut [usb_kbd]) {}
}

fn send_keycode(kbd: &HIDClass<UsbBusType>, key: MediaKey) {
    let report = MediaKeyboardReport {
        usage_id: key.into(),
    };

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

fn button_to_mediakey(b: Button) -> MediaKey {
    match b {
        Button::Play_Pause => MediaKey::PlayPause,
        Button::Up => MediaKey::VolumeIncrement,
        Button::Down => MediaKey::VolumeDecrement,
        Button::Right => MediaKey::NextTrack,
        Button::Left => MediaKey::PrevTrack,
        Button::Stop => MediaKey::Stop,
        _ => MediaKey::Zero,
    }
}
