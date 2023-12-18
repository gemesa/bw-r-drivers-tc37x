// TODO Remove asap
#![allow(dead_code)]

use super::baud_rate::*;
use super::can_module::ClockSource;
use super::frame::Frame;
use super::CanModule;
use crate::util::wait_nop_cycles;
use tc37x_pac::hidden::RegValue;

// TODO Default values are not valid
#[derive(Default)]
pub struct BaudRate {
    pub baud_rate: u32,
    pub sample_point: u16,
    pub sync_jump_with: u16,
    pub prescalar: u16,
    pub time_segment_1: u8,
    pub time_segment_2: u8,
}

// TODO Default values are not valid
#[derive(Default)]
pub struct FastBaudRate {
    pub baud_rate: u32,
    pub sample_point: u16,
    pub sync_jump_with: u16,
    pub prescalar: u16,
    pub time_segment_1: u8,
    pub time_segment_2: u8,
    pub transceiver_delay_offset: u8,
}

#[derive(PartialEq, Debug, Default, Copy, Clone)]
pub enum FrameMode {
    // TODO refactor (annabo)
    #[default]
    Standard,
    FdLong,
    FdLongAndFast,
}
#[derive(PartialEq, Debug, Default)]
pub enum FrameType
// TODO refactor (annabo)
{
    #[default]
    Receive,
    Transmit,
    TransmitAndReceive,
    RemoteRequest,
    RemoteAnswer,
}

#[derive(Clone, Copy, Default)]
pub enum TxMode {
    #[default]
    DedicatedBuffers,
    Fifo,
    Queue,
    SharedFifo,
    SharedQueue,
}

#[derive(Clone, Copy, Default)]
pub enum RxMode {
    #[default]
    DedicatedBuffers,
    Fifo0,
    Fifo1,
    SharedFifo0,
    SharedFifo1,
    SharedAll,
}

#[derive(Default)]
pub struct CanNodeConfig {
    pub clock_source: ClockSource,
    pub calculate_bit_timing_values: bool,
    pub baud_rate: BaudRate,
    pub fast_baud_rate: FastBaudRate,
    pub frame_mode: FrameMode,
    pub frame_type: FrameType,
    pub tx_mode: TxMode,
    pub rx_mode: RxMode,
    pub tx_buffer_data_field_size: u8, //(TODO) limit possibile values to valid ones
    pub message_ram_tx_buffers_start_address: u16,
}

#[derive(Copy, Clone, Debug)]
pub struct NodeId(pub(crate) u8);

impl NodeId {
    pub const fn new(n: u8) -> Self {
        Self(n)
    }
}

pub struct NewCanNode {
    module: CanModule,
    node_id: NodeId,
    inner: tc37x_pac::can0::Node,
}

pub struct CanNode {
    module: CanModule,
    node_id: NodeId,
    inner: tc37x_pac::can0::Node,
    frame_mode: FrameMode,
}

impl CanNode {
    /// Only a module can create a node. This function is only accessible from within this crate.
    pub(crate) fn new(module: CanModule, node_id: NodeId) -> NewCanNode {
        let inner = module.registers().node(node_id.0.into());
        NewCanNode {
            module,
            node_id,
            inner,
        }
    }
}

