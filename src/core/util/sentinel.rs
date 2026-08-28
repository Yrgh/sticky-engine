//! Alternatives to `Option<NonZero>`

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// Represents "`None`" with `u32::MAX`, but has the same size as a u32.
pub struct SentinelMaxU32 {
    inner: u32
}

impl SentinelMaxU32 {
    /// The sentinel value representing `None`.
    pub const NONE: Self = Self::from_inner(u32::MAX);
    
    /// Converts a `u32` to a `SentinelMaxU32`, not caring whether or not it is `Some`.
    pub const fn from_inner(inner: u32) -> Self {
        Self {
            inner
        }
    }

    /// Same as [`Self::from_inner`], but panics if `some` is `u32::MAX`.
    pub const fn from_some(some: u32) -> Self {
        if some == u32::MAX {
            panic!("SentinelMaxU32::from_some was given u32::MAX");
        }

        Self {
            inner: some
        }
    }

    /// Same as [`Self::from_some`], but returns `None` if the input is `None`.
    pub const fn from_option(opt: Option<u32>) -> Self {
        match opt {
            Some(u32::MAX) => panic!("SentinelMaxU32::from_option was given u32::MAX"),
            Some(some) => Self::from_inner(some),
            None => Self::NONE,
        }
    }

    /// Same as [`Self::from_inner`], but returns `None` if `some` is `u32::MAX`.
    pub const fn try_from_some(some: u32) -> Option<Self> {
        if some == u32::MAX {
            None
        } else {
            Some(Self::from_inner(some))
        }
    }

    /// Returns the underlying `u32`. If `self` is `None`, returns `u32::MAX`.
    pub const fn into_inner(self) -> u32 {
        self.inner
    }

    /// Returns `true` if `self` is `Some`.
    pub const fn is_some(&self) -> bool {
        self.inner != u32::MAX
    }

    /// Returns `self`, leaving `None` in its place.
    pub const fn take(&mut self) -> Self {
        std::mem::replace(self, Self::NONE)
    }
}

impl Default for SentinelMaxU32 {
    fn default() -> Self {
        Self::NONE
    }
}