//! Serde defaults shared by the configuration documents; defaults used by one module live
//! next to its types.

pub(crate) const fn yes() -> bool {
    true
}
pub(crate) const fn three() -> u32 {
    3
}
pub(crate) const fn secs_10() -> u32 {
    10
}
pub(crate) const fn secs_30() -> u32 {
    30
}
pub(crate) const fn secs_60() -> u32 {
    60
}
pub(crate) const fn ms_1000() -> u32 {
    1000
}
pub(crate) const fn ms_5000() -> u32 {
    5000
}
pub(crate) const fn ms_30000() -> u32 {
    30_000
}
pub(crate) const fn mib_1() -> u64 {
    1024 * 1024
}