impl NewCanNode {
    pub fn configure(self, config: CanNodeConfig) -> Result<CanNode, ()> {
        self.module
            .set_clock_source(self.node_id.into(), config.clock_source);

        // TODO Document why this is needed
        wait_nop_cycles(10);

        self.enable_configuration_change();

        self.configure_baud_rate(config.calculate_bit_timing_values, &config.baud_rate);

        // for CAN FD frames, set fast baud rate
        if config.frame_mode != FrameMode::Standard {
            self.configure_fast_baud_rate(
                config.calculate_bit_timing_values,
                &config.fast_baud_rate,
            );
        }

        // transmit frame configuration
        if let FrameType::Transmit
        | FrameType::TransmitAndReceive
        | FrameType::RemoteRequest
        | FrameType::RemoteAnswer = config.frame_type
        {
            self.set_tx_buffer_data_field_size(config.tx_buffer_data_field_size);
            self.set_tx_buffer_start_address(config.message_ram_tx_buffers_start_address);
        }

        self.set_frame_mode(config.frame_mode);

        self.disable_configuration_change();

        // TODO FifoData from config
        self.set_rx_fifo0(FifoData {
            field_size: DataFieldSize::_8,
            operation_mode: RxFifoMode::Blocking,
            watermark_level: 0,
            size: 4,
            start_address: 0x100,
        });

        // TODO DedicatedData from config
        self.set_tx_fifo(
            DedicatedData {
                field_size: DataFieldSize::_8,
                start_address: 0x440,
            },
            4,
        );

        // self.interrupt(
        //     InterruptGroup::Rxf0n,
        //     Interrupt::RxFifo0newMessage,
        //     InterruptLine(1),
        //     2,
        //     Tos::Cpu0,
        // );
        //
        // self.connect_pin_rx(
        //     RXD00B_P20_7_IN,
        //     InputMode::PULL_UP,
        //     PadDriver::CmosAutomotiveSpeed3,
        // );
        //
        // self.connect_pin_tx(
        //     TXD00_P20_8_OUT,
        //     OutputMode::PUSH_PULL,
        //     PadDriver::CmosAutomotiveSpeed3,
        // );

        Ok(CanNode {
            frame_mode: config.frame_mode,
            module: self.module,
            node_id: self.node_id,
            inner: self.inner,
        })
    }

    fn set_rx_fifo0(&self, data: FifoData) {
        self.set_rx_fifo0_data_field_size(data.field_size);
        self.set_rx_fifo0_start_address(data.start_address);
        self.set_rx_fifo0_size(data.size);
        self.set_rx_fifo0_operating_mode(data.operation_mode);
        self.set_rx_fifo0_watermark_level(data.watermark_level);
    }

    fn set_rx_fifo0_data_field_size(&self, size: DataFieldSize) {
        let size = tc37x_pac::can0::node::rxesc::F0Ds(size as u8);
        unsafe { self.inner.rxesc().modify(|r| r.f0ds().set(size)) };
    }

    fn set_rx_fifo0_start_address(&self, address: u16) {
        unsafe { self.inner.rxf0c().modify(|r| r.f0sa().set(address >> 2)) };
    }

    fn set_rx_fifo0_size(&self, size: u8) {
        unsafe { self.inner.rxf0c().modify(|r| r.f0s().set(size)) };
    }

    fn set_rx_fifo0_watermark_level(&self, level: u8) {
        unsafe { self.inner.rxf0c().modify(|r| r.f0wm().set(level)) };
    }

    fn set_rx_fifo0_operating_mode(&self, mode: RxFifoMode) {
        unsafe {
            self.inner
                .rxf0c()
                .modify(|r| r.f0om().set(mode == RxFifoMode::Overwrite))
        };
    }

    fn set_tx_fifo(&self, buffers: DedicatedData, fifo_size: u8) {
        self.set_inner_tx_buffers(buffers);
        self.set_inner_tx_fifo_queue(TxMode::Fifo, fifo_size);
        self.set_inner_tx_int(fifo_size);
    }

    fn set_inner_tx_buffers(&self, dedicated: DedicatedData) {
        self.set_tx_buffer_data_field_size(dedicated.field_size as u8);
        self.set_tx_buffer_start_address(dedicated.start_address);
    }

    fn set_inner_tx_fifo_queue(&self, mode: TxMode, size: u8) {
        self.set_transmit_fifo_queue_mode(mode);
        self.set_transmit_fifo_queue_size(size);
    }

    fn set_inner_tx_int(&self, size: u8) {
        for id in 0..size {
            self.enable_tx_buffer_transmission_interrupt(TxBufferId(id));
        }
    }

    fn enable_tx_buffer_transmission_interrupt(&self, tx_buffer_id: TxBufferId) {
        unsafe {
            self.inner.txbtie().modify(|mut r| {
                *r.data_mut_ref() |= 1 << tx_buffer_id.0;
                r
            })
        };
    }

