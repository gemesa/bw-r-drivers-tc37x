#![allow(clippy::identity_op)]
#![allow(clippy::eq_op)]
#![allow(clippy::result_unit_err)]
#![allow(clippy::float_arithmetic)]
// TODO Remove this once the code is stable
#![allow(clippy::undocumented_unsafe_blocks)]

use super::wdt;
use crate::log::debug;
use tc37x::scu;
use tc37x::{RegisterValue, SCU, SMU};

const SYSPLLSTAT_PWDSTAT_TIMEOUT_COUNT: usize = 0x3000;
const OSCCON_PLLLV_OR_HV_TIMEOUT_COUNT: usize = 0x493E0;
const PLL_LOCK_TIMEOUT_COUNT: usize = 0x3000;

const CCUCON_LCK_BIT_TIMEOUT_COUNT: usize = 0x1000;
const PLL_KRDY_TIMEOUT_COUNT: usize = 0x6000;

pub enum InitError {
    ConfigureCCUInitialStep,
    ModulationInit,
    DistributeClockInline,
    ThrottleSysPllClockInline,
}

pub(crate) fn init(config: &Config) -> Result<(), InitError> {
    configure_ccu_initial_step(config).map_err(|()| InitError::ConfigureCCUInitialStep)?;
    modulation_init(config).map_err(|()| InitError::ModulationInit)?;
    distribute_clock_inline(config).map_err(|()| InitError::DistributeClockInline)?;
    throttle_sys_pll_clock_inline(config).map_err(|()| InitError::ThrottleSysPllClockInline)?;
    Ok(())
}

fn wait_ccucon0_lock() -> Result<(), ()> {
    wait_cond(CCUCON_LCK_BIT_TIMEOUT_COUNT, || {
        unsafe { SCU.cucon0().read() }.lck().get()
    })
}

fn wait_ccucon1_lock() -> Result<(), ()> {
    wait_cond(CCUCON_LCK_BIT_TIMEOUT_COUNT, || {
        unsafe { SCU.ccucon1().read() }.lck().get()
    })
}

fn wait_ccucon2_lock() -> Result<(), ()> {
    wait_cond(CCUCON_LCK_BIT_TIMEOUT_COUNT, || {
        unsafe { SCU.ccucon2().read() }.lck().get()
    })
}

fn wait_ccucon5_lock() -> Result<(), ()> {
    wait_cond(CCUCON_LCK_BIT_TIMEOUT_COUNT, || {
        unsafe { SCU.ccucon5().read() }.lck().get() 
    })
}

fn wait_divider() -> Result<(), ()> {
    wait_cond(PLL_KRDY_TIMEOUT_COUNT, || {
        let sys = unsafe { SCU.syspllstat().read() };
        let per = unsafe { SCU.perpllstat().read() };
        let sys_k2 = sys.k2rdy().get();
        let per_k2 = sys.k2rdy().get();
        let per_k3 = per.k3rdy().get();
        sys_k2 == false || per_k2 == false || per_k3 == false
    })
}

fn set_pll_power(
    syspllpower: bool,
    perpllpower: bool,
) -> Result<(), ()> {
    unsafe { SCU.syspllcon0().modify(|r| r.pllpwd().set(syspllpower)) };
    unsafe { SCU.perpllcon0().modify(|r| r.pllpwd().set(perpllpower)) };

    wait_cond(SYSPLLSTAT_PWDSTAT_TIMEOUT_COUNT, || {
        let sys = unsafe { SCU.syspllstat().read() };
        let per = unsafe { SCU.perpllstat().read() };
        (syspllpower) == (sys.pwdstat().get()) || (perpllpower) == (per.pwdstat().get())
    })
}

