#[inline]
pub fn clamp_i32(value: i32, min: i32, max: i32) -> i32 {
    value.clamp(min, max)
}