    fn set_transmit_fifo_queue_mode(&self, mode: TxMode) {
        if let TxMode::Fifo | TxMode::Queue = mode {
            let val = (mode as u8) != 0;
            unsafe { self.inner.txbc().modify(|r| r.tfqm().set(val)) };
        } else {
            panic!("invalid fifo queue mode");
        }
    }

    fn set_transmit_fifo_queue_size(&self, number: u8) {
        unsafe { self.inner.txbc().modify(|r| r.tfqs().set(number)) };
    }

    fn enable_configuration_change(&self) {
        let cccr = self.inner.cccr();

        if unsafe { cccr.read() }.init().get() {
            unsafe { cccr.modify(|r| r.cce().set(false)) };
            while unsafe { cccr.read() }.cce().get() {}

            unsafe { cccr.modify(|r| r.init().set(false)) };
            while unsafe { cccr.read() }.init().get() {}
        }

        unsafe { cccr.modify(|r| r.init().set(true)) };
        while !unsafe { cccr.read() }.init().get() {}

        unsafe { cccr.modify(|r| r.cce().set(true).init().set(true)) };
    }

    fn disable_configuration_change(&self) {
        let cccr = self.inner.cccr();

        unsafe { cccr.modify(|r| r.cce().set(false)) };

        while unsafe { cccr.read() }.cce().get() {}

        unsafe { cccr.modify(|r| r.init().set(false)) };

        while unsafe { cccr.read() }.init().get() {}
    }

    fn configure_baud_rate(&self, calculate_bit_timing_values: bool, baud_rate: &BaudRate) {
        if calculate_bit_timing_values {
            let module_freq = crate::scu::ccu::get_mcan_frequency() as f32;
            let timing: BitTiming = calculate_bit_timing(
                module_freq,
                baud_rate.baud_rate,
                baud_rate.sample_point,
                baud_rate.sync_jump_with,
            );
            self.set_bit_timing(timing);
        } else {
            self.set_bit_timing_values(
                baud_rate.sync_jump_with as u8,
                baud_rate.time_segment_2,
                baud_rate.time_segment_1,
                baud_rate.prescalar,
            )
        }
    }

    fn configure_fast_baud_rate(
        &self,
        calculate_bit_timing_values: bool,
        baud_rate: &FastBaudRate,
    ) {
        if calculate_bit_timing_values {
            let module_freq = crate::scu::ccu::get_mcan_frequency() as f32;
            self.set_fast_bit_timing(
                module_freq,
                baud_rate.baud_rate,
                baud_rate.sample_point,
                baud_rate.sync_jump_with,
            );
        } else {
            self.set_fast_bit_timing_values(
                baud_rate.sync_jump_with as u8,
                baud_rate.time_segment_2,
                baud_rate.time_segment_1,
                baud_rate.prescalar as u8,
            );
        }

        if baud_rate.transceiver_delay_offset != 0 {
            self.set_transceiver_delay_compensation_offset(baud_rate.transceiver_delay_offset);
        }
    }

    fn set_bit_timing(&self, timing: BitTiming) {
        unsafe {
            self.inner.nbtp().modify(|r| {
                r.nbrp()
                    .set(timing.brp)
                    .nsjw()
                    .set(timing.sjw)
                    .ntseg1()
                    .set(timing.tseg1)
                    .ntseg2()
                    .set(timing.tseg2)
            })
        }
    }

    fn set_bit_timing_values(&self, sjw: u8, time_segment2: u8, time_segment1: u8, prescaler: u16) {
        unsafe {
            self.inner.nbtp().modify(|r| {
                r.nsjw()
                    .set(sjw)
                    .ntseg1()
                    .set(time_segment1)
                    .ntseg2()
                    .set(time_segment2)
                    .nbrp()
                    .set(prescaler)
            })
        };
    }

