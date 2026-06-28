use core::ops::{Add, AddAssign, Sub, SubAssign};
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct PhysAddr(u64);

impl PhysAddr {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }
    pub const fn is_aligned(self, alignment: usize) -> bool {
        self.0 & (alignment as u64 - 1) == 0
    }

    pub const fn align_down(self, alignment: usize) -> Self {
        Self(self.0 & !(alignment as u64 - 1))
    }

    pub const fn align_up(self, alignment: usize) -> Self {
        Self((self.0 + (alignment as u64 - 1)) & !(alignment as u64 - 1))
    }

    pub const fn page_offset(self) -> usize {
        (self.0 & 0xfff) as usize
    }

    pub const fn page_number(self) -> usize {
        (self.0 >> 12) as usize
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct VirtAddr(u64);

impl VirtAddr {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }
    pub const fn is_aligned(self, alignment: usize) -> bool {
        self.0 & (alignment as u64 - 1) == 0
    }

    pub const fn align_down(self, alignment: usize) -> Self {
        Self(self.0 & !(alignment as u64 - 1))
    }

    pub const fn align_up(self, alignment: usize) -> Self {
        Self((self.0 + (alignment as u64 - 1)) & !(alignment as u64 - 1))
    }

    pub const fn page_offset(self) -> usize {
        (self.0 & 0xfff) as usize
    }

    pub const fn page_number(self) -> usize {
        (self.0 >> 12) as usize
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PagingRoot {
    phys: PhysAddr,
}

impl PagingRoot {
    pub const fn new(phys: PhysAddr) -> Self {
        Self { phys }
    }

    pub const fn phys_addr(self) -> PhysAddr {
        self.phys
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PageFlags(u64);

impl PageFlags {
    pub const PRESENT: Self = Self(1 << 0);
    pub const WRITABLE: Self = Self(1 << 1);
    pub const USER: Self = Self(1 << 2);
    pub const GLOBAL: Self = Self(1 << 8);
    pub const NO_EXECUTE: Self = Self(1 << 63);
    pub const NO_CACHE: Self = Self(1 << 4);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl core::ops::BitOr for PageFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitOrAssign for PageFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}
impl Add<u64> for PhysAddr {
    type Output = Self;

    fn add(self, rhs: u64) -> Self {
        Self(self.0 + rhs)
    }
}

impl Sub<u64> for PhysAddr {
    type Output = Self;

    fn sub(self, rhs: u64) -> Self {
        Self(self.0 - rhs)
    }
}

impl Sub for PhysAddr {
    type Output = u64;

    fn sub(self, rhs: Self) -> u64 {
        self.0 - rhs.0
    }
}
impl Add<u64> for VirtAddr {
    type Output = Self;

    fn add(self, rhs: u64) -> Self {
        Self(self.0 + rhs)
    }
}

impl Sub<u64> for VirtAddr {
    type Output = Self;

    fn sub(self, rhs: u64) -> Self {
        Self(self.0 - rhs)
    }
}

impl Sub for VirtAddr {
    type Output = u64;

    fn sub(self, rhs: Self) -> u64 {
        self.0 - rhs.0
    }
}
impl AddAssign<u64> for PhysAddr {
    fn add_assign(&mut self, rhs: u64) {
        self.0 += rhs;
    }
}

impl SubAssign<u64> for PhysAddr {
    fn sub_assign(&mut self, rhs: u64) {
        self.0 -= rhs;
    }
}

impl AddAssign<u64> for VirtAddr {
    fn add_assign(&mut self, rhs: u64) {
        self.0 += rhs;
    }
}

impl SubAssign<u64> for VirtAddr {
    fn sub_assign(&mut self, rhs: u64) {
        self.0 -= rhs;
    }
}
