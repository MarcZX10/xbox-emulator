#[link(name = "winmm")]
extern "system" {
    fn timeBeginPeriod(period: u32) -> u32;
}

static RESULT: std::sync::OnceLock<u32> = std::sync::OnceLock::new();

pub fn console_timer() -> u32 {
    *RESULT.get_or_init(|| unsafe { timeBeginPeriod(1) })
}

#[used]
#[link_section = ".CRT$XCU"]
pub static CONSOLE_TIMER_INIT: extern "C" fn() = console_timer_init;

extern "C" fn console_timer_init() {
    if std::env::var("TOMBOX_TIMER_1MS").ok().as_deref() != Some("0") {
        console_timer();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn one_millisecond_timer_is_active() {
        assert_eq!(super::console_timer(), 0);
    }
}