pub(crate) fn configure_ccu_initial_step(config: &Config) -> Result<(), ()> {
    // TODO Should be an enum variant in the pac crate
    const CLKSEL_BACKUP: u8 = 0;

    wdt::clear_safety_endinit_inline();

    wait_ccucon0_lock()?;

    // TODO Explain this
    unsafe {
        SCU.ccucon0().modify(|r| {
            r.clksel()
                .set(scu::Ccucon0::Clksel(CLKSEL_BACKUP))
                .up()
                .set(true)
        })
    };
    wait_ccucon0_lock()?;

    // disable SMU
    {
        // The SMU core configuration is only possible if this field is set to 0xBC
        unsafe { SMU.keys().init(|r| r.cfglck().set(0xBC)) };

        // FIXME After pac update, this is a BW patch on pac
        unsafe { SMU.ag8cfj()[0].modify(|r| r.set_raw(r.get_raw() & !0x1D)) };
        unsafe { SMU.ag8cfj()[1].modify(|r| r.set_raw(r.get_raw() & !0x1D)) };
        unsafe { SMU.ag8cfj()[2].modify(|r| r.set_raw(r.get_raw() & !0x1D)) };

        unsafe { SMU.keys().init(|r| r.cfglck().set(0)) };
    }

    // Power down the both the PLLs before configuring registers
    // Both the PLLs are powered down to be sure for asynchronous PLL registers
    // update cause no glitches.
    set_pll_power(
        scu::Syspllcon0::Pllpwd::CONST_00,
        scu::Perpllcon0::Pllpwd::CONST_00,
    )?;

    let plls_params = &config.pll_initial_step.plls_parameters;

    // Configure the oscillator, required oscillator mode is external crystal
    if let PllInputClockSelection::F0sc0 | PllInputClockSelection::FSynclk =
        plls_params.pll_input_clock_selection
    {
        // TODO Should be an enum variant in the pac crate
        const MODE_EXTERNALCRYSTAL: u8 = 0;

        let mode = MODE_EXTERNALCRYSTAL;
        let oscval: u8 = ((plls_params.xtal_frequency / 1000000) - 15)
            .try_into()
            .map_err(|_| ())?;

        unsafe {
            SCU.osccon()
                .modify(|r| r.mode().set(scu::Osccon::Mode(mode)).oscval().set(oscval))
        };
    }

    // Configure the initial steps for the system PLL
    unsafe {
        SCU.syspllcon0().modify(|r| {
            r.pdiv()
                .set(plls_params.sys_pll.p_divider)
                .ndiv()
                .set(plls_params.sys_pll.n_divider)
                .insel()
                .set(scu::Syspllcon0::Insel(
                    plls_params.pll_input_clock_selection as u8,
                ))
        })
    }

    // Configure the initial steps for the peripheral PLL
    unsafe {
        SCU.perpllcon0().modify(|r| {
            r.divby()
                .set(plls_params.per_pll.k3_divider_bypass.into())
                .pdiv()
                .set(plls_params.per_pll.p_divider)
                .ndiv()
                .set(plls_params.per_pll.n_divider)
        })
    }

    set_pll_power(
        scu::Syspllcon0::Pllpwd::CONST_11,
        scu::Perpllcon0::Pllpwd::CONST_11,
    )?;

    wait_divider()?;

    unsafe {
        SCU.syspllcon1()
            .modify(|r| r.k2div().set(plls_params.sys_pll.k2_divider));
    }

    unsafe {
        SCU.perpllcon1().modify(|r| {
            r.k2div()
                .set(plls_params.per_pll.k2_divider)
                .k3div()
                .set(plls_params.per_pll.k3_divider)
        })
    };

    wait_divider()?;

    // Check if OSC frequencies are in the limit
    wait_cond(OSCCON_PLLLV_OR_HV_TIMEOUT_COUNT, || {
        let osccon = unsafe { SCU.osccon().read() };
        osccon.plllv().get().0 == 0 && osccon.pllhv().get().0 == 0
    })?;

    // Start PLL locking for latest set values
    {
        unsafe { SCU.syspllcon0().modify(|r| r.resld().set(true)) };
        unsafe { SCU.perpllcon0().modify(|r| r.resld().set(true)) };

        wait_cond(PLL_LOCK_TIMEOUT_COUNT, || {
            let sys = unsafe { SCU.syspllstat().read() };
            let per = unsafe { SCU.perpllstat().read() };
            sys.lock().get().0 == 0 || per.lock().get().0 == 0
        })?;
    }

    // enable SMU alarms
    {
        // TODO Explain these magic numbers
        unsafe { SMU.keys().write(RegisterValue::new(0xBC)) };
        unsafe { SMU.cmd().write(RegisterValue::new(0x00000005)) };
        unsafe {
            SMU.agi()[8].write(RegisterValue::new(0x1D));
        }
        unsafe { SMU.keys().write(RegisterValue::new(0)) };
    }

    {
        let ccucon0 = unsafe { SCU.ccucon0().read() }
            .clksel()
            .set(scu::Ccucon0::Clksel::CONST_11)
            .up()
            .set(scu::Ccucon0::Up::CONST_11);

        wait_ccucon0_lock()?;

        unsafe { SCU.ccucon0().write(ccucon0) };

        wait_ccucon0_lock()?;
    }

    wdt::set_safety_endinit_inline();

    Ok(())
}

