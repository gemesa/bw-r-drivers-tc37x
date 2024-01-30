use crate::scu::wdt;

pub fn call_without_endinit<R>(f: impl FnOnce() -> R) -> R {
    call_without_cpu_endinit(|| call_without_safety_endinit(f))
}

pub fn call_without_cpu_endinit<R>(f: impl FnOnce() -> R) -> R {
    wdt::clear_cpu_endinit_inline();
    let result = f();
    wdt::set_cpu_endinit_inline();
    result
}

pub fn call_without_safety_endinit<R>(f: impl FnOnce() -> R) -> R {
    wdt::clear_safety_endinit_inline();
    let result = f();
    wdt::set_safety_endinit_inline();
    result
}
