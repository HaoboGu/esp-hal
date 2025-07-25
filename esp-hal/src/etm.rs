#![cfg_attr(docsrs, procmacros::doc_replace)]
//! # Event Task Matrix (ETM)
//!
//! ## Overview
//!
//! Normally, if a peripheral X needs to notify peripheral Y of a particular
//! event, this could only be done via a CPU interrupt from peripheral X, where
//! the CPU notifies peripheral Y on behalf of peripheral X. However, in
//! time-critical applications, the latency introduced by CPU interrupts is
//! non-negligible.
//!
//! With the help of the Event Task Matrix (ETM) module, some peripherals can
//! directly notify other peripherals of events through pre-set connections
//! without the intervention of CPU interrupts. This allows precise and low
//! latency synchronization between peripherals, and lessens the CPU’s workload
//! as the CPU no longer needs to handle these events.
//!
//! The ETM module has multiple programmable channels, they are used to connect
//! a particular Event to a particular Task. When an event is activated, the ETM
//! channel will trigger the corresponding task automatically.
//!
//! For more information, please refer to the
#![doc = concat!("[ESP-IDF documentation](https://docs.espressif.com/projects/esp-idf/en/latest/", chip!(), "/api-reference/peripherals/etm.html)")]
//! ## Examples
//!
//! ### Control LED by the button via ETM
//! ```rust, no_run
//! # {before_snippet}
//! # use esp_hal::gpio::etm::{Channels, InputConfig, OutputConfig};
//! # use esp_hal::etm::Etm;
//! # use esp_hal::gpio::Pull;
//! # use esp_hal::gpio::Level;
//!
//! let led = peripherals.GPIO1;
//! let button = peripherals.GPIO9;
//!
//! // setup ETM
//! let gpio_ext = Channels::new(peripherals.GPIO_SD);
//! let led_task = gpio_ext.channel0_task.toggle(
//!     led,
//!     OutputConfig {
//!         open_drain: false,
//!         pull: Pull::None,
//!         initial_state: Level::Low,
//!     },
//! );
//! let button_event = gpio_ext
//!     .channel0_event
//!     .falling_edge(button, InputConfig { pull: Pull::Down });
//!
//! let etm = Etm::new(peripherals.ETM);
//! let channel0 = etm.channel0;
//!
//! // make sure the configured channel doesn't get dropped - dropping it will
//! // disable the channel
//! let _configured_channel = channel0.setup(&button_event, &led_task);
//!
//! // the LED is controlled by the button without involving the CPU
//! loop {}
//! # }
//! ```
//!
//! ### Control LED by the systimer via ETM
//! ```rust, no_run
//! # {before_snippet}
//! # use esp_hal::gpio::etm::{Channels, InputConfig, OutputConfig};
//! # use esp_hal::etm::Etm;
//! # use esp_hal::gpio::Pull;
//! # use esp_hal::gpio::Level;
//! # use esp_hal::timer::systimer::{etm::Event, SystemTimer};
//! # use esp_hal::timer::PeriodicTimer;
//! use esp_hal::time::Duration;
//!
//! let syst = SystemTimer::new(peripherals.SYSTIMER);
//! let mut alarm0 = syst.alarm0;
//! let mut timer = PeriodicTimer::new(alarm0.reborrow());
//! timer.start(Duration::from_secs(1));
//!
//! let led = peripherals.GPIO1;
//!
//! // setup ETM
//! let gpio_ext = Channels::new(peripherals.GPIO_SD);
//! let led_task = gpio_ext.channel0_task.toggle(
//!     led,
//!     OutputConfig {
//!         open_drain: false,
//!         pull: Pull::None,
//!         initial_state: Level::Low,
//!     },
//! );
//!
//! let timer_event = Event::new(&alarm0);
//!
//! let etm = Etm::new(peripherals.ETM);
//! let channel0 = etm.channel0;
//!
//! // make sure the configured channel doesn't get dropped - dropping it will
//! // disable the channel
//! let _configured_channel = channel0.setup(&timer_event, &led_task);
//!
//! // the LED is controlled by the timer without involving the CPU
//! loop {}
//! # }
//! ```

use crate::{peripherals::ETM, system::GenericPeripheralGuard};

/// Unconfigured EtmChannel.
#[non_exhaustive]
pub struct EtmChannel<const C: u8> {}

impl<const C: u8> EtmChannel<C> {
    /// Setup the channel
    ///
    /// Enabled the channel and configures the assigned event and task.
    pub fn setup<'a, E, T>(self, event: &'a E, task: &'a T) -> EtmConfiguredChannel<'a, E, T, C>
    where
        E: EtmEvent,
        T: EtmTask,
    {
        let etm = ETM::regs();
        let guard = GenericPeripheralGuard::new();

        etm.ch(C as usize)
            .evt_id()
            .modify(|_, w| unsafe { w.evt_id().bits(event.id()) });
        etm.ch(C as usize)
            .task_id()
            .modify(|_, w| unsafe { w.task_id().bits(task.id()) });
        if C < 32 {
            etm.ch_ena_ad0_set().write(|w| w.ch_set(C).set_bit());
        } else {
            etm.ch_ena_ad1_set().write(|w| w.ch_set(C - 32).set_bit());
        }

        EtmConfiguredChannel {
            _event: event,
            _task: task,
            _guard: guard,
        }
    }
}

fn disable_channel(channel: u8) {
    if channel < 32 {
        ETM::regs()
            .ch_ena_ad0_clr()
            .write(|w| w.ch_clr(channel).set_bit());
    } else {
        ETM::regs()
            .ch_ena_ad1_clr()
            .write(|w| w.ch_clr(channel - 32).set_bit());
    }
}

/// A readily configured channel
///
/// The channel is enabled and event and task are configured.
#[non_exhaustive]
pub struct EtmConfiguredChannel<'a, E, T, const C: u8>
where
    E: EtmEvent,
    T: EtmTask,
{
    _event: &'a E,
    _task: &'a T,
    _guard: GenericPeripheralGuard<{ crate::system::Peripheral::Etm as u8 }>,
}

impl<E, T, const C: u8> Drop for EtmConfiguredChannel<'_, E, T, C>
where
    E: EtmEvent,
    T: EtmTask,
{
    fn drop(&mut self) {
        debug!("Drop ETM channel {}", C);
        disable_channel(C);
    }
}

macro_rules! create_etm {
    ($($num:literal),+) => {
        paste::paste! {
            /// ETM Instance
            ///
            /// Provides access to all the [EtmChannel]
            pub struct Etm<'d> {
                _peripheral: crate::peripherals::ETM<'d>,
                $(
                    /// An individual ETM channel, identified by its index number.
                    pub [< channel $num >]: EtmChannel<$num>,
                )+
            }

            impl<'d> Etm<'d> {
                /// Creates a new `Etm` instance.
                pub fn new(peripheral: crate::peripherals::ETM<'d>) -> Self {
                    Self {
                        _peripheral: peripheral,
                        $([< channel $num >]: EtmChannel { },)+
                    }
                }
            }
        }
    };
}

create_etm!(
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49
);

#[doc(hidden)]
pub trait EtmEvent: crate::private::Sealed {
    fn id(&self) -> u8;
}

#[doc(hidden)]
pub trait EtmTask: crate::private::Sealed {
    fn id(&self) -> u8;
}
