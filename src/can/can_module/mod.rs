#![allow(clippy::cast_possible_truncation)]

mod service_request;

use super::can_node::{Node, NodeConfig};
use crate::can::NodeId;
use crate::util::wait_nop_cycles;
use crate::{pac, scu};
use core::marker::PhantomData;

pub trait ModuleId {}

pub struct Module0;
impl ModuleId for Module0 {}

pub struct Module1;
impl ModuleId for Module1 {}

// Type states for Module
pub struct Disabled;
pub struct Enabled;

pub struct Module<ModuleId, Reg, State> {
    nodes_taken: [bool; 4],
    _phantom: PhantomData<(ModuleId, Reg, State)>,
}

impl<ModuleId, Reg> Module<ModuleId, Reg, Disabled> {
    /// Create a new (disabled) CAN module
    pub fn new(_module_id: ModuleId) -> Self {
        Self {
            nodes_taken: [false; 4],
            _phantom: PhantomData,
        }
    }
}

macro_rules! impl_can_module {
    ($module_reg:path, $($m:ident)::+, $ModuleReg:ty, $ModuleId: ty) => {
        impl Module<$ModuleId, $ModuleReg, Disabled> {
            fn is_enabled(&self) -> bool {
                // SAFETY: DISS is a RH bit
                !unsafe { $module_reg.clc().read() }.diss().get()
            }

            /// Enable the CAN module
            #[must_use] pub fn enable(self) -> Module<$ModuleId, $ModuleReg, Enabled> {
                scu::wdt::clear_cpu_endinit_inline();

                // SAFETY: DISR is a RW bit, bits 2 and 31:4 are written with 0
                unsafe { $module_reg.clc().modify_atomic(|r| r.disr().set(false)) };
                while !self.is_enabled() {}

                scu::wdt::set_cpu_endinit_inline();

                Module::<$ModuleId, $ModuleReg, Enabled> {
                    nodes_taken: [false; 4],
                    _phantom: PhantomData,
                }
            }
        }

        impl Module<$ModuleId, $ModuleReg, Enabled> {
            /// Take ownership of a CAN node and configure it
            pub fn take_node<I>(&mut self, node_id: I, config: NodeConfig) -> Option<Node<$($m)::+::N, $ModuleReg, I, crate::can::can_node::Configurable>> where I: NodeId {
                let node_index = node_id.as_index();

                #[allow(clippy::indexing_slicing)]
                let flag : &mut bool = &mut self.nodes_taken[node_index];

                // Check if node is already taken, return None if it is
                if *flag {
                    return None;
                }

                // Mark node as taken
                *flag = true;

                // Create node
                Node::<$($m)::+::N, $ModuleReg, I, crate::can::can_node::Configurable>::new(self, node_id, config).ok()
            }

            pub(crate) fn set_clock_source(
                &self,
                clock_select: ClockSelect,
                clock_source: ClockSource,
            ) -> Result<(), ()> {
                // SAFETY: Entire MCR register is readable
                let mcr = unsafe { $module_reg.mcr().read() };

                // Enable CCCE and CI
                let mcr = mcr
                    .ccce()
                    .set(true)
                    .ci()
                    .set(true);

                // SAFETY: CCCE and CI are RW bits, bits 23:8 are written with 0
                unsafe { $module_reg.mcr().write(mcr) }

                // Select clock
                let clock_source: u8 = clock_source.into();

                let mcr = match clock_select.0 {
                    0 => mcr.clksel0().set(clock_source.into()),
                    1 => mcr.clksel1().set(clock_source.into()),
                    2 => mcr.clksel2().set(clock_source.into()),
                    3 => mcr.clksel3().set(clock_source.into()),
                    _ => unreachable!(),
                };

                // SAFETY: CLKSELx are 2 bits fields, clock_source is in range [1,3], bits 23:8 are written with 0
                unsafe { $module_reg.mcr().write(mcr) }

                // Disable CCCE and CI
                let mcr = mcr.ccce().set(false).ci().set(false);
                // SAFETY: CCCE and CI are RW bits, bits 23:8 are written with 0
                unsafe { $module_reg.mcr().write(mcr) }

                // TODO Is this enough or we need to wait until actual_clock_source == clock_source
                // Wait for clock switch
                 wait_nop_cycles(10);

                // Check if clock switch was successful
                // SAFETY: Entire MCR register is readable
                let mcr = unsafe { $module_reg.mcr().read() };

                let actual_clock_source = match clock_select.0 {
                    0 => mcr.clksel0().get(),
                    1 => mcr.clksel1().get(),
                    2 => mcr.clksel2().get(),
                    3 => mcr.clksel3().get(),
                    _ => unreachable!(),
                };

                if actual_clock_source != clock_source {
                    return Err(());
                }

                Ok(())
            }

            pub(crate) fn registers(&self) -> &$ModuleReg {
                &$module_reg
            }

            pub(crate) fn ram_base_address(&self) -> u32 {
                // TODO Ugly hack to obtain the ram base addresssize
                // This is needed because current pac does not provide it
                ($module_reg.accen0().ptr() as u32) - 33020u32
            }
        }
    };
}

impl_can_module!(pac::CAN0, pac::can0, pac::can0::Can0, Module0);
impl_can_module!(pac::CAN1, pac::can1, pac::can1::Can1, Module1);

pub(crate) struct ClockSelect(pub(crate) u8);

impl<T> From<T> for ClockSelect
where
    T: NodeId,
{
    fn from(value: T) -> Self {
        ClockSelect(value.as_index() as u8)
    }
}

#[derive(Default, Clone, Copy)]
pub enum ClockSource {
    Asynchronous,
    Synchronous,
    #[default]
    Both,
}

impl From<ClockSource> for u8 {
    fn from(x: ClockSource) -> Self {
        match x {
            ClockSource::Asynchronous => 1,
            ClockSource::Synchronous => 2,
            ClockSource::Both => 3,
        }
    }
}
