//! Math utilities and reexports

pub use glamx::{FloatExt, Vec2Swizzles, Vec3Swizzles, Vec4Swizzles, MatExt, EulerRot};

/// 2D, single-precision vector
pub type Vec2 = glamx::Vec2;
/// 3D, single-precision vector
pub type Vec3 = glamx::Vec3;
/// 4D, single-precision vector
pub type Vec4 = glamx::Vec4;

/// 2x2, single-precision matrix
pub type Mat2x2 = glamx::Mat2;
/// 2x3, single-precision matrix
pub type Mat2x3 = glamx::Affine2;
/// 3x3, single-precision matrix
pub type Mat3x3 = glamx::Mat3;
/// 3x4, single-precision matrix
pub type Mat3x4 = glamx::Affine3;
/// 4x4, single-precision matrix
pub type Mat4x4 = glamx::Mat4;

/// Single-precision complex 2D rotation
pub type Rot2d = glamx::Rot2;

/// Single-precision quaternion
pub type Quat = glamx::Quat;

/// 2D, single-precision translation and rotation
pub type Pose2 = glamx::Pose2;
/// 3D, single-precision translation and rotation
pub type Pose3 = glamx::Pose3;