pub(crate) fn modulation_init(config: &Config) -> Result<(), ()> {
    if let ModulationEn::Enabled = config.modulation.enable {
        let rgain_p = calc_rgain_parameters(config.modulation.amp);

        wdt::clear_safety_endinit_inline();

        unsafe {
            SCU.syspllcon2()
                .modify(|r| r.modcfg().set((0x3 << 10) | rgain_p.rgain_hex))
        };

        unsafe {
            SCU.syspllcon0()
                .modify(|r| r.moden().set(scu::Syspllcon0::Moden::CONST_11))
        };

        wdt::set_safety_endinit_inline();
    }
    Ok(())
}

pub struct RGainValues {
    pub rgain_nom: f32,
    pub rgain_hex: u16,
}

fn calc_rgain_parameters(modamp: ModulationAmplitude) -> RGainValues {
    const MA_PERCENT: [f32; 6] = [0.5, 1.0, 1.25, 1.5, 2.0, 2.5];

    #[allow(clippy::indexing_slicing)]
    let mod_amp = MA_PERCENT[modamp as usize];

    let fosc_hz = get_osc_frequency();
    let syspllcon0 = unsafe { SCU.syspllcon0().read() };
    let fdco_hz = (fosc_hz * (f32::from(syspllcon0.ndiv().get()) + 1.0))
        / (f32::from(syspllcon0.pdiv().get()) + 1.0);

    let rgain_nom = 2.0 * (mod_amp / 100.0) * (fdco_hz / 3600000.0);
    let rgain_hex = ((rgain_nom * 32.0) + 0.5) as u16;

    RGainValues {
        rgain_nom,
        rgain_hex,
    }
}

