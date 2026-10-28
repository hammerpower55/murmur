pub mod capture;
pub mod resample;
pub mod wav;

/// Every engine takes 16 kHz mono f32.
pub const TARGET_RATE: u32 = 16_000;
