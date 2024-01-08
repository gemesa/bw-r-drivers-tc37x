use crate::can::{
    can_node::{FrameMode, MessageIdLenght},
    frame::DataLenghtCode,
    //types::{DataLenghtCode, FrameMode, MessageIdLenght},
    reg,
};
// create RxMsg using pac structure and unsafe transmute

pub struct Rx {
    inner: reg::RxMsg,
}

impl Rx {
    pub fn new(ptr: *mut u8) -> Self {
        Self {
            inner: unsafe { core::mem::transmute(ptr) },
        }
    }

    pub fn get_ptr(&self) -> *mut u8 {
        unsafe { core::mem::transmute(self.inner) }
    }
}

impl Rx {
    #[inline]
    pub fn get_message_id(&self) -> u32 {
        let r0 = unsafe { self.inner.r0().read() };
        let message_lenght = if r0.xtd().get() {
            MessageIdLenght::Extended
        } else {
            MessageIdLenght::Standard
        };

        let id = r0.id().get();
        let shift = if message_lenght == MessageIdLenght::Standard {
            18
        } else {
            0
        };
        id >> shift
    }

    #[inline]
    pub fn get_message_id_lenght(&self) -> MessageIdLenght {
        if unsafe { self.inner.r0().read() }.xtd().get() {
            MessageIdLenght::Extended
        } else {
            MessageIdLenght::Standard
        }
    }

    #[inline]
    pub fn get_data_lenght(&self) -> DataLenghtCode {
        let d = unsafe { self.inner.r1().read() }.dlc().get();
        DataLenghtCode::try_from(d).unwrap()
    }

    pub fn get_frame_mode(&self) -> FrameMode {
        let r1 = unsafe { self.inner.r1().read() };

        if r1.fdf().get() {
            if r1.brs().get() {
                FrameMode::FdLongAndFast
            } else {
                FrameMode::FdLong
            }
        } else {
            FrameMode::Standard
        }
    }

    pub fn read_data(&self, data_lenght_code: DataLenghtCode, data: *mut u8) {
        let source_address = self.inner.db().ptr() as _;
        let lenght = data_lenght_code.get_data_lenght_in_bytes();

        #[cfg(feature = "log")]
        defmt::debug!("reading {} bytes from {:x}", lenght, source_address);

        unsafe { core::ptr::copy_nonoverlapping(source_address, data, lenght as _) };
    }
}