pub(crate) fn distribute_clock_inline(config: &Config) -> Result<(), ()> {
    wdt::clear_safety_endinit_inline();

    // CCUCON0 config
    {
        let mut cuccon0 = unsafe { SCU.ccucon0().read() };
        *cuccon0.data_mut_ref() &= !(config.clock_distribution.ccucon0.mask);
        *cuccon0.data_mut_ref() |=
            config.clock_distribution.ccucon0.mask & config.clock_distribution.ccucon0.value;

        wait_ccucon0_lock()?;

        unsafe { SCU.ccucon0().write(cuccon0) };

        wait_ccucon0_lock()?;
    }
    // CCUCON1 config
    {
        let mut ccucon1 = unsafe { SCU.ccucon1().read() };
        if ccucon1.clkselmcan().get() !=  scu::Ccucon1::Clkselmcan::CONST_00 /*ccucon1::Clkselmcan::CLKSELMCAN_STOPPED*/
            || ccucon1.clkselmsc().get() != scu::Ccucon1::Clkselmsc::CONST_11 /*ccucon1::Clkselmsc::CLKSELMSC_STOPPED*/
            || ccucon1.clkselqspi().get() != scu::Ccucon1::Clkselqspi::CONST_22
        /*ccucon1::Clkselqspi::CLKSELQSPI_STOPPED*/
        {
            *ccucon1.data_mut_ref() &= !config.clock_distribution.ccucon1.mask;
            *ccucon1.data_mut_ref() |=
                config.clock_distribution.ccucon1.mask & config.clock_distribution.ccucon1.value;

            ccucon1 = ccucon1
                .clkselmcan()
                .set(
                    scu::Ccucon1::Clkselmcan::CONST_00, /*ccucon1::Clkselmcan::CLKSELMCAN_STOPPED*/
                )
                .clkselmsc()
                .set(
                    scu::Ccucon1::Clkselmsc::CONST_11, /*ccucon1::Clkselmsc::CLKSELMSC_STOPPED*/
                )
                .clkselqspi()
                .set(
                    scu::Ccucon1::Clkselqspi::CONST_22, /*ccucon1::Clkselqspi::CLKSELQSPI_STOPPED*/
                );

            wait_ccucon1_lock()?;
            unsafe { SCU.ccucon1().write(ccucon1) };
            wait_ccucon1_lock()?;
        }

        ccucon1 = unsafe { SCU.ccucon1().read() };
        *ccucon1.data_mut_ref() &= !config.clock_distribution.ccucon1.mask;
        *ccucon1.data_mut_ref() |=
            config.clock_distribution.ccucon1.mask & config.clock_distribution.ccucon1.value;

        wait_ccucon1_lock()?;
        unsafe { SCU.ccucon1().write(ccucon1) };
        wait_ccucon1_lock()?;
    }

    // CCUCON2 config
    {
        let mut ccucon2 = unsafe { SCU.ccucon2().read() };
        if ccucon2.clkselasclins().get() != scu::Ccucon2::Clkselasclins::CONST_00
        /*scu::Ccucon2::Clkselasclins::CLKSELASCLINS_STOPPED*/
        {
            ccucon2 = unsafe { SCU.ccucon2().read() };
            *ccucon2.data_mut_ref() &= !config.clock_distribution.ccucon2.mask;
            *ccucon2.data_mut_ref() =
                config.clock_distribution.ccucon2.mask & config.clock_distribution.ccucon2.value;

            ccucon2 = ccucon2.clkselasclins().set(
                scu::Ccucon2::Clkselasclins::CONST_00, /*scu::Ccucon2::Clkselasclins::CLKSELASCLINS_STOPPED*/
            );

            wait_ccucon2_lock()?;

            unsafe { SCU.ccucon2().write(ccucon2) };

            wait_ccucon2_lock()?;
        }

        ccucon2 = unsafe { SCU.ccucon2().read() };
        *ccucon2.data_mut_ref() &= !config.clock_distribution.ccucon2.mask;
        *ccucon2.data_mut_ref() |=
            config.clock_distribution.ccucon2.mask & config.clock_distribution.ccucon2.value;

        wait_ccucon2_lock()?;
        unsafe { SCU.ccucon2().write(ccucon2) };
        wait_ccucon2_lock()?;
    }

    // CUCCON5 config
    {
        let mut ccucon5 = unsafe { SCU.ccucon5().read() };
        *ccucon5.data_mut_ref() &= !config.clock_distribution.ccucon5.mask;
        *ccucon5.data_mut_ref() |=
            config.clock_distribution.ccucon5.mask & config.clock_distribution.ccucon5.value;
        ccucon5 = ccucon5.up().set(scu::Ccucon5::Up::CONST_11);

        wait_ccucon5_lock()?;

        unsafe { SCU.ccucon5().write(ccucon5) };

        wait_ccucon5_lock()?;
    }

    // CUCCON6 config
    {
        unsafe {
            SCU.ccucon6().modify(|mut r| {
                *r.data_mut_ref() &= !config.clock_distribution.ccucon6.mask;
                *r.data_mut_ref() |= config.clock_distribution.ccucon6.mask
                    & config.clock_distribution.ccucon6.value;
                r
            })
        };
    }

    // CUCCON7 config
    {
        unsafe {
            SCU.ccucon7().modify(|mut r| {
                *r.data_mut_ref() &= !config.clock_distribution.ccucon7.mask;
                *r.data_mut_ref() |= config.clock_distribution.ccucon7.mask
                    & config.clock_distribution.ccucon7.value;
                r
            })
        };
    }

    // CUCCON8 config
    {
        unsafe {
            SCU.ccucon8().modify(|mut r| {
                *r.data_mut_ref() &= !config.clock_distribution.ccucon8.mask;
                *r.data_mut_ref() |= config.clock_distribution.ccucon8.mask
                    & config.clock_distribution.ccucon8.value;
                r
            })
        };
    }

    wdt::set_safety_endinit_inline();

    Ok(())
}