    fn set_fast_bit_timing(&self, module_freq: f32, baudrate: u32, sample_point: u16, sjw: u16) {
        let timing = calculate_fast_bit_timing(module_freq, baudrate, sample_point, sjw);

        unsafe {
            self.inner.dbtp().modify(|r| {
                r.dbrp()
                    .set(timing.brp.try_into().unwrap())
                    .dsjw()
                    .set(timing.sjw)
                    .dtseg1()
                    .set(timing.tseg1)
                    .dtseg2()
                    .set(timing.tseg2)
            })
        }
    }

    fn set_fast_bit_timing_values(
        &self,
        sjw: u8,
        time_segment2: u8,
        time_segment1: u8,
        prescaler: u8,
    ) {
        unsafe {
            self.inner.dbtp().modify(|r| {
                r.dsjw()
                    .set(sjw)
                    .dtseg1()
                    .set(time_segment1)
                    .dtseg2()
                    .set(time_segment2)
                    .dbrp()
                    .set(prescaler)
            })
        };
    }

    fn set_tx_buffer_data_field_size(&self, data_field_size: u8) {
        let data_field_size = tc37x_pac::can0::node::txesc::Tbds(data_field_size);
        unsafe { self.inner.txesc().modify(|r| r.tbds().set(data_field_size)) };
    }

    fn set_tx_buffer_start_address(&self, address: u16) {
        unsafe { self.inner.txbc().modify(|r| r.tbsa().set(address >> 2)) };
    }

    fn set_frame_mode(&self, frame_mode: FrameMode) {
        let (fdoe, brse) = match frame_mode {
            FrameMode::Standard => (false, false),
            FrameMode::FdLong => (true, false),
            FrameMode::FdLongAndFast => (true, true),
        };

        unsafe {
            self.inner
                .cccr()
                .modify(|r| r.fdoe().set(fdoe).brse().set(brse))
        };
    }

    fn set_transceiver_delay_compensation_offset(&self, delay: u8) {
        unsafe { self.inner.dbtp().modify(|r| r.tdc().set(true)) };
        unsafe { self.inner.tdcr().modify(|r| r.tdco().set(delay)) };
    }
}

impl CanNode {
    pub fn transmit(&self, _frame: &Frame) -> Result<(), ()> {
        // TODO
        Ok(())
    }

    fn get_rx_fifo0_fill_level(&self) -> u8 {
        unsafe { self.inner.rxf0s().read() }.f0fl().get()
    }

    fn get_rx_fifo1_fill_level(&self) -> u8 {
        unsafe { self.inner.rxf1s().read() }.f1fl().get()
    }

    fn set_rx_buffers_start_address(&self, address: u16) {
        unsafe { self.inner.rxbc().modify(|r| r.rbsa().set(address >> 2)) };
    }

    fn set_rx_fifo0_size(&self, size: u8) {
        unsafe { self.inner.rxf0c().modify(|r| r.f0s().set(size)) };
    }

    fn set_rx_fifo0_start_address(&self, address: u16) {
        unsafe { self.inner.rxf0c().modify(|r| r.f0sa().set(address >> 2)) };
    }

    fn set_rx_fifo0_watermark_level(&self, level: u8) {
        unsafe { self.inner.rxf0c().modify(|r| r.f0wm().set(level)) };
    }

    fn set_rx_fifo1_size(&self, size: u8) {
        unsafe { self.inner.rxf1c().modify(|r| r.f1s().set(size)) };
    }

    fn set_rx_fifo1_start_address(&self, address: u16) {
        unsafe { self.inner.rxf1c().modify(|r| r.f1sa().set(address >> 2)) };
    }

    fn set_rx_fifo1_watermark_level(&self, level: u8) {
        unsafe { self.inner.rxf1c().modify(|r| r.f1wm().set(level)) };
    }

    fn is_tx_event_fifo_element_lost(&self) -> bool {
        unsafe { self.inner.txefs().read() }.tefl().get()
    }

    fn is_tx_event_fifo_full(&self) -> bool {
        unsafe { self.inner.txefs().read() }.eff().get()
    }

    fn is_tx_fifo_queue_full(&self) -> bool {
        unsafe { self.inner.txfqs().read() }.tfqf().get()
    }

