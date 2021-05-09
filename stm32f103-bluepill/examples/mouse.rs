#![no_main]
#![no_std]

use panic_rtt_target as _;

use infrared::remotecontrol::Button;
use stm32f1xx_hal::usb::UsbBusType;
use usb_device::prelude::*;
use usbd_hid::{descriptor::MouseReport, hid_class::HIDClass};

#[rtic::app(device = stm32f1xx_hal::stm32, peripherals = true, dispatchers = [USART1])]
mod app {
    use dwt_systick_monotonic::DwtSystick;
    use embedded_hal::digital::v2::OutputPin;
    use infrared::{
        protocol::NecApple, remotecontrol::RemoteControl, remotecontrol::nec::Apple2009,
        Receiver,
    };

    use rtt_target::{rprintln, rtt_init_print};
    use stm32f1xx_hal::{
        gpio::{gpiob::PB8, Edge, ExtiPin, Floating, Input},
        prelude::*,
        usb::{Peripheral, UsbBus, UsbBusType},
    };
    use usb_device::{bus, prelude::*};
    use usbd_hid::{
        descriptor::{generator_prelude::*, MouseReport},
        hid_class::HIDClass,
    };
    use core::convert::TryFrom;
    use rtic::time::duration::Microseconds;
    use rtic::time::{Instant};
    use infrared::receiver::{PinInput, Event};
    use infrared::remotecontrol::Button;

    const MONOTIMER_FREQ: u32 = 48_000_000;

    /// The pin connected to the infrared receiver module
    type RxPin = PB8<Input<Floating>>;

    #[monotonic(binds = SysTick, default = true)]
    type InfraMono = DwtSystick<{crate::app::MONOTIMER_FREQ}>;

    #[resources]
    struct Resources {
        usb_dev: UsbDevice<'static, UsbBusType>,
        usb_hid: HIDClass<'static, UsbBusType>,
        #[lock_free]
        ir_rx: Receiver<NecApple, Event, PinInput<crate::app::RxPin>>,
        #[lock_free]
        last_event: Instant<crate::app::InfraMono>,
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
        let mut usb_dp = gpioa.pa12.into_push_pull_output(&mut gpioa.crh);
        usb_dp.set_low().unwrap();
        cortex_m::asm::delay(clocks.sysclk().0 / 100);
        let usb_dm = gpioa.pa11;
        let usb_dp = usb_dp.into_floating_input(&mut gpioa.crh);
        let usb = Peripheral {
            usb: cx.device.USB,
            pin_dm: usb_dm,
            pin_dp: usb_dp,
        };

        USB_BUS.replace(UsbBus::new(usb));
        let usb_hid = HIDClass::new(USB_BUS.as_ref().unwrap(), MouseReport::desc(), 60);

        let usb_dev = UsbDeviceBuilder::new(USB_BUS.as_ref().unwrap(), UsbVidPid(0, 0x3821))
            .manufacturer("Infrared")
            .product("Mouse")
            .serial_number("InfraredR09")
            .device_class(0x00)
            .build();

        let rx_pin = {
            let mut gpiob = cx.device.GPIOB.split(&mut rcc.apb2);
            let mut pin = gpiob.pb8.into_floating_input(&mut gpiob.crh);
            pin.make_interrupt_source(&mut afio);
            pin.trigger_on_edge(&cx.device.EXTI, Edge::RISING_FALLING);
            pin.enable_interrupt(&cx.device.EXTI);
            pin
        };

        // Run the receiver with native resolution and let embedded time to the conversion
        let ir_rx = Receiver::builder()
            .nec_apple()
            .resolution(1_000_000)
            .pin(rx_pin)
            .build();

        let mono_clock = clocks.hclk().0;
        let mono = DwtSystick::new(&mut cx.core.DCB, cx.core.DWT, cx.core.SYST, mono_clock);
        let zero: Instant<crate::app::InfraMono> = Instant::new(0);

        let late = init::LateResources {
            ir_rx,
            usb_dev,
            usb_hid,
            last_event: zero
        };

        (late, init::Monotonics(mono))
    }

    #[idle]
    fn idle(_ctx: idle::Context) -> ! {
        rprintln!("Setup done: in idle");
        loop {
            continue;
        }
    }

    #[task(binds = EXTI9_5, priority = 2, resources = [ir_rx, last_event])]
    fn ir_rx(cx: ir_rx::Context) {

        let last_event = cx.resources.last_event;
        let ir_rx = cx.resources.ir_rx;

        let now = monotonics::InfraMono::now();
        let dt = now
            .checked_duration_since(&last_event)
            .and_then(|v| Microseconds::<u32>::try_from(v).ok())
            .map(|ms| ms.0 as usize)
            .unwrap_or_default();

        if let Ok(Some(cmd)) = ir_rx.event(dt) {
            if let Some(button) = Apple2009::decode(&cmd) {
                let _ = process_ir_cmd::spawn(button, cmd.repeat).ok();
            }
        }

        ir_rx.pin().clear_interrupt_pending_bit();
        *last_event = now;
    }

    #[task]
    fn process_ir_cmd(_: process_ir_cmd::Context, button: Button, is_repeated: bool) {
        static mut REPEATS: u32 = 0;

        if !is_repeated {
            *REPEATS = 0;
        }
        *REPEATS += 1;

        rprintln!("Received: {:?}, repeat: {}", button, *REPEATS);
        let report = super::button_to_mousereport(button, *REPEATS);
        keydown::spawn(report).unwrap()
    }

    #[task(binds = USB_LP_CAN_RX0, priority = 3, resources = [usb_dev, usb_hid])]
    fn usb_rx0(cx: usb_rx0::Context) {
        let usb_dev = cx.resources.usb_dev;
        let usb_hid = cx.resources.usb_hid;

        (usb_dev, usb_hid).lock(|usb_dev, usb_hid| usb_dev.poll(&mut [usb_hid]));
    }

    #[task(resources = [usb_hid])]
    fn keydown(mut cx: keydown::Context, mr: MouseReport) {
        cx.resources
            .usb_hid
            .lock(|kbd| super::send_mousereport(kbd, mr));
    }
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
        r @ 0..=6 => 1 << (r as i8),
        _ => 64,
    };

    let mut buttons = 0;
    let mut x = 0;
    let mut y = 0;

    match button {
        Button::Play_Pause => {
            // Hold the button long enough to get a repeat that we use to signal mouse button release
            buttons = u8::from(repeats == 0);
        }
        Button::Up => y = -steps,
        Button::Down => y = steps,
        Button::Right => x = steps,
        Button::Left => x = -steps,
        _ => (),
    };

    MouseReport {
        buttons,
        x,
        y,
        wheel: 0,
    }
}