pub(crate) fn throttle_sys_pll_clock_inline(config: &Config) -> Result<(), ()> {
    for pll_step_count in 0..config.sys_pll_throttle.len() {
        wdt::clear_safety_endinit_inline();

        wait_cond(PLL_KRDY_TIMEOUT_COUNT, || {
            unsafe { SCU.syspllstat().read() }.k2rdy().get().0 != 1
        })?;

        #[allow(clippy::indexing_slicing)]
        let k2div = config.sys_pll_throttle[pll_step_count].k2_step;

        unsafe { SCU.syspllcon1().modify(|r| r.k2div().set(k2div)) };

        wdt::set_safety_endinit_inline();
    }
    Ok(())
}

/// Wait until cond return true or timeout
#[inline]
pub(crate) fn wait_cond(timeout_cycle_count: usize, cond: impl Fn() -> bool) -> Result<(), ()> {
    let mut timeout_cycle_count = timeout_cycle_count;
    while cond() {
        timeout_cycle_count -= 1;
        if timeout_cycle_count == 0 {
            return Err(());
        }
    }

    Ok(())
}

// PLL management
const EVR_OSC_FREQUENCY: u32 = 100_000_000;
const XTAL_FREQUENCY: u32 = 20_000_000;
const SYSCLK_FREQUENCY: u32 = 20_000_000;

#[inline]
pub(crate) fn get_osc_frequency() -> f32 {
    let f = match unsafe { SCU.syspllcon0().read() }.insel().get() {
        scu::Syspllcon0::Insel::CONST_00 => EVR_OSC_FREQUENCY,
        scu::Syspllcon0::Insel::CONST_11 => XTAL_FREQUENCY,
        scu::Syspllcon0::Insel::CONST_22 => SYSCLK_FREQUENCY,
        _ => 0,
    };
    f as f32
}

pub(crate) fn get_pll_frequency() -> u32 {
    let osc_freq = get_osc_frequency();
    let syspllcon0 = unsafe { SCU.syspllcon0().read() };
    let syspllcon1 = unsafe { SCU.syspllcon1().read() };
    let f = (osc_freq * f32::from(syspllcon0.ndiv().get() + 1))
        / f32::from((syspllcon1.k2div().get() + 1) * (syspllcon0.pdiv().get() + 1));
    f as u32
}

pub(crate) fn get_per_pll_frequency1() -> u32 {
    let osc_freq = get_osc_frequency();
    let perpllcon0 = unsafe { SCU.perpllcon0().read() };
    let perpllcon1 = unsafe { SCU.perpllcon1().read() };
    let f = (osc_freq * f32::from(perpllcon0.ndiv().get() + 1))
        / f32::from((perpllcon0.pdiv().get() + 1) * (perpllcon1.k2div().get() + 1));
    f as u32
}

pub(crate) fn get_per_pll_frequency2() -> u32 {
    let osc_freq = get_osc_frequency();
    let perpllcon0 = unsafe { SCU.perpllcon0().read() };
    let perpllcon1 = unsafe { SCU.perpllcon1().read() };

    let multiplier = if perpllcon0.divby().get().0 == 1 {
        2.0
    } else {
        1.6
    };

    let f = (osc_freq * f32::from(perpllcon0.ndiv().get() + 1))
        / (f32::from(perpllcon0.pdiv().get() + 1)
            * f32::from(perpllcon1.k2div().get() + 1)
            * multiplier);
    f as u32
}