    fn pause_trasmission(&self, enable: bool) {
        unsafe { self.inner.cccr().modify(|r| r.txp().set(enable)) };
    }

    fn set_dedicated_tx_buffers_number(&self, number: u8) {
        unsafe { self.inner.txbc().modify(|r| r.ndtb().set(number)) };
    }

    fn set_transmit_fifo_queue_size(&self, number: u8) {
        unsafe { self.inner.txbc().modify(|r| r.tfqs().set(number)) };
    }

    fn set_tx_event_fifo_start_address(&self, address: u16) {
        unsafe { self.inner.txefc().modify(|r| r.efsa().set(address >> 2)) };
    }

    fn set_tx_event_fifo_size(&self, size: u8) {
        unsafe { self.inner.txefc().modify(|r| r.efs().set(size)) };
    }

    fn set_standard_filter_list_start_address(&self, address: u16) {
        unsafe { self.inner.sidfc().modify(|r| r.flssa().set(address >> 2)) };
    }

    fn set_standard_filter_list_size(&self, size: u8) {
        unsafe { self.inner.sidfc().modify(|r| r.lss().set(size)) };
    }

    fn reject_remote_frames_with_standard_id(&self) {
        unsafe { self.inner.gfc().modify(|r| r.rrfs().set(true)) };
    }

    fn set_extended_filter_list_start_address(&self, address: u16) {
        unsafe { self.inner.xidfc().modify(|r| r.flesa().set(address >> 2)) };
    }

    fn set_extended_filter_list_size(&self, size: u8) {
        unsafe { self.inner.xidfc().modify(|r| r.lse().set(size)) };
    }

    fn reject_remote_frames_with_extended_id(&self) {
        unsafe { self.inner.gfc().modify(|r| r.rrfe().set(true)) };
    }
}

#[derive(Clone, Copy)]
pub struct FifoData {
    pub field_size: DataFieldSize,
    pub operation_mode: RxFifoMode,
    pub watermark_level: u8,
    pub size: u8,
    pub start_address: u16,
}

#[derive(Clone, Copy, PartialEq)]
pub enum RxFifoMode {
    Blocking,
    Overwrite,
}

#[repr(u8)]
#[derive(Clone, Copy)]
pub enum DataFieldSize {
    _8,
    _12,
    _16,
    _20,
    _24,
    _32,
    _48,
    _64,
}

