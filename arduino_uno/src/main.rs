//! A basic implementation of the `millis()` function from Arduino:
//!
//!     https://www.arduino.cc/reference/en/language/functions/time/millis/
//!
//! Uses timer 0 and one of its interrupts to update a global millisecond
//! counter.  A walkthough of this code is available here:
//!
//!     https://blog.rahix.de/005-avr-hal-millis/
#![no_std]
#![no_main]
#![feature(abi_avr_interrupt)]

use arduino_uno::prelude::*;
use panic_halt as _;
use infrared::protocols::Nec;
use arduino_uno::hal::port::mode::{Floating, Input};
use arduino_uno::hal::port::portd::PD7;


const TOP: u32 = 100; // (16_000_000 * 50 / 1_000_000) / 8;

type ReceiverPin = PD7<Input<Floating>>;
static mut RECEIVER: Option<infrared::PeriodicReceiver<Nec, ReceiverPin>> = None;
static mut CMD: Option<infrared::protocols::nec::NecCommand> = None;

fn timer_init(tc0: arduino_uno::pac::TC0) {
    // Configure the timer for the above interval (in CTC mode)
    // and enable its interrupt.
    tc0.tccr0a.write(|w| w.wgm0().ctc());
    tc0.ocr0a.write(|w| unsafe { w.bits(TOP as u8) });
    tc0.tccr0b.write(|w| w.cs0().prescale_8());

    tc0.timsk0.write(|w| w.ocie0a().set_bit());
}

#[avr_device::interrupt(atmega328p)]
fn TIMER0_COMPA() {

    let recv = unsafe {
        RECEIVER.as_mut().unwrap()
    };

    if let Ok(Some(cmd)) = recv.poll() {
        unsafe {
            CMD.replace(cmd);
        }
    }

}

// ----------------------------------------------------------------------------

#[arduino_uno::entry]
fn main() -> ! {
    let dp = arduino_uno::Peripherals::take().unwrap();

    let mut pins = arduino_uno::Pins::new(dp.PORTB, dp.PORTC, dp.PORTD);

    let mut serial = arduino_uno::Serial::new(
        dp.USART0,
        pins.d0,
        pins.d1.into_output(&mut pins.ddr),
        57600.into_baudrate(),
    );

    timer_init(dp.TC0);

    let receiver = infrared::PeriodicReceiver::new(pins.d7, 20_000);

    unsafe {
        RECEIVER.replace(receiver);
    }

    // Enable interrupts globally
    unsafe { avr_device::interrupt::enable() };

    // Wait for a character and print current time once it is received
    loop {
        let b = nb::block!(serial.read()).void_unwrap();

        if let Some(cmd) = unsafe {CMD} {
            ufmt::uwriteln!(
                &mut serial, "{} {} {}\r",
                 cmd.addr, cmd.cmd, cmd.repeat
            )
            .void_unwrap();
        }

        ufmt::uwriteln!(&mut serial, "Got {} \r", b).void_unwrap();
    }
}