pub struct SysPllConfig {
    pub p_divider: u8,
    pub n_divider: u8,
    pub k2_divider: u8,
}

pub struct PerPllConfig {
    pub p_divider: u8,
    pub n_divider: u8,
    pub k2_divider: u8,
    pub k3_divider: u8,
    pub k3_divider_bypass: u8,
}

#[repr(u8)]
#[derive(Clone, Copy)]
pub enum PllInputClockSelection {
    F0sc1,
    F0sc0,
    FSynclk,
}

pub struct PllsParameterConfig {
    pub xtal_frequency: u32,
    pub pll_input_clock_selection: PllInputClockSelection,
    pub sys_pll: SysPllConfig,
    pub per_pll: PerPllConfig,
}

pub struct InitialConfigStep {
    pub plls_parameters: PllsParameterConfig,
    pub wait_time: f32,
}

pub struct PllStepConfig {
    pub k2_step: u8,
    pub wait_time: f32,
}

pub struct ConRegConfig {
    pub value: u32,
    pub mask: u32,
}

pub struct ClockDistributionConfig {
    pub ccucon0: ConRegConfig,
    pub ccucon1: ConRegConfig,
    pub ccucon2: ConRegConfig,
    pub ccucon5: ConRegConfig,
    pub ccucon6: ConRegConfig,
    pub ccucon7: ConRegConfig,
    pub ccucon8: ConRegConfig,
}

pub struct FlashWaitStateConfig {
    pub value: u32,
    pub mask: u32,
}

#[repr(u8)]
pub enum ModulationEn {
    Disabled,
    Enabled,
}

#[repr(u8)]
#[derive(Clone, Copy)]
pub enum ModulationAmplitude {
    _0p5,
    _1p0,
    _1p25,
    _1p5,
    _2p0,
    _2p5,
}

pub struct ModulationConfig {
    pub enable: ModulationEn,
    pub amp: ModulationAmplitude,
}

pub struct Config {
    pub pll_initial_step: InitialConfigStep,
    pub sys_pll_throttle: &'static [PllStepConfig],
    pub clock_distribution: ClockDistributionConfig,
    pub flash_wait_state: FlashWaitStateConfig,
    pub modulation: ModulationConfig,
}

pub const DEFAULT_PLL_CONFIG_STEPS: [PllStepConfig; 3] = [
    PllStepConfig {
        k2_step: 4 - 1,
        wait_time: 0.000100,
    },
    PllStepConfig {
        k2_step: 3 - 1,
        wait_time: 0.000100,
    },
    PllStepConfig {
        k2_step: 2 - 1,
        wait_time: 0.000100,
    },
];