#[derive(Clone, Copy)]
pub struct DedicatedData {
    pub field_size: DataFieldSize,
    pub start_address: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub enum InterruptGroup {
    Tefifo,
    Hpe,
    Wati,
    Alrt,
    Moer,
    Safe,
    Boff,
    Loi,
    Reint,
    Rxf1f,
    Rxf0f,
    Rxf1n,
    Rxf0n,
    Reti,
    Traq,
    Traco,
}

#[derive(Debug, Clone, Copy)]
pub enum Interrupt {
    RxFifo0newMessage,
    RxFifo0watermarkReached,
    RxFifo0full,
    RxFifo0messageLost,
    RxFifo1newMessage,
    RxFifo1watermarkReached,
    RxFifo1full,
    RxFifo1messageLost,
    HighPriorityMessage,
    TransmissionCompleted,
    TransmissionCancellationFinished,
    TxFifoEmpty,
    TxEventFifoNewEntry,
    TxEventFifoWatermarkReached,
    TxEventFifoFull,
    TxEventFifoEventLost,
    TimestampWraparound,
    MessageRamaccessFailure,
    TimeoutOccurred,
    MessageStoredToDedicatedRxBuffer,
    BitErrorCorrected,
    BitErrorUncorrected,
    ErrorLoggingOverflow,
    ErrorPassive,
    WarningStatus,
    BusOffStatus,
    Watchdog,
    ProtocolErrorArbitration,
    ProtocolErrorData,
    AccessToReservedAddress,
}

#[repr(transparent)]
#[derive(PartialEq, PartialOrd, Clone, Copy, Debug, Default)]
pub struct InterruptLine(pub u8);

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Tos {
    #[default]
    Cpu0,
    Dma,
    Cpu1,
    Cpu2,
}

pub const RXD00B_P20_7_IN: RxdIn =
    RxdIn::new(CanModuleId::_0, NodeId(0), PortNumber::_20, 7, RxSel::_B);

pub const TXD00_P20_8_OUT: TxdOut = TxdOut::new(
    CanModuleId::_0,
    NodeId(0),
    PortNumber::_20,
    8,
    OutputIdx::ALT5,
);

#[derive(Clone, Copy)]
pub struct InputMode(u32);
impl InputMode {
    pub const NO_PULL_DEVICE: Self = Self(0 << 3);
    pub const PULL_DOWN: Self = Self(1 << 3);
    pub const PULL_UP: Self = Self(2 << 3);
}

#[derive(Clone, Copy)]
pub struct OutputMode(u32);
impl OutputMode {
    pub const PUSH_PULL: OutputMode = Self(0x10 << 3);
    pub const OPEN_DRAIN: OutputMode = Self(0x18 << 3);
    pub const NONE: OutputMode = Self(0);
}

#[derive(Clone, Copy)]
pub enum PadDriver {
    CmosAutomotiveSpeed1 = 0,
    CmosAutomotiveSpeed2 = 1,
    CmosAutomotiveSpeed3 = 2,
    CmosAutomotiveSpeed4 = 3,
    TtlSpeed1 = 8,
    TtlSpeed2 = 9,
    TtlSpeed3 = 10,
    TtlSpeed4 = 11,
    Ttl3v3speed1 = 12,
    Ttl3v3speed2 = 13,
    Ttl3v3speed3 = 14,
    Ttl3v3speed4 = 15,
}

#[derive(PartialEq, PartialOrd, Clone, Copy, Debug, Default)]
pub struct CanModuleId(u8);
impl CanModuleId {
    pub const _0: Self = Self(0);
    pub const _1: Self = Self(0);
}

#[derive(Clone, Copy)]
pub enum PortNumber {
    _00,
    _01,
    _02,
    _10,
    _11,
    _12,
    _13,
    _14,
    _15,
    _20,
    _21,
    _22,
    _23,
    _32,
    _33,
    _34,
    _40,
}

#[derive(Clone, Copy)]
pub struct OutputIdx(u32);
impl OutputIdx {
    pub const GENERAL: Self = Self(0x10 << 3);
    pub const ALT1: Self = Self(0x11 << 3);
    pub const ALT2: Self = Self(0x12 << 3);
    pub const ALT3: Self = Self(0x13 << 3);
    pub const ALT4: Self = Self(0x14 << 3);
    pub const ALT5: Self = Self(0x15 << 3);
    pub const ALT6: Self = Self(0x16 << 3);
    pub const ALT7: Self = Self(0x17 << 3);
}

#[derive(Clone, Copy)]
pub struct RxdIn {
    pub module: CanModuleId,
    pub node_id: NodeId,
    pub port: PortNumber,
    pub pin_index: u8,
    pub select: RxSel,
}

impl RxdIn {
    pub const fn new(
        module: CanModuleId,
        node_id: NodeId,
        port: PortNumber,
        pin_index: u8,
        select: RxSel,
    ) -> Self {
        Self {
            module,
            node_id,
            port,
            pin_index,
            select,
        }
    }
}

#[derive(Clone, Copy)]
pub enum RxSel {
    _A,
    _B,
    _C,
    _D,
    _E,
    _F,
    _G,
    _H,
}

#[derive(Clone, Copy)]
pub struct TxdOut {
    pub module: CanModuleId,
    pub node_id: NodeId,
    pub port: PortNumber,
    pub pin_index: u8,
    pub select: OutputIdx,
}

impl TxdOut {
    pub const fn new(
        module: CanModuleId,
        node_id: NodeId,
        port: PortNumber,
        pin_index: u8,
        select: OutputIdx,
    ) -> Self {
        Self {
            module,
            node_id,
            port,
            pin_index,
            select,
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct TxBufferId(pub u8);