pub const DEFAULT_CLOCK_CONFIG: Config = Config {
    pll_initial_step: InitialConfigStep {
        plls_parameters: PllsParameterConfig {
            xtal_frequency: 20000000,
            pll_input_clock_selection: PllInputClockSelection::F0sc0,
            sys_pll: SysPllConfig {
                p_divider: 1 - 1,
                n_divider: 30 - 1,
                k2_divider: 6 - 1,
            },
            per_pll: PerPllConfig {
                p_divider: 1 - 1,
                n_divider: 32 - 1,
                k2_divider: 2 - 1,
                k3_divider: 2 - 1,
                k3_divider_bypass: 0,
            },
        },
        wait_time: 0.000200,
    },
    sys_pll_throttle: &DEFAULT_PLL_CONFIG_STEPS,
    clock_distribution: ClockDistributionConfig {
        ccucon0: ConRegConfig {
            value: ((3) << (0))
                | ((1) << (4))
                | ((1) << (8))
                | ((3) << (16))
                | ((2) << (20))
                | (((1) * 3) << (24))
                | (((1) * 1) << (26)),
            mask: ((0xf) << (0))
                | ((0xf) << (4))
                | ((0xf) << (8))
                | ((0xf) << (16))
                | ((0xf) << (20))
                | ((0x3) << (24))
                | ((0x3) << (26)),
        },
        ccucon1: ConRegConfig {
            value: ((2) << (0))
                | ((1) << (4))
                | ((0) << (7))
                | ((2) << (8))
                | ((1) << (16))
                | ((1) << (20))
                | ((1) << (24))
                | ((2) << (28)),
            mask: ((0xf) << (0))
                | ((0x3) << (4))
                | ((0x1) << (7))
                | ((0xf) << (8))
                | ((0xf) << (16))
                | ((0x3) << (20))
                | ((0xf) << (24))
                | ((0x3) << (28)),
        },
        ccucon2: ConRegConfig {
            value: ((1) << (0)) | ((2) << (8)) | ((1) << (12)),
            mask: ((0xf) << (0)) | ((0xf) << (8)) | ((0x3) << (12)),
        },
        ccucon5: ConRegConfig {
            value: (((2) << (0)) | ((3) << (4))),
            mask: ((0xf) << (0)) | ((0xf) << (4)),
        },
        ccucon6: ConRegConfig {
            value: 0 << 0,
            mask: 0x3f << 0,
        },
        ccucon7: ConRegConfig {
            value: 0 << 0,
            mask: 0x3f << 0,
        },
        ccucon8: ConRegConfig {
            value: 0 << 0,
            mask: 0x3f << 0,
        },
    },
    flash_wait_state: FlashWaitStateConfig {
        value: 0x00000105,
        mask: 0x0000073F,
    },
    modulation: ModulationConfig {
        enable: ModulationEn::Disabled,
        amp: ModulationAmplitude::_0p5,
    },
};

pub(crate) fn get_mcan_frequency() -> u32 {
    //TODO create enum!
    const CLKSELMCAN_USEMCANI:u32 = 1; // scu::Ccucon1::Clkselmcan = scu::Ccucon1::Clkselmcan::CONST_11;
    const CLKSELMCAN_USEOSCILLATOR: u32 = 2; //scu::Ccucon1::Clkselmcan = scu::Ccucon1::Clkselmcan::CONST_22;
    const MCANDIV_STOPPED: u32 = 0; //scu::Ccucon1::Mcandiv = scu::Ccucon1::Mcandiv::CONST_00;

    // SAFETY: each bit of CCUCON1 is at least R
    let ccucon1 = unsafe { SCU.ccucon1().read() };
    let clkselmcan = ccucon1.clkselmcan().get();
    let mcandiv = ccucon1.mcandiv().get();

    //info!("clkselmcan: {}, mcandiv: {}", clkselmcan, mcandiv);

    match clkselmcan {
        CLKSELMCAN_USEMCANI => {
            let source = get_source_frequency(1);
            debug!("source: {}", source);
            if mcandiv == MCANDIV_STOPPED {
                source
            } else {
                let div: u64 = mcandiv.into();
                let div: u32 = div as u32;
                source / div
            }
        }
        CLKSELMCAN_USEOSCILLATOR => get_osc0_frequency(),
        _ => 0,
    }
}

fn get_source_frequency(source: u32) -> u32 {
    const CLKSEL_BACKUP:u8 = 0; // TODO create enum 
    const CLKSEL_PLL:u8 = 1;

    // SAFETY: each bit of CCUCON0 is at least R
    let clksel = unsafe { SCU.ccucon0().read() }.clksel().get();
    //info!("clksel: {}", clksel);

    match clksel.0 {
        CLKSEL_BACKUP => get_evr_frequency(),
        CLKSEL_PLL => match source {
            0 => get_pll_frequency(),
            1 => {
                let source_freq = get_per_pll_frequency1();
                // SAFETY: each bit of CCUCON1 is at least R
                let ccucon1 = unsafe { SCU.ccucon1().read() };
                if ccucon1.pll1divdis().get().0 == 1 {
                    source_freq
                } else {
                    source_freq / 2
                }
            }
            2 => get_per_pll_frequency2(),
            _ => unreachable!(),
        },
        _ => 0,
    }
}

fn get_evr_frequency() -> u32 {
    EVR_OSC_FREQUENCY
}

pub(crate) fn get_osc0_frequency() -> u32 {
    XTAL_FREQUENCY
}
